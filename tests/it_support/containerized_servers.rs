use crate::it_support::docker_support::{
    self, RunningContainer, get_docker_host_address, get_host_accessible_address,
};
use std::time::Duration;
use testcontainers::{
    GenericImage,
    core::{IntoContainerPort, WaitFor},
};

/// Common trait for test server implementations providing a unified interface
pub trait TestServer {
    /// Get URL for accessing the server in tests
    fn url(&self) -> String;
}

/// A containerized HTTP echo server based on httpbin image
pub struct HttpEchoServer {
    #[allow(dead_code)]
    container: RunningContainer,
    pub port: u16,
}

impl HttpEchoServer {
    /// Start a new HTTP echo server instance
    #[allow(dead_code)]
    pub async fn start() -> Result<Self, Box<dyn std::error::Error>> {
        // Create httpbin image
        let image = GenericImage::new("kennethreitz/httpbin", "latest")
            .with_exposed_port(80.tcp())
            .with_wait_for(WaitFor::message_on_stderr(
                "Listening at: http://0.0.0.0:80",
            ));

        // Start the container
        let container = docker_support::start_container(image).await?;

        // Get the host port
        let port = container.get_host_port(80).await?;

        // Wait for the server to be ready
        docker_support::wait_for_port("127.0.0.1", port, Duration::from_secs(30)).await?;

        // Add a small delay to ensure the server is fully initialized
        tokio::time::sleep(Duration::from_millis(500)).await;

        Ok(HttpEchoServer { container, port })
    }
}

impl TestServer for HttpEchoServer {
    fn url(&self) -> String {
        // For tests running on the host machine, use localhost address for port mapping
        let host_address = get_host_accessible_address();
        format!(
            "http://{host_address}:{host_port}",
            host_address = host_address,
            host_port = self.port
        )
    }
}

impl HttpEchoServer {
    /// Get URL that's accessible from other Docker containers
    /// This is needed when HTTP proxy containers need to reach the httpbin server
    pub fn docker_url(&self) -> String {
        let host_address = get_docker_host_address();
        format!(
            "http://{host_address}:{host_port}",
            host_address = host_address,
            host_port = self.port
        )
    }
}

/// A containerized HTTPS echo server that provides test endpoints over TLS
///
/// This implementation uses the mendhak/http-https-echo image which provides
/// both HTTP and HTTPS endpoints out of the box with built-in self-signed certificates.
/// The server echoes request properties back in the response, making it perfect for testing.
pub struct HttpsEchoServer {
    #[allow(dead_code)]
    container: RunningContainer,
    pub port: u16,
}

impl HttpsEchoServer {
    /// Start a new HTTPS echo server instance with real SSL/TLS support
    pub async fn start() -> Result<Self, Box<dyn std::error::Error>> {
        // Create http-https-echo image with HTTPS configuration
        let image = GenericImage::new("mendhak/http-https-echo", "31")
            .with_exposed_port(8443.tcp())
            .with_wait_for(WaitFor::message_on_stdout("Listening on ports"));

        // Start the container
        let container = docker_support::start_container(image).await?;

        // Get the host port for HTTPS (8443 is the default HTTPS port in the image)
        let port = container.get_host_port(8443).await?;

        // Wait for the HTTPS server to be ready
        docker_support::wait_for_port("127.0.0.1", port, Duration::from_secs(30)).await?;

        // Add a small delay to ensure the server is fully initialized
        tokio::time::sleep(Duration::from_millis(500)).await;

        Ok(HttpsEchoServer { container, port })
    }
}

impl TestServer for HttpsEchoServer {
    fn url(&self) -> String {
        let host_address = get_host_accessible_address();
        format!(
            "https://{host_address}:{port}",
            host_address = host_address,
            port = self.port
        )
    }
}
