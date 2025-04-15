use std::io;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{Duration, timeout};
use tracing::{debug, error};

// SOCKS5 protocol constants
pub const SOCKS_VERSION: u8 = 0x05;
pub const NO_AUTHENTICATION: u8 = 0x00;
pub const CONNECT_COMMAND: u8 = 0x01;
pub const IPV4_TYPE: u8 = 0x01;
pub const DOMAIN_TYPE: u8 = 0x03;
pub const IPV6_TYPE: u8 = 0x04;
pub const SUCCESS_REPLY: u8 = 0x00;

pub struct Socks5Request {
    pub target: String,
    pub port: u16,
}

pub async fn forward_to_proxy(
    request: &Socks5Request,
    proxy_host: &str,
    proxy_port: u16,
) -> io::Result<TcpStream> {
    debug!("Connecting to proxy at {}:{}", proxy_host, proxy_port);
    let mut proxy = TcpStream::connect(format!("{}:{}", proxy_host, proxy_port)).await?;

    proxy
        .write_all(&[SOCKS_VERSION, 1, NO_AUTHENTICATION])
        .await?;
    debug!("Sent authentication request to proxy");
    let mut response = [0u8; 2];
    proxy.read_exact(&mut response).await?;
    debug!("Received authentication response: {:?}", response);

    if response[0] != SOCKS_VERSION || response[1] != NO_AUTHENTICATION {
        error!("Proxy authentication failed: {:?}", response);
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "Proxy authentication failed",
        ));
    }

    proxy.write_all(&[SOCKS_VERSION]).await?;
    proxy.write_all(&[CONNECT_COMMAND]).await?;
    proxy.write_all(&[0x00]).await?;
    debug!("Sending SOCKS5 request to proxy");
    proxy.write_all(&[DOMAIN_TYPE]).await?;
    proxy.write_all(&[request.target.len() as u8]).await?;
    proxy.write_all(request.target.as_bytes()).await?;

    proxy.write_all(&request.port.to_be_bytes()).await?;
    debug!("Forwarded request to proxy");

    debug!("Waiting for proxy response with timeout");
    let mut response_header = [0u8; 4];
    match timeout(
        Duration::from_secs(10),
        proxy.read_exact(&mut response_header),
    )
    .await
    {
        Ok(Ok(_)) => debug!("Received proxy response header: {:?}", response_header),
        Ok(Err(e)) => {
            error!("Failed to read proxy response header: {}", e);
            return Err(e);
        }
        Err(_) => {
            error!("Timed out while waiting for proxy response");
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Timed out while waiting for proxy response",
            ));
        }
    }

    if response_header[1] != SUCCESS_REPLY {
        error!(
            "Proxy connection failed with status: {}",
            response_header[1]
        );
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "Proxy connection failed with status: {}",
                response_header[1]
            ),
        ));
    }

    debug!("Processing proxy response address type");
    match response_header[3] {
        IPV4_TYPE => {
            let mut addr = [0u8; 6];
            match timeout(Duration::from_secs(10), proxy.read_exact(&mut addr)).await {
                Ok(Ok(_)) => debug!("Proxy bound IPv4 address: {:?}", addr),
                Ok(Err(e)) => {
                    error!("Failed to read proxy bound IPv4 address: {}", e);
                    return Err(e);
                }
                Err(_) => {
                    error!("Timed out while reading proxy bound IPv4 address");
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "Timed out while reading proxy bound IPv4 address",
                    ));
                }
            }
        }
        IPV6_TYPE => {
            let mut addr = [0u8; 18];
            match timeout(Duration::from_secs(10), proxy.read_exact(&mut addr)).await {
                Ok(Ok(_)) => debug!("Proxy bound IPv6 address: {:?}", addr),
                Ok(Err(e)) => {
                    error!("Failed to read proxy bound IPv6 address: {}", e);
                    return Err(e);
                }
                Err(_) => {
                    error!("Timed out while reading proxy bound IPv6 address");
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "Timed out while reading proxy bound IPv6 address",
                    ));
                }
            }
        }
        DOMAIN_TYPE => {
            let mut len = [0u8; 1];
            match timeout(Duration::from_secs(10), proxy.read_exact(&mut len)).await {
                Ok(Ok(_)) => {
                    let mut domain = vec![0u8; len[0] as usize + 2];
                    match timeout(Duration::from_secs(10), proxy.read_exact(&mut domain)).await {
                        Ok(Ok(_)) => debug!("Proxy bound domain address: {:?}", domain),
                        Ok(Err(e)) => {
                            error!("Failed to read proxy bound domain address: {}", e);
                            return Err(e);
                        }
                        Err(_) => {
                            error!("Timed out while reading proxy bound domain address");
                            return Err(io::Error::new(
                                io::ErrorKind::TimedOut,
                                "Timed out while reading proxy bound domain address",
                            ));
                        }
                    }
                }
                Ok(Err(e)) => {
                    error!("Failed to read domain length: {}", e);
                    return Err(e);
                }
                Err(_) => {
                    error!("Timed out while reading domain length");
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "Timed out while reading domain length",
                    ));
                }
            }
        }
        _ => {
            error!(
                "Invalid address type in proxy response: {}",
                response_header[3]
            );
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid address type in proxy response",
            ));
        }
    }

    Ok(proxy)
}
