use crate::config::Config;
use crate::protocols::{http, socks};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info};

fn select_profile(config: &Config, target_host: &str) -> String {
    let mut selected = config.switch.default.clone();
    for rule in config.switch.rules.iter() {
        let pattern = &rule.pattern;
        if crate::utils::matches_pattern(target_host, pattern) {
            selected = rule.profile.clone();
            break;
        }
    }
    selected
}

async fn extract_host_and_port(
    client: &mut tokio::net::TcpStream,
    request: &http::HttpRequest,
) -> tokio::io::Result<(String, u16)> {
    if request.method == "CONNECT" {
        return http::handle_connect(client, request.clone()).await;
    }
    let host = request
        .headers
        .get("host")
        .cloned()
        .or_else(|| {
            let uri = request.target.clone();
            if let Some(uri) = uri.strip_prefix("http://") {
                uri.split('/').next().map(|h| h.to_string())
            } else {
                None
            }
        })
        .ok_or_else(|| {
            tokio::io::Error::new(
                tokio::io::ErrorKind::InvalidData,
                "No host header in request",
            )
        })?;
    let parts: Vec<&str> = host.split(':').collect();
    let host_without_port = parts[0].to_string();
    let port = if parts.len() > 1 {
        parts[1].parse().unwrap_or(80)
    } else {
        80
    };
    Ok((host_without_port, port))
}

async fn handle_direct_connection(
    mut client: tokio::net::TcpStream,
    request: &http::HttpRequest,
    target_host: &str,
    port: u16,
) -> tokio::io::Result<()> {
    if request.method == "CONNECT" {
        match tokio::net::TcpStream::connect(format!("{}:{}", target_host, port)).await {
            Ok(target_stream) => {
                let (mut ri, mut wi) = client.into_split();
                let (mut ro, mut wo) = target_stream.into_split();
                tokio::try_join!(
                    tokio::io::copy(&mut ri, &mut wo),
                    tokio::io::copy(&mut ro, &mut wi)
                )?;
            }
            Err(e) => {
                error!("Could not connect directly to {}: {}", target_host, e);
                client.write_all(http::HTTP_SERVER_ERROR.as_bytes()).await?;
            }
        }
    } else {
        match tokio::net::TcpStream::connect(format!("{}:{}", target_host, port)).await {
            Ok(target_stream) => {
                let mut modified_request = request.clone();
                if modified_request.target.starts_with("http://") {
                    modified_request.target = modified_request
                        .target
                        .splitn(4, '/')
                        .nth(3)
                        .map(|p| format!("/{}", p))
                        .unwrap_or_else(|| "/".to_string());
                }
                http::forward_http_request(
                    &modified_request,
                    target_host,
                    port,
                    target_host,
                    port,
                    None,
                )
                .await?;
                let (mut ri, mut wi) = client.into_split();
                let (mut ro, mut wo) = target_stream.into_split();
                tokio::try_join!(
                    tokio::io::copy(&mut ro, &mut wi),
                    tokio::io::copy(&mut ri, &mut wo)
                )?;
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
    proxy: &crate::config::Profile,
) -> tokio::io::Result<()> {
    match proxy {
        crate::config::Profile::Socks5 {
            host,
            port: proxy_port,
        } => {
            info!(
                "Using Socks5 proxy {}:{} for {}:{}",
                host, proxy_port, target_host, port
            );
            let socks5_request = socks::Socks5Request {
                target: target_host.to_string(),
                port,
            };
            let proxy_stream_result =
                socks::forward_to_proxy(&socks5_request, host, *proxy_port).await;
            match proxy_stream_result {
                Ok(mut proxy_stream) => {
                    if request.method == "CONNECT" {
                        let (mut ci, mut co) = client.into_split();
                        let (mut pi, mut po) = proxy_stream.into_split();
                        tokio::try_join!(
                            tokio::io::copy(&mut ci, &mut po),
                            tokio::io::copy(&mut pi, &mut co)
                        )?;
                    } else {
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
                        tokio::try_join!(
                            tokio::io::copy(&mut pi, &mut co),
                            tokio::io::copy(&mut ci, &mut po)
                        )?;
                    }
                }
                Err(e) => {
                    error!("Could not connect through proxy: {}", e);
                    client.write_all(http::HTTP_SERVER_ERROR.as_bytes()).await?;
                }
            }
        }
        crate::config::Profile::Http {
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
                    tokio::try_join!(
                        tokio::io::copy(&mut ci, &mut po),
                        tokio::io::copy(&mut pi, &mut co)
                    )?;
                }
                Err(e) => {
                    error!("Could not connect through proxy: {}", e);
                    client.write_all(http::HTTP_SERVER_ERROR.as_bytes()).await?;
                }
            }
        }
        _ => {
            return Err(tokio::io::Error::new(
                tokio::io::ErrorKind::InvalidInput,
                "Invalid proxy type",
            ));
        }
    }
    Ok(())
}

async fn handle_client(
    mut client: tokio::net::TcpStream,
    config: Arc<RwLock<Config>>,
    _cancel_token: CancellationToken,
) -> tokio::io::Result<()> {
    let request = http::parse_request(&mut client).await?;
    let (target_host, port) = extract_host_and_port(&mut client, &request).await?;
    let config_guard = config.read().await;
    let chosen_profile_name = select_profile(&config_guard, &target_host);
    debug!(
        "Target is '{}', using '{}' profile",
        target_host, chosen_profile_name
    );
    if let Some(proxy) = config_guard.profiles.get(&chosen_profile_name) {
        match proxy {
            crate::config::Profile::Direct => {
                handle_direct_connection(client, &request, &target_host, port).await?;
            }
            crate::config::Profile::Socks5 { .. } | crate::config::Profile::Http { .. } => {
                handle_proxy_connection(client, &request, &target_host, port, proxy).await?;
            }
        }
    } else {
        error!("Profile {} not found in configuration", chosen_profile_name);
        client.write_all(http::HTTP_SERVER_ERROR.as_bytes()).await?;
    }
    Ok(())
}

pub async fn run_listener(
    addr: String,
    config: Arc<RwLock<Config>>,
    cancel_token: CancellationToken,
) {
    let listener = match TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            error!("Failed to bind to {}: {}", addr, e);
            return;
        }
    };
    info!("Listening on {}", addr);
    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                info!("Listener on {} received shutdown signal", addr);
                break;
            }
            accept_result = listener.accept() => {
                match accept_result {
                    Ok((client_socket, _addr)) => {
                        let config = config.clone();
                        let cancel_token = cancel_token.clone();
                        tokio::spawn(async move {
                            let _ = handle_client(client_socket, config, cancel_token).await;
                        });
                    }
                    Err(e) => {
                        error!("Accept error on {}: {:?}", addr, e);
                    }
                }
            }
        }
    }
}
