#![allow(dead_code)]
#![allow(unused_imports)]

use std::time::Duration;
use tokio::time::timeout;

use crate::it_support::{
    HttpEchoServer, HttpsEchoServer, TestServer, docker_support,
    proxy_twister_helper::{ProxyTwisterInstance, create_direct_client, create_test_client},
};

/// Standard test timeout duration
#[allow(dead_code)]
pub const STANDARD_TIMEOUT: Duration = Duration::from_secs(15);

/// TestEnvironment provides a standard way to set up and tear down test resources.
/// It follows the builder pattern for a fluent API, allowing tests to easily
/// configure their environment requirements.
///
/// # Example
/// ```
/// let env = TestEnvironment::new()
///     .with_http_server().await?
///     .with_direct_proxy().await?;
///     
/// let client = env.create_proxy_client()?;
/// let response = test_http_get(&client, &format!("{}/get", env.http_url())).await?;
/// ```
#[allow(dead_code)]
pub struct TestEnvironment {
    pub http_server: Option<HttpEchoServer>,
    #[allow(dead_code)]
    pub https_server: Option<HttpsEchoServer>,
    pub proxy_instance: Option<ProxyTwisterInstance>,
}

impl TestEnvironment {
    /// Create a new empty test environment
    pub fn new() -> Self {
        TestEnvironment {
            http_server: None,
            https_server: None,
            proxy_instance: None,
        }
    }

    /// Set up an HTTP echo server
    pub async fn with_http_server(mut self) -> Result<Self, Box<dyn std::error::Error>> {
        self.http_server = Some(HttpEchoServer::start().await?);
        Ok(self)
    }

    /// Set up an HTTPS echo server
    pub async fn with_https_server(mut self) -> Result<Self, Box<dyn std::error::Error>> {
        self.https_server = Some(HttpsEchoServer::start().await?);
        Ok(self)
    }

    /// Set up a proxy-twister instance with the given configuration
    pub async fn with_proxy(mut self, config: &str) -> Result<Self, Box<dyn std::error::Error>> {
        self.proxy_instance = Some(ProxyTwisterInstance::start(config, None).await?);
        Ok(self)
    }

