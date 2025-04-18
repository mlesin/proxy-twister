use clap::Parser;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{self, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tracing::{debug, error, info};

mod config;
mod protocols;
mod utils;

use config::watcher::spawn_config_watcher;
use config::{Config, Profile};
use protocols::{http, socks};
use utils::matches_pattern;

/// SOCKS5 proxy switcher that routes traffic based on target host patterns
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Config file path
    #[arg(short, long)]
    config: String,

    /// Address to listen on
    #[arg(short, long, default_value = "127.0.0.1")]
    address: String,

    /// Port to listen on
    #[arg(short, long, default_value_t = 1080)]
    port: u16,
}

fn select_profile(config: &Config, target_host: &str) -> String {
    let mut selected = config.switch.default.clone();
    for rule in config.switch.rules.iter() {
        let pattern = &rule.pattern;
        if matches_pattern(target_host, pattern) {
            selected = rule.profile.clone();
            break;
        }
    }
    selected
}

async fn extract_host_and_port(
    client: &mut tokio::net::TcpStream,
    request: &http::HttpRequest,
) -> io::Result<(String, u16)> {
    if request.method == "CONNECT" {
        return http::handle_connect(client, request.clone()).await;
    }

    // For non-CONNECT requests, extract host from headers or target
    let host = request
        .headers
        .get("host")
        .cloned()
        .or_else(|| {
            let uri = request.target.clone();
            if let Some(uri) = uri.strip_prefix("http://") {
                // Extract host from absolute URI
                uri.split('/').next().map(|h| h.to_string())
            } else {
                None
            }
        })
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "No host header in request"))?;

    let parts: Vec<&str> = host.split(':').collect();
    let host_without_port = parts[0].to_string();
    let port = if parts.len() > 1 {
        parts[1].parse().unwrap_or(80)
    } else {
        80 // Default HTTP port
    };

    Ok((host_without_port, port))
}

async fn handle_direct_connection(
    mut client: tokio::net::TcpStream,
    request: &http::HttpRequest,
    target_host: &str,
    port: u16,
) -> io::Result<()> {
    if request.method == "CONNECT" {
        match tokio::net::TcpStream::connect(format!("{}:{}", target_host, port)).await {
            Ok(target_stream) => {
                // For direct connections, just pipe data between client and target
                let (mut ri, mut wi) = client.into_split();
                let (mut ro, mut wo) = target_stream.into_split();
                tokio::try_join!(io::copy(&mut ri, &mut wo), io::copy(&mut ro, &mut wi))?;
            }
            Err(e) => {
                error!("Could not connect directly to {}: {}", target_host, e);
                client.write_all(http::HTTP_SERVER_ERROR.as_bytes()).await?;
            }
        }
    } else {
        // For regular HTTP requests, forward directly
        match tokio::net::TcpStream::connect(format!("{}:{}", target_host, port)).await {
            Ok(target_stream) => {
                // Modify request target to be path-only for direct connection
                let mut modified_request = request.clone();
                if modified_request.target.starts_with("http://") {
                    modified_request.target = modified_request
                        .target
                        .splitn(4, '/')
                        .nth(3)
                        .map(|p| format!("/{}", p))
                        .unwrap_or_else(|| "/".to_string());
                }

                // Forward the modified request
                http::forward_http_request(
                    &modified_request,
                    target_host,
                    port,
                    target_host,
                    port,
                    None,
                )
                .await?;

                // Copy response back to client
                let (mut ri, mut wi) = client.into_split();
                let (mut ro, mut wo) = target_stream.into_split();
                tokio::try_join!(io::copy(&mut ro, &mut wi), io::copy(&mut ri, &mut wo))?;
            }
            Err(e) => {
                error!("Could not connect directly to {}: {}", target_host, e);
                client.write_all(http::HTTP_SERVER_ERROR.as_bytes()).await?;
            }
        }
    }

    Ok(())
}

