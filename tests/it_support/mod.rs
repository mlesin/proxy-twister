pub mod containerized_servers;
pub mod docker_support;
pub mod proxy_twister_helper;
pub mod test_helpers;

#[allow(unused_imports)]
pub use containerized_servers::*;
#[allow(unused_imports)]
pub use proxy_twister_helper::*;
#[allow(unused_imports)]
pub use test_helpers::*;

use std::time::Duration;
use testcontainers::{
    GenericImage,
    core::{IntoContainerPort, WaitFor},
};

/// Common test timeout
pub const TEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Get a containerized httpbin server for tests
/// This is useful for environments without direct internet access
#[allow(dead_code)]
pub async fn get_httpbin_container() -> Result<(String, u16), Box<dyn std::error::Error>> {
    // Start a httpbin container
    let httpbin_image = GenericImage::new("kennethreitz/httpbin", "latest")
        .with_exposed_port(80.tcp())
        .with_wait_for(WaitFor::message_on_stderr(
            "Listening at: http://0.0.0.0:80",
        ));

    let httpbin_container = docker_support::start_container(httpbin_image).await?;
    let httpbin_port = httpbin_container.get_host_port(80).await?;

    // Wait for httpbin to be ready
    wait_for_port("127.0.0.1", httpbin_port, TEST_TIMEOUT).await?;

    // Get the Docker host address for container-to-container communication
    let docker_host = docker_support::get_docker_host_address();

    // Add a small delay to ensure the service is fully initialized
    tokio::time::sleep(Duration::from_millis(500)).await;

    Ok((docker_host, httpbin_port))
}

/// Helper function to wait for service to be ready
#[allow(dead_code)]
pub async fn wait_for_service_ready(port: u16) -> Result<(), String> {
    wait_for_port("127.0.0.1", port, TEST_TIMEOUT)
        .await
        .map_err(|e| e.to_string())
}

/// Helper function to wait for a port to be available
pub async fn wait_for_port(
    host: &str,
    port: u16,
    timeout: Duration,
) -> Result<(), Box<dyn std::error::Error>> {
    let start = std::time::Instant::now();

    while start.elapsed() < timeout {
        if tokio::net::TcpStream::connect((host, port)).await.is_ok() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    Err(format!("Port {host}:{port} not available after {timeout:?}").into())
}

/// Helper function to create a temporary config file for testing
#[allow(dead_code)]
pub fn create_test_config_content(profiles: &[(&str, &str)], rules: &[(&str, &str)]) -> String {
    let mut config = serde_json::json!({
        "switch": {
            "default": "direct",
            "rules": []
        },
        "profiles": {
            "direct": {
                "scheme": "direct"
            }
        }
    });

    // Add profiles
    for (name, profile_json) in profiles {
        config["profiles"][name] = serde_json::from_str(profile_json).unwrap();
    }

    // Add rules
    for (pattern, profile) in rules {
        config["switch"]["rules"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "pattern": pattern,
                "profile": profile
            }));
    }

    serde_json::to_string_pretty(&config).unwrap()
}

/// Helper function to create a temporary config file
pub async fn create_temp_config_file(content: &str) -> Result<std::path::PathBuf, std::io::Error> {
    use std::io::Write;

    let temp_dir = std::env::temp_dir();
    let config_file = temp_dir.join(format!("proxy-twister-test-{}.json", uuid::Uuid::new_v4()));

    let mut file = std::fs::File::create(&config_file)?;
    file.write_all(content.as_bytes())?;

    Ok(config_file)
}
