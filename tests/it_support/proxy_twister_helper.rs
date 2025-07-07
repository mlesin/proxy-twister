use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};
use tokio::time::sleep;

/// Helper to manage a proxy-twister instance for testing
pub struct ProxyTwisterInstance {
    pub process: Child,
    pub port: u16,
    pub config_file: PathBuf,
}

impl ProxyTwisterInstance {
    #[allow(dead_code)]
    pub async fn start(
        config_content: &str,
        listen_port: Option<u16>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // Create temporary config file
        let config_file = crate::it_support::create_temp_config_file(config_content).await?;

        let port = listen_port.unwrap_or(0);
        let listen_port = if port == 0 {
            // Find available port
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
            let actual_port = listener.local_addr()?.port();
            drop(listener);
            actual_port
        } else {
            port
        };

        let listen_address = format!("127.0.0.1:{listen_port}");

        // Start proxy-twister process
        let mut process = Command::new("cargo")
            .arg("run")
            .arg("--")
            .arg("--config")
            .arg(&config_file)
            .arg("--listen")
            .arg(&listen_address)
            .env("RUST_LOG", "debug")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        // Wait for the process to start
        sleep(Duration::from_millis(500)).await;

        // Check if process is still running
        if let Ok(Some(exit_status)) = process.try_wait() {
            // Process exited, try to read stderr
            let stderr_output = if let Some(mut stderr) = process.stderr.take() {
                let mut output = String::new();
                let _ = stderr.read_to_string(&mut output).await;
                output
            } else {
                "No stderr output".to_string()
            };
            return Err(format!(
                "Proxy-twister process exited with: {exit_status}. Stderr: {stderr_output}"
            )
            .into());
        }

        // Wait for the port to be available
        crate::it_support::wait_for_port("127.0.0.1", listen_port, Duration::from_secs(10)).await?;

        Ok(ProxyTwisterInstance {
            process,
            port: listen_port,
            config_file,
        })
    }

    #[allow(dead_code)]
    pub fn proxy_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    #[allow(dead_code)]
    pub fn socks5_proxy_url(&self) -> String {
        format!("socks5://127.0.0.1:{}", self.port)
    }

    #[allow(dead_code)]
    pub async fn stop(mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.process.kill().await?;
        self.process.wait().await?;

        // Clean up config file
        if self.config_file.exists() {
            std::fs::remove_file(&self.config_file)?;
        }

        Ok(())
    }
}

impl Drop for ProxyTwisterInstance {
    fn drop(&mut self) {
        // Try to kill the process
        let _ = self.process.start_kill();

        // Clean up config file
        if self.config_file.exists() {
            let _ = std::fs::remove_file(&self.config_file);
        }
    }
}

/// Create a test HTTP client configured to use the proxy-twister instance
#[allow(dead_code)]
pub fn create_test_client(proxy_url: &str) -> Result<reqwest::Client, reqwest::Error> {
    let proxy = reqwest::Proxy::http(proxy_url)?;

    reqwest::Client::builder()
        .proxy(proxy)
        .timeout(Duration::from_secs(10))
        .danger_accept_invalid_certs(true)
        .build()
}

/// Create a test HTTP client configured to use SOCKS5 proxy
#[allow(dead_code)]
pub fn create_socks5_test_client(proxy_url: &str) -> Result<reqwest::Client, reqwest::Error> {
    let proxy = reqwest::Proxy::all(proxy_url)?;

    reqwest::Client::builder()
        .proxy(proxy)
        .timeout(Duration::from_secs(10))
        .danger_accept_invalid_certs(true)
        .build()
}

/// Create a test HTTP client with custom root certificate
#[allow(dead_code)]
pub fn create_test_client_with_cert(
    proxy_url: &str,
    cert_pem: &str,
) -> Result<reqwest::Client, reqwest::Error> {
    let cert = reqwest::Certificate::from_pem(cert_pem.as_bytes())?;
    let proxy = reqwest::Proxy::http(proxy_url)?;

    reqwest::Client::builder()
        .proxy(proxy)
        .add_root_certificate(cert)
        .timeout(Duration::from_secs(10))
        .build()
}

/// Create a direct HTTP client (no proxy)
#[allow(dead_code)]
pub fn create_direct_client() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .danger_accept_invalid_certs(true)
        .build()
}

/// Create a direct HTTP client with custom root certificate
#[allow(dead_code)]
pub fn create_direct_client_with_cert(cert_pem: &str) -> Result<reqwest::Client, reqwest::Error> {
    let cert = reqwest::Certificate::from_pem(cert_pem.as_bytes())?;

    reqwest::Client::builder()
        .add_root_certificate(cert)
        .timeout(Duration::from_secs(10))
        .build()
}
