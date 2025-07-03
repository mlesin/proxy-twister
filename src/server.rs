use crate::config::Config;
use crate::protocols::{http, socks};
use std::sync::{Arc, Mutex};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, trace};

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
    trace!(
        "extract_host_and_port: method={}, target={}, headers={:?}",
        request.method, request.target, request.headers
    );

    if request.method == "CONNECT" {
        return http::handle_connect(client, request.clone()).await;
    }
    let host = request
        .headers
        .get("host")
        .cloned()
        .or_else(|| {
            let uri = request.target.clone();
            trace!(
                "extract_host_and_port: trying to extract host from URI: {}",
                uri
            );
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

    trace!("extract_host_and_port: extracted host string: '{}'", host);

    let parts: Vec<&str> = host.split(':').collect();
    let host_without_port = parts[0].to_string();
    let port = if parts.len() > 1 {
        parts[1].parse().unwrap_or(80)
    } else {
        // Use the url crate to parse the scheme and determine the default port
        match url::Url::parse(&request.target) {
            Ok(url) => match url.scheme().to_ascii_lowercase().as_str() {
                "https" => 443,
                "http" => 80,
                _ => 80,
            },
            Err(_) => 80,
        }
    };

    trace!(
        "extract_host_and_port: final result - host: '{}', port: {}",
        host_without_port, port
    );
    Ok((host_without_port, port))
}

async fn handle_direct_connection(
    mut client: tokio::net::TcpStream,
    request: &http::HttpRequest,
    target_host: &str,
    port: u16,
) -> tokio::io::Result<()> {
    if request.method == "CONNECT" {
        trace!("Attempting direct CONNECT to {}:{}", target_host, port);
        match tokio::net::TcpStream::connect(format!("{target_host}:{port}")).await {
            Ok(target_stream) => {
                trace!("Successfully connected to {}:{}", target_host, port);

                // Set socket options for better performance
                if let Err(e) = target_stream.set_nodelay(true) {
                    trace!("Failed to set TCP_NODELAY on target stream: {}", e);
                }

                // Send 200 Connection Established to the client
                client
                    .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                    .await?;

                let (mut ri, mut wi) = client.into_split();
                let (mut ro, mut wo) = target_stream.into_split();
                tokio::try_join!(
                    tokio::io::copy(&mut ri, &mut wo),
                    tokio::io::copy(&mut ro, &mut wi)
                )?;
            }
            Err(e) => {
                error!(
                    "Could not connect directly to {}:{}: {} (error kind: {:?})",
                    target_host,
                    port,
                    e,
                    e.kind()
                );
                client.write_all(http::HTTP_SERVER_ERROR.as_bytes()).await?;
            }
        }
    } else {
        trace!(
            "Attempting direct HTTP connection to {}:{} using hyper",
            target_host, port
        );

        // Use our helper function to send the HTTP request
        match http::send_http_request(request, target_host, port).await {
            Ok((status, headers, body_bytes)) => {
                trace!(
                    "Received response from {}:{}: {:?}",
                    target_host, port, status
                );

                // Convert to HTTP/1.1 response string
                let status_code = status.as_u16();
                let reason = status.canonical_reason().unwrap_or("");

                let mut response_string = format!("HTTP/1.1 {status_code} {reason}\r\n");

                // Add response headers
                for (name, value) in headers {
                    response_string.push_str(&format!("{name}: {value}\r\n"));
                }

                // End headers section
                response_string.push_str("\r\n");

                // Write response headers to client
                client.write_all(response_string.as_bytes()).await?;

                // Write response body to client
                if !body_bytes.is_empty() {
                    client.write_all(&body_bytes).await?;
                }

                trace!("HTTP response sent successfully to client");
            }
            Err(e) => {
                error!("Failed to send request to {}:{}: {}", target_host, port, e);
                client.write_all(http::HTTP_SERVER_ERROR.as_bytes()).await?;
                return Err(std::io::Error::other(e.to_string()));
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
            trace!(
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
                        // Send 200 Connection Established to the client for CONNECT requests
                        client
                            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                            .await?;

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
                            http_req.push_str(&format!("{k}: {v}\r\n"));
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
                    error!(
                        "Could not connect through proxy to {}:{} : {}",
                        target_host, port, e
                    );
                    client.write_all(http::HTTP_SERVER_ERROR.as_bytes()).await?;
                }
            }
        }
        crate::config::Profile::Http {
            host,
            port: proxy_port,
        } => {
            trace!(
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
                    if request.method == "CONNECT" {
                        // Send 200 Connection Established to the client for CONNECT requests
                        client
                            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                            .await?;
                    }

                    let (mut ci, mut co) = client.into_split();
                    let (mut pi, mut po) = proxy_stream.into_split();
                    tokio::try_join!(
                        tokio::io::copy(&mut ci, &mut po),
                        tokio::io::copy(&mut pi, &mut co)
                    )?;
                }
                Err(e) => {
                    error!(
                        "Could not connect through proxy to {}:{} : {}",
                        target_host, port, e
                    );
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
    cancel_token: CancellationToken,
) -> tokio::io::Result<()> {
    // Check for cancellation before starting
    if cancel_token.is_cancelled() {
        return Ok(());
    }

    let request = http::parse_request(&mut client).await?;
    let (target_host, port) = extract_host_and_port(&mut client, &request).await?;

    trace!(
        "Extracted target_host: '{}', port: {}, method: '{}'",
        target_host, port, request.method
    );

    // IMPORTANT: Scope the read lock to ensure it's released as soon as we extract what we need
    let proxy_config = {
        let config_guard = config.read().await;
        let profile_name = select_profile(&config_guard, &target_host);
        debug!(
            "Target is '{}', using '{}' profile",
            target_host, profile_name
        );

        // Clone what we need from the config to avoid holding the lock

        match config_guard.profiles.get(&profile_name) {
            Some(p) => p.clone(),
            None => {
                error!("Profile {} not found in configuration", profile_name);
                client.write_all(http::HTTP_SERVER_ERROR.as_bytes()).await?;
                return Ok(());
            }
        }
    }; // read lock is released here

    // Process the request with our cloned data, without holding the lock
    match proxy_config {
        crate::config::Profile::Direct => {
            handle_direct_connection(client, &request, &target_host, port).await?;
        }
        crate::config::Profile::Socks5 { .. } | crate::config::Profile::Http { .. } => {
            handle_proxy_connection(client, &request, &target_host, port, &proxy_config).await?;
        }
    }

    Ok(())
}

pub async fn run_listener(
    addr: String,
    config: Arc<RwLock<Config>>,
    connections_token: Arc<Mutex<CancellationToken>>,
    shutdown_token: CancellationToken,
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
            _ = shutdown_token.cancelled() => {
                info!("Listener on {} received shutdown signal", addr);
                break;
            }
            accept_result = listener.accept() => {
                match accept_result {
                    Ok((client_socket, _addr)) => {
                        let config = config.clone();
                        let token = connections_token.clone();
                        tokio::spawn(async move {
                            // Get the current token for this connection
                            let current_token = { token.lock().unwrap().clone() };
                            let _ = handle_client(client_socket, config, current_token).await;
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