async fn handle_proxy_connection(
    mut client: tokio::net::TcpStream,
    request: &http::HttpRequest,
    target_host: &str,
    port: u16,
    proxy: &Profile,
) -> io::Result<()> {
    match proxy {
        Profile::Socks5 {
            host,
            port: proxy_port,
        } => {
            info!(
                "Using Socks5 proxy {}:{} for {}:{}",
                host, proxy_port, target_host, port
            );
            let socks5_request = socks::Socks5Request {
                target: target_host.to_string(),
                port, // Use the target port from client request
            };
            let proxy_stream_result =
                socks::forward_to_proxy(&socks5_request, host, *proxy_port).await;
            match proxy_stream_result {
                Ok(mut proxy_stream) => {
                    if request.method == "CONNECT" {
                        // For CONNECT, just tunnel data
                        let (mut ci, mut co) = client.into_split();
                        let (mut pi, mut po) = proxy_stream.into_split();
                        tokio::try_join!(io::copy(&mut ci, &mut po), io::copy(&mut pi, &mut co))?;
                    } else {
                        // For HTTP, send the request through the tunnel
                        let mut http_req =
                            format!("{} {} HTTP/1.1\r\n", request.method, request.target);
                        for (k, v) in &request.headers {
                            http_req.push_str(&format!("{}: {}\r\n", k, v));
                        }
                        http_req.push_str("\r\n");
                        proxy_stream.write_all(http_req.as_bytes()).await?;
                        if !request.body.is_empty() {
                            proxy_stream.write_all(&request.body).await?;
                        }
                        let (mut ci, mut co) = client.into_split();
                        let (mut pi, mut po) = proxy_stream.into_split();
                        tokio::try_join!(io::copy(&mut pi, &mut co), io::copy(&mut ci, &mut po))?;
                    }
                }
                Err(e) => {
                    error!("Could not connect through proxy: {}", e);
                    client.write_all(http::HTTP_SERVER_ERROR.as_bytes()).await?;
                }
            }
        }
        Profile::Http {
            host,
            port: proxy_port,
        } => {
            info!(
                "Using HTTP proxy {}:{} for {}:{}",
                host, proxy_port, target_host, port
            );
            let proxy_stream = if request.method == "CONNECT" {
                http::forward_to_proxy(target_host, port, host, *proxy_port, None).await
            } else {
                http::forward_http_request(request, target_host, port, host, *proxy_port, None)
                    .await
            };
            match proxy_stream {
                Ok(proxy_stream) => {
                    let (mut ci, mut co) = client.into_split();
                    let (mut pi, mut po) = proxy_stream.into_split();
                    tokio::try_join!(io::copy(&mut ci, &mut po), io::copy(&mut pi, &mut co))?;
                }
                Err(e) => {
                    error!("Could not connect through proxy: {}", e);
                    client.write_all(http::HTTP_SERVER_ERROR.as_bytes()).await?;
                }
            }
        }
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid proxy type",
            ));
        }
    }

    Ok(())
}

async fn handle_client(
    mut client: tokio::net::TcpStream,
    config: Arc<RwLock<Config>>,
) -> io::Result<()> {
    // Parse HTTP proxy request
    let request = http::parse_request(&mut client).await?;

    // Extract target host and port from the request
    let (target_host, port) = extract_host_and_port(&mut client, &request).await?;

    // Read config under lock
    let config_guard = config.read().await;
    let chosen_profile_name = select_profile(&config_guard, &target_host);

    debug!(
        "Target is '{}', using '{}' profile",
        target_host, chosen_profile_name
    );

    // Handle the connection based on the chosen profile
    if let Some(proxy) = config_guard.profiles.get(&chosen_profile_name) {
        match proxy {
            Profile::Direct => {
                handle_direct_connection(client, &request, &target_host, port).await?;
            }
            Profile::Socks5 { .. } | Profile::Http { .. } => {
                handle_proxy_connection(client, &request, &target_host, port, proxy).await?;
            }
        }
    } else {
        error!("Profile {} not found in configuration", chosen_profile_name);
        client.write_all(http::HTTP_SERVER_ERROR.as_bytes()).await?;
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let args = Args::parse();
    let config_path = args.config.clone();
    let config = match Config::load(&config_path) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("Configuration error: {}", e);
            std::process::exit(1);
        }
    };
    let config = Arc::new(RwLock::new(config));

    // Use the new watcher module
    spawn_config_watcher(PathBuf::from(config_path.clone()), config.clone());

    let listener_addr = format!("{}:{}", args.address, args.port);
    let listener = TcpListener::bind(&listener_addr).await?;
    info!("HTTP proxy switcher listening on {}", listener_addr);

    loop {
        let (client_socket, _addr) = listener.accept().await?;
        let config_clone = config.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_client(client_socket, config_clone).await {
                error!("Error handling connection: {:?}", e);
            }
        });
    }
}
