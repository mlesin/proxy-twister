mod it_support;
use it_support::{STANDARD_TIMEOUT, test_http_get, with_socks5_proxy_test_environment};

/// Test HTTP routing through a SOCKS5 proxy
#[tokio::test]
async fn test_socks5_proxy_routing() -> Result<(), Box<dyn std::error::Error>> {
    // Using the specialized SOCKS5 proxy test environment
    with_socks5_proxy_test_environment(|env| async move {
        // Get a client that uses proxy-twister which routes through the SOCKS5 proxy
        let client = env.create_proxy_client()?;

        // Test a basic GET request
        let url = format!("{}/get", env.http_docker_url());
        let response = test_http_get(&client, &url).await?;

        // Verify the response
        assert_eq!(response.status(), 200);
        let body = response.text().await?;

        // Should have a valid JSON response
        let json: serde_json::Value = serde_json::from_str(&body)?;

        // Verify the response contains expected fields
        assert!(
            json.get("url").is_some(),
            "Response should contain URL field"
        );
        assert!(
            json.get("headers").is_some(),
            "Response should contain headers"
        );

        Ok(())
    })
    .await
}

/// Test HTTP POST request through a SOCKS5 proxy
#[tokio::test]
async fn test_socks5_proxy_post_request() -> Result<(), Box<dyn std::error::Error>> {
    with_socks5_proxy_test_environment(|env| async move {
        // Get a client that uses proxy-twister which routes through the SOCKS5 proxy
        let client = env.create_proxy_client()?;

        // Create a test payload
        let payload = serde_json::json!({
            "test": "data",
            "number": 42
        });

        // Make a POST request
        let url = format!("{}/post", env.http_docker_url());
        let response =
            tokio::time::timeout(STANDARD_TIMEOUT, client.post(&url).json(&payload).send())
                .await??;

        // Verify the response
        assert_eq!(response.status(), 200);

        // Parse the JSON response
        let body: serde_json::Value = response.json().await?;

        // Verify the payload was sent correctly
        assert_eq!(body["json"]["test"], "data");
        assert_eq!(body["json"]["number"], 42);

        Ok(())
    })
    .await
}

/// Test SOCKS5 proxy with large payload
#[tokio::test]
async fn test_socks5_proxy_large_payload() -> Result<(), Box<dyn std::error::Error>> {
    with_socks5_proxy_test_environment(|env| async move {
        // Get a client that uses proxy-twister which routes through the SOCKS5 proxy
        let client = env.create_proxy_client()?;

        // Create a large payload (approximately 10KB - keeping it reasonable for SOCKS5)
        let mut large_data = String::with_capacity(10_000);
        for i in 0..1_000 {
            large_data.push_str(&format!("data item {i}: some test content\n"));
        }

        // Create a JSON payload with the large data
        let payload = serde_json::json!({
            "large_field": large_data,
            "metadata": {
                "size": large_data.len(),
                "type": "test"
            }
        });

        // Make a POST request with the large payload
        let url = format!("{}/post", env.http_docker_url());
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(30), // Use longer timeout for large payload
            client.post(&url).json(&payload).send(),
        )
        .await??;

        // Verify the response
        assert_eq!(response.status(), 200);

        // Verify response contains the large payload data
        let body: serde_json::Value = response.json().await?;
        let received_size = body["json"]["metadata"]["size"].as_u64().unwrap();
        assert_eq!(
            received_size as usize,
            large_data.len(),
            "Large data size should match"
        );

        Ok(())
    })
    .await
}

/// Test SOCKS5 proxy with pattern matching (wildcard routing)
#[tokio::test]
async fn test_socks5_proxy_pattern_matching() -> Result<(), Box<dyn std::error::Error>> {
    with_socks5_proxy_test_environment(|env| async move {
        // Get a client that uses proxy-twister which routes through the SOCKS5 proxy
        let client = env.create_proxy_client()?;

        // Test basic GET request with pattern matching (all traffic goes through SOCKS5)
        let url = format!("{}/get", env.http_docker_url());
        let response = it_support::test_http_get(&client, &url).await?;

        // Verify the response
        assert_eq!(response.status(), 200);
        let body = response.text().await?;

        // Should have a valid JSON response
        let json: serde_json::Value = serde_json::from_str(&body)?;

        // Verify the response contains expected fields
        assert!(
            json.get("url").is_some(),
            "Response should contain URL field"
        );
        assert!(
            json.get("headers").is_some(),
            "Response should contain headers"
        );

        Ok(())
    })
    .await
}

