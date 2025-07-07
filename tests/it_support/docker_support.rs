use std::time::Duration;
use testcontainers::{
    GenericImage,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use tokio::time::sleep;

pub struct RunningContainer {
    container: testcontainers::ContainerAsync<GenericImage>,
}

impl RunningContainer {
    #[allow(dead_code)]
    pub async fn get_host_port(
        &self,
        internal_port: u16,
    ) -> Result<u16, Box<dyn std::error::Error>> {
        Ok(self.container.get_host_port_ipv4(internal_port).await?)
    }

    #[allow(dead_code)]
    pub async fn get_host(&self) -> Result<String, Box<dyn std::error::Error>> {
        Ok(self.container.get_host().await?.to_string())
    }

    #[allow(dead_code)]
    pub fn id(&self) -> String {
        self.container.id().to_string()
    }
}

/// Start a Docker container with the given image
#[allow(dead_code)]
pub async fn start_container(
    image: GenericImage,
) -> Result<RunningContainer, Box<dyn std::error::Error>> {
    let container = image.start().await?;
    Ok(RunningContainer { container })
}

/// Wait for a port to be available
#[allow(dead_code)]
pub async fn wait_for_port(
    host: &str,
    port: u16,
    timeout_duration: Duration,
) -> Result<(), Box<dyn std::error::Error>> {
    let start = std::time::Instant::now();

    while start.elapsed() < timeout_duration {
        if (tokio::net::TcpStream::connect(format!("{host}:{port}")).await).is_ok() {
            return Ok(());
        }
        sleep(Duration::from_millis(100)).await;
    }

    Err(format!("Port {host}:{port} not available within timeout").into())
}

/// Create a tinyproxy image configuration
#[allow(dead_code)]
pub fn tinyproxy_image() -> GenericImage {
    GenericImage::new("tinyproxy/tinyproxy", "latest")
        .with_exposed_port(8888.tcp())
        .with_wait_for(WaitFor::message_on_stdout("Starting tinyproxy"))
}

/// Create a simple SOCKS5 proxy image configuration
#[allow(dead_code)]
pub fn simple_socks5_image() -> GenericImage {
    GenericImage::new("serjs/go-socks5-proxy", "latest")
        .with_exposed_port(1080.tcp())
        // Wait for the SOCKS5 service to start by looking for startup message in stderr
        .with_wait_for(WaitFor::message_on_stderr("Start listening proxy service"))
}

/// Create a simple HTTP proxy container image
/// Uses tinyproxy which is a lightweight HTTP proxy
#[allow(dead_code)]
pub fn simple_http_proxy_image() -> GenericImage {
    GenericImage::new("vimagick/tinyproxy", "latest")
        .with_exposed_port(8888.tcp())
        // Wait for the service to start
        .with_wait_for(WaitFor::message_on_stdout(
            "Starting main loop. Accepting connections.",
        ))
}

// Legacy compatibility - keep these for existing tests
#[allow(dead_code)]
pub async fn wait_for_port_legacy(host: &str, port: u16, timeout: Duration) -> Result<(), String> {
    wait_for_port(host, port, timeout)
        .await
        .map_err(|e| e.to_string())
}

/// Get the host address that Docker containers can use to reach the host machine
/// Returns the actual IP address rather than hostname to avoid DNS resolution issues
#[allow(dead_code)]
pub fn get_docker_host_address() -> String {
    // On Docker Desktop (Mac/Windows), get the actual IP that host.docker.internal resolves to
    if cfg!(target_os = "macos") || cfg!(target_os = "windows") {
        get_docker_desktop_host_ip().unwrap_or_else(|| "192.168.65.254".to_string())
    } else {
        // On Linux, try to get the Docker bridge IP
        // Fallback to the standard bridge IP
        get_docker_bridge_ip().unwrap_or_else(|_| "172.17.0.1".to_string())
    }
}

/// Get the Docker Desktop host IP by checking Docker's internal network
#[allow(dead_code)]
fn get_docker_desktop_host_ip() -> Option<String> {
    use std::process::Command;

    // Try to get the host IP from Docker Desktop
    let output = Command::new("docker")
        .args([
            "run",
            "--rm",
            "alpine",
            "getent",
            "hosts",
            "host.docker.internal",
        ])
        .output()
        .ok()?;

    if output.status.success() {
        let output_str = String::from_utf8(output.stdout).ok()?;
        // Parse the output to get the IP (format: "IP hostname")
        let parts: Vec<&str> = output_str.split_whitespace().collect();
        if !parts.is_empty() {
            return Some(parts[0].to_string());
        }
    }

    None
}

/// Attempt to get the Docker bridge IP on Linux
/// This is a best-effort approach and may not work in all environments
#[allow(dead_code)]
fn get_docker_bridge_ip() -> Result<String, Box<dyn std::error::Error>> {
    use std::process::Command;

    let output = Command::new("docker")
        .args(["network", "inspect", "bridge"])
        .output()?;

    if !output.status.success() {
        return Err("Failed to inspect Docker bridge network".into());
    }

    let output_str = String::from_utf8(output.stdout)?;

    // Parse JSON to find the gateway IP
    let json: serde_json::Value = serde_json::from_str(&output_str)?;
    if let Some(gateway) = json[0]["IPAM"]["Config"][0]["Gateway"].as_str() {
        Ok(gateway.to_string())
    } else {
        Err("Could not find Docker bridge gateway IP".into())
    }
}

/// Convert a localhost URL to a Docker-accessible URL using the container's host address
/// This replaces 127.0.0.1 and localhost with the appropriate Docker host address
#[allow(dead_code)]
pub fn convert_localhost_url_for_docker_with_host(url: &str, host: &str) -> String {
    url.replace("127.0.0.1", host).replace("localhost", host)
}

/// Convert a localhost URL to a Docker-accessible URL
/// This replaces 127.0.0.1 and localhost with the appropriate Docker host address
#[allow(dead_code)]
pub fn convert_localhost_url_for_docker(url: &str) -> String {
    let host_address = get_docker_host_address();
    url.replace("127.0.0.1", &host_address)
        .replace("localhost", &host_address)
}
