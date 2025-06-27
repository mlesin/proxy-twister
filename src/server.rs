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
        "extract_host_and_port: method={}, target={}",
        request.method, request.target
    );
    trace!("extract_host_and_port: headers={:?}", request.headers);

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
        80
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
        debug!("Attempting direct CONNECT to {}:{}", target_host, port);
        match tokio::net::TcpStream::connect(format!("{}:{}", target_host, port)).await {
            Ok(target_stream) => {
                debug!("Successfully connected to {}:{}", target_host, port);

                // Set socket options for better performance
                if let Err(e) = target_stream.set_nodelay(true) {
                    trace!("Failed to set TCP_NODELAY on target stream: {}", e);
                }

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
            "Attempting direct HTTP connection to {}:{}",
            target_host, port
        );
        match tokio::net::TcpStream::connect(format!("{}:{}", target_host, port)).await {
            Ok(mut target_stream) => {
                trace!("Successfully connected to {}:{}", target_host, port);

                // Set socket options for better performance and stability
                if let Err(e) = target_stream.set_nodelay(true) {
                    trace!("Failed to set TCP_NODELAY on target stream: {}", e);
                }

                // Prepare the HTTP request for direct forwarding
                let mut modified_request = request.clone();

                // For direct connections, we need to convert absolute URLs to relative paths
                if modified_request.target.starts_with("http://") {
                    // Extract the path part from the full URL
                    if let Some(url_without_scheme) =
                        modified_request.target.strip_prefix("http://")
                    {
                        if let Some(slash_pos) = url_without_scheme.find('/') {
                            modified_request.target = url_without_scheme[slash_pos..].to_string();
                        } else {
                            modified_request.target = "/".to_string();
                        }
                    } else {
                        // Fallback if strip_prefix fails for some reason
                        modified_request.target = "/".to_string();
                    }
                } else if modified_request.target.starts_with("https://") {
                    // Handle HTTPS URLs (though they should typically use CONNECT)
                    if let Some(url_without_scheme) =
                        modified_request.target.strip_prefix("https://")
                    {
                        if let Some(slash_pos) = url_without_scheme.find('/') {
                            modified_request.target = url_without_scheme[slash_pos..].to_string();
                        } else {
                            modified_request.target = "/".to_string();
                        }
                    } else {
                        modified_request.target = "/".to_string();
                    }
                }

                // Ensure the target starts with '/' for proper HTTP request format
                if !modified_request.target.starts_with('/') {
                    modified_request.target = format!("/{}", modified_request.target);
                }

                // Build the HTTP request string - preserve original HTTP version if possible
                let http_version = "HTTP/1.1";
                let mut http_request = format!(
                    "{} {} {}\r\n",
                    modified_request.method, modified_request.target, http_version
                );

                // Add headers, ensuring Host header is present
                let mut has_host_header = false;
                let mut has_connection_header = false;
                for (key, value) in &modified_request.headers {
                    let key_lower = key.to_lowercase();
                    if key_lower == "host" {
                        has_host_header = true;
                    }
                    if key_lower == "connection" {
                        has_connection_header = true;
                    }
                    // Skip proxy-specific headers for direct connections
                    if !key_lower.starts_with("proxy-") {
                        http_request.push_str(&format!("{}: {}\r\n", key, value));
                    }
                }

                // Ensure Host header is present
                if !has_host_header {
                    if port == 80 {
                        http_request.push_str(&format!("Host: {}\r\n", target_host));
                    } else {
                        http_request.push_str(&format!("Host: {}:{}\r\n", target_host, port));
                    }
                }

                // For HTTP/1.1, ensure proper connection handling
                if !has_connection_header {
                    http_request.push_str("Connection: close\r\n");
                }

                // Add Content-Length if body is present
                if !modified_request.body.is_empty() {
                    http_request.push_str(&format!(
                        "Content-Length: {}\r\n",
                        modified_request.body.len()
                    ));
                }

                // End headers
                http_request.push_str("\r\n");

                // Send the HTTP request to the target server
                if let Err(e) = target_stream.write_all(http_request.as_bytes()).await {
                    error!(
                        "Failed to send HTTP request to {}:{}: {}",
                        target_host, port, e
                    );
                    return Err(e);
                }

                // Send body if present
                if !modified_request.body.is_empty() {
                    if let Err(e) = target_stream.write_all(&modified_request.body).await {
                        error!(
                            "Failed to send HTTP body to {}:{}: {}",
                            target_host, port, e
                        );
                        return Err(e);
                    }
                }

                // Flush the target stream to ensure data is sent
                if let Err(e) = target_stream.flush().await {
                    error!(
                        "Failed to flush target stream for {}:{}: {}",
                        target_host, port, e
                    );
                    return Err(e);
                }

                trace!("HTTP request sent successfully to {}:{}", target_host, port);

                // Now set up bidirectional forwarding
                let (mut client_read, mut client_write) = client.into_split();
                let (mut target_read, mut target_write) = target_stream.into_split();

                // Forward data in both directions concurrently
                match tokio::try_join!(
                    tokio::io::copy(&mut target_read, &mut client_write),
                    tokio::io::copy(&mut client_read, &mut target_write)
                ) {
                    Ok((bytes_to_client, bytes_to_target)) => {
                        trace!(
                            "Direct HTTP connection completed: {} bytes to client, {} bytes to target",
                            bytes_to_client, bytes_to_target
                        );
                    }
                    Err(e) => {
                        trace!("Direct HTTP connection ended with error: {}", e);
                        return Err(e);
                    }
                }
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
            debug!(
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