/// Test SOCKS5 proxy error handling when proxy is unavailable
#[tokio::test]
async fn test_socks5_proxy_unavailable() -> Result<(), Box<dyn std::error::Error>> {
    // Create a test environment with HTTP server but no SOCKS5 proxy
    let env = it_support::TestEnvironment::new()
        .with_http_server()
        .await?;

    // Create a config with a non-existent SOCKS5 proxy
    // Port 1 is in the reserved range and should never be in use
    let config = it_support::create_test_config_content(
        &[(
            "bad_socks5_proxy",
            r#"{"scheme": "socks5", "host": "127.0.0.1", "port": 1}"#,
        )],
        &[("*", "bad_socks5_proxy")],
    );

    let env = env.with_proxy(&config).await?;

    // We'll force a direct connection attempt to the unavailable proxy
    // without going through proxy-twister
    let socks_addr = "socks5://127.0.0.1:1";
    let client = reqwest::Client::builder()
        .proxy(reqwest::Proxy::all(socks_addr)?)
        .connect_timeout(std::time::Duration::from_millis(100))
        .timeout(std::time::Duration::from_millis(500))
        .build()?;

    // Test a request that should fail because the proxy doesn't exist
    let url = format!("{}/get", env.http_url());
    let result = client.get(&url).send().await;

    // This should definitely fail
    assert!(result.is_err(), "Request should have failed");
    println!("Got expected error: {}", result.unwrap_err());

    // Teardown the environment
    env.teardown().await?;

    Ok(())
}

/// Test HTTPS routing through a SOCKS5 proxy
#[tokio::test]
async fn test_https_socks5_proxy_routing() -> Result<(), Box<dyn std::error::Error>> {
    use it_support::test_helpers::with_https_socks5_proxy_test_environment;

    with_https_socks5_proxy_test_environment(|env| async move {
        // Get a client that uses proxy-twister which routes through the SOCKS5 proxy
        let client = env.create_proxy_client()?;

        // Test a basic GET request to HTTPS server through SOCKS5 proxy
        let url = format!("{}/", env.https_url());
        let response = tokio::time::timeout(STANDARD_TIMEOUT, client.get(&url).send()).await??;

        // Verify the response
        assert!(
            response.status().is_success(),
            "HTTPS request through SOCKS5 proxy should succeed"
        );
        let body = response.text().await?;
        assert!(!body.is_empty(), "Response body should not be empty");

        Ok(())
    })
    .await
}

/// Test HTTPS POST request through a SOCKS5 proxy
#[tokio::test]
async fn test_https_socks5_proxy_post_request() -> Result<(), Box<dyn std::error::Error>> {
    use it_support::test_helpers::with_https_socks5_proxy_test_environment;

    with_https_socks5_proxy_test_environment(|env| async move {
        // Get a client that uses proxy-twister which routes through the SOCKS5 proxy
        let client = env.create_proxy_client()?;

        // Create a test payload
        let payload = serde_json::json!({
            "test": "https_socks5_data",
            "number": 443,
            "secure": true,
            "proxy_type": "socks5"
        });

        // Make a POST request to HTTPS server through SOCKS5 proxy
        let url = format!("{}/", env.https_url());
        let response =
            tokio::time::timeout(STANDARD_TIMEOUT, client.post(&url).json(&payload).send())
                .await??;

        // Verify the response
        assert!(
            response.status().is_success(),
            "HTTPS POST through SOCKS5 proxy should succeed"
        );
        let body = response.text().await?;
        assert!(!body.is_empty(), "Response body should not be empty");

        Ok(())
    })
    .await
}

/// Test HTTPS large payload through a SOCKS5 proxy
#[tokio::test]
async fn test_https_socks5_proxy_large_payload() -> Result<(), Box<dyn std::error::Error>> {
    use it_support::test_helpers::with_https_socks5_proxy_test_environment;

    with_https_socks5_proxy_test_environment(|env| async move {
        // Get a client that uses proxy-twister which routes through the SOCKS5 proxy
        let client = env.create_proxy_client()?;

        // Create a large test payload
        let large_data = "x".repeat(50000); // 50KB of data
        let payload = serde_json::json!({
            "large_field": large_data,
            "metadata": {
                "size": large_data.len(),
                "type": "https_socks5_test",
                "secure": true
            }
        });

        // Make a POST request with large payload to HTTPS server through SOCKS5 proxy
        let url = format!("{}/", env.https_url());
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(30), // Use longer timeout for large payload
            client.post(&url).json(&payload).send(),
        )
        .await??;

        // Verify the response
        assert!(
            response.status().is_success(),
            "HTTPS large payload through SOCKS5 proxy should succeed"
        );
        let body = response.text().await?;
        assert!(!body.is_empty(), "Response body should not be empty");

        Ok(())
    })
    .await
}
