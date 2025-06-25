use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use std::collections::HashMap;
use std::io;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::time::{Duration, timeout};
use tracing::{error, trace};

pub const HTTP_SERVER_ERROR: &str = "500 Internal Server Error\r\n\r\n";

#[derive(Clone)]
pub struct HttpRequest {
    pub method: String,
    pub target: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>, // Add body field for POST/PUT requests
}

pub async fn parse_request(stream: &mut TcpStream) -> io::Result<HttpRequest> {
    let mut reader = BufReader::new(stream);
    let mut first_line = String::new();
    
    // Add timeout for reading the first line to prevent hanging
    match timeout(Duration::from_secs(30), reader.read_line(&mut first_line)).await {
        Ok(Ok(_)) => {},
        Ok(Err(e)) => return Err(e),
        Err(_) => return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "Timeout reading HTTP request line"
        )),
    }

    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() != 3 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid HTTP request",
        ));
    }

    let method = parts[0].to_string();
    let target = parts[1].to_string();
    let mut headers = HashMap::new();
    let mut content_length = 0;

    // Read headers with timeout
    loop {
        let mut line = String::new();
        match timeout(Duration::from_secs(30), reader.read_line(&mut line)).await {
            Ok(Ok(_)) => {},
            Ok(Err(e)) => return Err(e),
            Err(_) => return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Timeout reading HTTP headers"
            )),
        }

        if line.trim().is_empty() {
            break;
        }

        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim().to_lowercase();
            let value = value.trim().to_string();

            if key == "content-length" {
                content_length = value.parse().unwrap_or_else(|_| {
                    trace!("Invalid content-length value: {}", value);
                    0
                });
            }

            headers.insert(key, value);
        }
    }

    // Read body if present with timeout
    let mut body = Vec::new();
    if content_length > 0 {
        let mut buffer = vec![0u8; content_length];
        match timeout(Duration::from_secs(30), reader.read_exact(&mut buffer)).await {
            Ok(Ok(_)) => body = buffer,
            Ok(Err(e)) => return Err(e),
            Err(_) => return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Timeout reading HTTP body"
            )),
        }
    }

    Ok(HttpRequest {
        method,
        target,
        headers,
        body,
    })
}

pub async fn handle_connect(
    stream: &mut TcpStream,
    request: HttpRequest,
) -> io::Result<(String, u16)> {
    if request.method != "CONNECT" {
        stream
            .write_all(b"HTTP/1.1 405 Method Not Allowed\r\n\r\n")
            .await?;
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Only CONNECT method is supported",
        ));
    }

    let target_parts: Vec<&str> = request.target.split(':').collect();
    if target_parts.len() != 2 {
        stream
            .write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n")
            .await?;
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid target format",
        ));
    }

    let host = target_parts[0].to_string();
    let port = target_parts[1]
        .parse::<u16>()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid port number"))?;

    stream
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .await?;

    Ok((host, port))
}

pub async fn forward_to_proxy(
    target_host: &str,
    target_port: u16,
    proxy_host: &str,
    proxy_port: u16,
    auth: Option<(&str, &str)>,
) -> io::Result<TcpStream> {
    let mut stream = TcpStream::connect(format!("{}:{}", proxy_host, proxy_port)).await?;

    let mut request = format!(
        "CONNECT {}:{} HTTP/1.1\r\n\
         Host: {}:{}\r\n",
        target_host, target_port, target_host, target_port
    );

    // Add Proxy-Authorization if credentials are provided
    if let Some((username, password)) = auth {
        let auth_string = format!("{}:{}", username, password);
        let encoded = BASE64.encode(auth_string);
        request.push_str(&format!("Proxy-Authorization: Basic {}\r\n", encoded));
    }

    request.push_str("\r\n");

    trace!("Sending request to proxy: {}", request);
    stream.write_all(request.as_bytes()).await?;

    trace!("Waiting for proxy response with timeout");
    let mut reader = BufReader::new(&mut stream);
    let mut response = String::new();
    match timeout(Duration::from_secs(10), reader.read_line(&mut response)).await {
        Ok(Ok(_)) => trace!("Received proxy response: {}", response.trim()),
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

    if !response.starts_with("HTTP/1.1 200") && !response.starts_with("HTTP/1.0 200") {
        error!("Proxy connection failed: {}", response.trim());
        return Err(io::Error::other(format!(
            "Proxy connection failed: {}",
            response.trim()
        )));
    }

    trace!("Reading and discarding proxy response headers");
    loop {
        let mut line = String::new();
        match timeout(Duration::from_secs(10), reader.read_line(&mut line)).await {
            Ok(Ok(_)) => {
                if line.trim().is_empty() {
                    break;
                }
                trace!("Proxy response header: {}", line.trim());
            }
            Ok(Err(e)) => {
                error!("Failed to read proxy response headers: {}", e);
                return Err(e);
            }
            Err(_) => {
                error!("Timed out while reading proxy response headers");
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Timed out while reading proxy response headers",
                ));
            }
        }
    }

    Ok(stream)
}

pub async fn forward_http_request(
    request: &HttpRequest,
    target_host: &str,
    target_port: u16,
    proxy_host: &str,
    proxy_port: u16,
    auth: Option<(&str, &str)>,
) -> io::Result<TcpStream> {
    let mut stream = TcpStream::connect(format!("{}:{}", proxy_host, proxy_port)).await?;

    // For HTTP proxy, modify the request
    let mut modified_request = format!("{} {} HTTP/1.1\r\n", request.method, request.target);

    // Copy original headers
    for (key, value) in &request.headers {
        if key != "proxy-connection" {
            modified_request.push_str(&format!("{}: {}\r\n", key, value));
        }
    }

    // Add proxy auth if provided
    if let Some((username, password)) = auth {
        let auth_string = format!("{}:{}", username, password);
        let encoded = BASE64.encode(auth_string);
        modified_request.push_str(&format!("Proxy-Authorization: Basic {}\r\n", encoded));
    }

    // Ensure host header is present
    if !request.headers.contains_key("host") {
        modified_request.push_str(&format!("Host: {}:{}\r\n", target_host, target_port));
    }

    // Add content length if body present
    if !request.body.is_empty() {
        modified_request.push_str(&format!("Content-Length: {}\r\n", request.body.len()));
    }

    modified_request.push_str("\r\n");

    // Write headers
    stream.write_all(modified_request.as_bytes()).await?;

    // Write body if present
    if !request.body.is_empty() {
        stream.write_all(&request.body).await?;
    }

    Ok(stream)
}