    /// Set up a proxy-twister instance with direct routing
    pub async fn with_direct_proxy(mut self) -> Result<Self, Box<dyn std::error::Error>> {
        let config = crate::it_support::create_test_config_content(
            &[("direct", r#"{"scheme": "direct"}"#)],
            &[("*", "direct")],
        );

        self.proxy_instance = Some(ProxyTwisterInstance::start(&config, None).await?);
        Ok(self)
    }

    /// Create a client that uses the proxy
    pub fn create_proxy_client(&self) -> Result<reqwest::Client, reqwest::Error> {
        match &self.proxy_instance {
            Some(proxy) => create_test_client(&proxy.proxy_url()),
            None => {
                // Create a simple client with reasonable defaults
                // but add a warning message
                eprintln!("Warning: No proxy instance available, creating direct client");
                create_direct_client()
            }
        }
    }

    /// Create a direct client that doesn't use a proxy
    pub fn create_direct_client(&self) -> Result<reqwest::Client, reqwest::Error> {
        create_direct_client()
    }

    /// Get the HTTP server URL, or panic if not available
    pub fn http_url(&self) -> String {
        self.http_server
            .as_ref()
            .expect("HTTP server not initialized")
            .url()
    }

    /// Get the HTTP server URL accessible from Docker containers
    pub fn http_docker_url(&self) -> String {
        self.http_server
            .as_ref()
            .expect("HTTP server not initialized")
            .docker_url()
    }

    /// Get the HTTPS server URL, or panic if not available
    pub fn https_url(&self) -> String {
        self.https_server
            .as_ref()
            .expect("HTTPS server not initialized")
            .url()
    }

    /// Tear down all resources
    pub async fn teardown(self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(proxy) = self.proxy_instance {
            proxy.stop().await?;
        }

        // Server containers are automatically cleaned up when dropped
        Ok(())
    }
}

/// Run a test with a standard HTTP environment (HTTP server + direct proxy)
pub async fn with_http_test_environment<F, Fut, R>(
    test_fn: F,
) -> Result<R, Box<dyn std::error::Error>>
where
    F: FnOnce(TestEnvironment) -> Fut,
    Fut: std::future::Future<Output = Result<R, Box<dyn std::error::Error>>>,
{
    let env = TestEnvironment::new()
        .with_http_server()
        .await?
        .with_direct_proxy()
        .await?;

    let result = test_fn(env).await?;
    Ok(result)
}

/// Helper for testing a basic HTTP GET request through proxy-twister
pub async fn test_http_get(
    client: &reqwest::Client,
    url: &str,
) -> Result<reqwest::Response, Box<dyn std::error::Error>> {
    Ok(timeout(STANDARD_TIMEOUT, client.get(url).send()).await??)
}

/// Helper for testing a basic HTTP POST request through proxy-twister
pub async fn test_http_post<T: serde::Serialize + ?Sized>(
    client: &reqwest::Client,
    url: &str,
    json: &T,
) -> Result<reqwest::Response, Box<dyn std::error::Error>> {
    Ok(timeout(STANDARD_TIMEOUT, client.post(url).json(json).send()).await??)
}

/// Run a test with an HTTP server and HTTP proxy
pub async fn with_http_proxy_test_environment<F, Fut, R>(
    proxy_port: u16,
    test_fn: F,
) -> Result<R, Box<dyn std::error::Error>>
where
    F: FnOnce(TestEnvironment) -> Fut,
    Fut: std::future::Future<Output = Result<R, Box<dyn std::error::Error>>>,
{
    // Start a containerized HTTP proxy server
    let http_proxy_image = docker_support::simple_http_proxy_image();
    let http_proxy_container = docker_support::start_container(http_proxy_image).await?;
    let http_proxy_port = http_proxy_container.get_host_port(proxy_port).await?;

    // Wait for proxy to be ready
    docker_support::wait_for_port("127.0.0.1", http_proxy_port, Duration::from_secs(30)).await?;

    // Set up the environment with an HTTP server
    let env = TestEnvironment::new().with_http_server().await?;

    // Create config that routes all traffic through containerized HTTP proxy
    let config = crate::it_support::create_test_config_content(
        &[(
            "http_proxy",
            &format!(r#"{{"scheme": "http", "host": "127.0.0.1", "port": {http_proxy_port}}}"#),
        )],
        &[("*", "http_proxy")],
    );

    let env = env.with_proxy(&config).await?;

    // Run the test function
    let result = test_fn(env).await?;

    // The HTTP proxy container will be cleaned up when it goes out of scope
    Ok(result)
}

/// Run a test with an HTTP server and SOCKS5 proxy
pub async fn with_socks5_proxy_test_environment<F, Fut, R>(
    test_fn: F,
) -> Result<R, Box<dyn std::error::Error>>
where
    F: FnOnce(TestEnvironment) -> Fut,
    Fut: std::future::Future<Output = Result<R, Box<dyn std::error::Error>>>,
{
    // Start containerized SOCKS5 proxy server
    let socks5_image = docker_support::simple_socks5_image();
    let socks5_container = docker_support::start_container(socks5_image).await?;
    let socks5_port = socks5_container.get_host_port(1080).await?;

    // Wait for proxy to be ready
    docker_support::wait_for_port("127.0.0.1", socks5_port, Duration::from_secs(30)).await?;

    // Set up the environment with an HTTP server
    let env = TestEnvironment::new().with_http_server().await?;

    // Create config that routes all traffic through containerized SOCKS5 proxy
    let config = crate::it_support::create_test_config_content(
        &[(
            "socks5_proxy",
            &format!(r#"{{"scheme": "socks5", "host": "127.0.0.1", "port": {socks5_port}}}"#),
        )],
        &[("*", "socks5_proxy")],
    );

    let env = env.with_proxy(&config).await?;

    // Run the test function
    let result = test_fn(env).await?;

    // The SOCKS5 proxy container will be cleaned up when it goes out of scope
    Ok(result)
}

/// Run a test with a standard HTTPS environment (HTTPS server + direct proxy)
pub async fn with_https_test_environment<F, Fut, R>(
    test_fn: F,
) -> Result<R, Box<dyn std::error::Error>>
where
    F: FnOnce(TestEnvironment) -> Fut,
    Fut: std::future::Future<Output = Result<R, Box<dyn std::error::Error>>>,
{
    let env = TestEnvironment::new()
        .with_https_server()
        .await?
        .with_direct_proxy()
        .await?;

    let result = test_fn(env).await?;
    Ok(result)
}

/// Helper for testing a basic HTTPS GET request through proxy-twister
/// Uses a client that accepts self-signed certificates
pub async fn test_https_get(
    client: &reqwest::Client,
    url: &str,
) -> Result<reqwest::Response, Box<dyn std::error::Error>> {
    Ok(timeout(STANDARD_TIMEOUT, client.get(url).send()).await??)
}

/// Helper for testing a basic HTTPS POST request through proxy-twister
/// Uses a client that accepts self-signed certificates
pub async fn test_https_post<T: serde::Serialize + ?Sized>(
    client: &reqwest::Client,
    url: &str,
    json: &T,
) -> Result<reqwest::Response, Box<dyn std::error::Error>> {
    Ok(timeout(STANDARD_TIMEOUT, client.post(url).json(json).send()).await??)
}

/// Run a test with an HTTPS server and HTTP proxy
pub async fn with_https_http_proxy_test_environment<F, Fut, R>(
    proxy_port: u16,
    test_fn: F,
) -> Result<R, Box<dyn std::error::Error>>
where
    F: FnOnce(TestEnvironment) -> Fut,
    Fut: std::future::Future<Output = Result<R, Box<dyn std::error::Error>>>,
{
    // Start a containerized HTTP proxy server
    let http_proxy_image = docker_support::simple_http_proxy_image();
    let http_proxy_container = docker_support::start_container(http_proxy_image).await?;
    let http_proxy_port = http_proxy_container.get_host_port(proxy_port).await?;

    // Wait for proxy to be ready
    docker_support::wait_for_port("127.0.0.1", http_proxy_port, Duration::from_secs(30)).await?;

    // Set up the environment with an HTTPS server
    let env = TestEnvironment::new().with_https_server().await?;

    // Create config that routes all traffic through containerized HTTP proxy
    let config = crate::it_support::create_test_config_content(
        &[(
            "http_proxy",
            &format!(r#"{{"scheme": "http", "host": "127.0.0.1", "port": {http_proxy_port}}}"#),
        )],
        &[("*", "http_proxy")],
    );

    let env = env.with_proxy(&config).await?;

    // Run the test function
    let result = test_fn(env).await?;

    // The HTTP proxy container will be cleaned up when it goes out of scope
    Ok(result)
}

/// Run a test with an HTTPS server and SOCKS5 proxy
pub async fn with_https_socks5_proxy_test_environment<F, Fut, R>(
    test_fn: F,
) -> Result<R, Box<dyn std::error::Error>>
where
    F: FnOnce(TestEnvironment) -> Fut,
    Fut: std::future::Future<Output = Result<R, Box<dyn std::error::Error>>>,
{
    // Start containerized SOCKS5 proxy server
    let socks5_image = docker_support::simple_socks5_image();
    let socks5_container = docker_support::start_container(socks5_image).await?;
    let socks5_port = socks5_container.get_host_port(1080).await?;

    // Wait for proxy to be ready
    docker_support::wait_for_port("127.0.0.1", socks5_port, Duration::from_secs(30)).await?;

    // Set up the environment with an HTTPS server
    let env = TestEnvironment::new().with_https_server().await?;

    // Create config that routes all traffic through containerized SOCKS5 proxy
    let config = crate::it_support::create_test_config_content(
        &[(
            "socks5_proxy",
            &format!(r#"{{"scheme": "socks5", "host": "127.0.0.1", "port": {socks5_port}}}"#),
        )],
        &[("*", "socks5_proxy")],
    );

    let env = env.with_proxy(&config).await?;

    // Run the test function
    let result = test_fn(env).await?;

    // The SOCKS5 proxy container will be cleaned up when it goes out of scope
    Ok(result)
}
