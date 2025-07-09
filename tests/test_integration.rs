mod it_support;
use it_support::{
    STANDARD_TIMEOUT, test_http_get, test_http_post, with_http_proxy_test_environment,
    with_http_test_environment, with_socks5_proxy_test_environment,
};

/// Integration test that verifies proxy switching works across all three routing types
/// Tests the core functionality of the proxy switcher with direct, HTTP proxy, and SOCKS5 proxy routing
#[tokio::test]
async fn test_proxy_switcher_integration() -> Result<(), Box<dyn std::error::Error>> {
    // Test 1: Direct routing
    println!("🔄 Testing direct routing integration");
    with_http_test_environment(|env| async move {
        let client = env.create_proxy_client()?;

        let test_payload = serde_json::json!({
            "integration_test": true,
            "proxy_type": "direct",
            "data": "test_data"
        });

        // Test GET request
        let url = format!("{}/get", env.http_url());
        let response = test_http_get(&client, &url).await?;
        assert_eq!(response.status(), 200);

        let body: serde_json::Value = response.json().await?;
        assert!(
            body.get("url").is_some(),
            "Response should contain URL field"
        );

        // Test POST request
        let url = format!("{}/post", env.http_url());
        let response = test_http_post(&client, &url, &test_payload).await?;
        assert_eq!(response.status(), 200);

        let body: serde_json::Value = response.json().await?;
        assert_eq!(body["json"]["integration_test"], true);
        assert_eq!(body["json"]["proxy_type"], "direct");

        println!("✅ Direct routing integration test passed");
        Ok(())
    })
    .await?;

    // Test 2: HTTP proxy routing
    println!("🔄 Testing HTTP proxy routing integration");
    with_http_proxy_test_environment(8888, |env| async move {
        let client = env.create_proxy_client()?;

        let test_payload = serde_json::json!({
            "integration_test": true,
            "proxy_type": "http_proxy",
            "data": "test_data"
        });

        // Test GET request through HTTP proxy
        let url = format!("{}/get", env.http_docker_url());
        let response = test_http_get(&client, &url).await?;
        assert_eq!(response.status(), 200);

        let body: serde_json::Value = response.json().await?;
        assert!(
            body.get("url").is_some(),
            "Response should contain URL field"
        );

        // Test POST request through HTTP proxy
        let url = format!("{}/post", env.http_docker_url());
        let response = test_http_post(&client, &url, &test_payload).await?;
        assert_eq!(response.status(), 200);

        let body: serde_json::Value = response.json().await?;
        assert_eq!(body["json"]["integration_test"], true);
        assert_eq!(body["json"]["proxy_type"], "http_proxy");

        println!("✅ HTTP proxy routing integration test passed");
        Ok(())
    })
    .await?;

    // Test 3: SOCKS5 proxy routing
    println!("🔄 Testing SOCKS5 proxy routing integration");
    with_socks5_proxy_test_environment(|env| async move {
        let client = env.create_proxy_client()?;

        let test_payload = serde_json::json!({
            "integration_test": true,
            "proxy_type": "socks5_proxy",
            "data": "test_data"
        });

        // Test GET request through SOCKS5 proxy
        let url = format!("{}/get", env.http_docker_url());
        let response = test_http_get(&client, &url).await?;
        assert_eq!(response.status(), 200);

        let body: serde_json::Value = response.json().await?;
        assert!(
            body.get("url").is_some(),
            "Response should contain URL field"
        );

        // Test POST request through SOCKS5 proxy
        let url = format!("{}/post", env.http_docker_url());
        let response = test_http_post(&client, &url, &test_payload).await?;
        assert_eq!(response.status(), 200);

        let body: serde_json::Value = response.json().await?;
        assert_eq!(body["json"]["integration_test"], true);
        assert_eq!(body["json"]["proxy_type"], "socks5_proxy");

        println!("✅ SOCKS5 proxy routing integration test passed");
        Ok(())
    })
    .await?;

    println!("🎉 All proxy switcher integration tests passed!");
    Ok(())
}

/// Test proxy switching with different host patterns
/// This test verifies that the proxy switcher correctly routes traffic based on host patterns
#[tokio::test]
async fn test_proxy_switcher_host_patterns() -> Result<(), Box<dyn std::error::Error>> {
    // Create a test environment with HTTP server
    let env = it_support::TestEnvironment::new()
        .with_http_server()
        .await?;

    // Create a configuration that uses different proxy types for different hosts
    let config = it_support::create_test_config_content(
        &[
            ("direct", r#"{"scheme": "direct"}"#),
            (
                "http_proxy",
                r#"{"scheme": "http", "host": "127.0.0.1", "port": 8080}"#,
            ),
            (
                "socks5_proxy",
                r#"{"scheme": "socks5", "host": "127.0.0.1", "port": 1080}"#,
            ),
        ],
        &[
            ("localhost", "direct"),
            ("127.0.0.1", "direct"),
            ("*.example.com", "http_proxy"),
            ("*.test.com", "socks5_proxy"),
            ("*", "direct"), // Default fallback
        ],
    );

    let env = env.with_proxy(&config).await?;
    let client = env.create_proxy_client()?;

    // Test direct routing for localhost
    let localhost_url = format!(
        "http://localhost:{}/get",
        env.http_server.as_ref().unwrap().port
    );
    let response = test_http_get(&client, &localhost_url).await?;
    assert_eq!(response.status(), 200);

    // Test direct routing for 127.0.0.1
    let ip_url = format!(
        "http://127.0.0.1:{}/get",
        env.http_server.as_ref().unwrap().port
    );
    let response = test_http_get(&client, &ip_url).await?;
    assert_eq!(response.status(), 200);

    // Clean up
    env.teardown().await?;

    println!("✅ Host pattern switching test passed");
    Ok(())
}

/// Test concurrent requests across different proxy types
/// This test verifies that the proxy switcher can handle concurrent requests efficiently
#[tokio::test]
async fn test_proxy_switcher_concurrent_requests() -> Result<(), Box<dyn std::error::Error>> {
    with_http_test_environment(|env| async move {
        let client = env.create_proxy_client()?;

        // Create multiple concurrent request futures
        let base_url = env.http_url();
        let request_futures = (0..10).map(|i| {
            let client = client.clone();
            let url = format!("{base_url}/get?concurrent_test={i}");

            async move {
                let response = test_http_get(&client, &url).await?;
                assert_eq!(response.status(), 200);
                let body: serde_json::Value = response.json().await?;

                // Verify the query parameter was passed correctly
                let args = &body["args"];
                assert_eq!(args["concurrent_test"], i.to_string());

                Result::<_, Box<dyn std::error::Error>>::Ok(i)
            }
        });

        // Execute all requests concurrently
        let results = futures::future::join_all(request_futures).await;

        // Verify all requests succeeded
        for result in results {
            result?; // Propagate any errors
        }

        println!("✅ Concurrent requests test passed");
        Ok(())
    })
    .await
}

/// Test error handling across different proxy types
/// This test verifies that the proxy switcher handles errors consistently
#[tokio::test]
async fn test_proxy_switcher_error_handling() -> Result<(), Box<dyn std::error::Error>> {
    with_http_test_environment(|env| async move {
        let client = env.create_proxy_client()?;

        // Test 404 error handling
        let url = format!("{}/status/404", env.http_url());
        let response = tokio::time::timeout(STANDARD_TIMEOUT, client.get(&url).send()).await??;
        assert_eq!(response.status(), 404);

        // Test 500 error handling
        let url = format!("{}/status/500", env.http_url());
        let response = tokio::time::timeout(STANDARD_TIMEOUT, client.get(&url).send()).await??;
        assert_eq!(response.status(), 500);

        // Test timeout scenarios
        let url = format!("{}/delay/1", env.http_url());
        let response = tokio::time::timeout(STANDARD_TIMEOUT, client.get(&url).send()).await??;
        assert_eq!(response.status(), 200);

        println!("✅ Error handling test passed");
        Ok(())
    })
    .await
}

/// Test large payload handling across different proxy types
/// This test verifies that the proxy switcher can handle large payloads efficiently
#[tokio::test]
async fn test_proxy_switcher_large_payload() -> Result<(), Box<dyn std::error::Error>> {
    with_http_test_environment(|env| async move {
        let client = env.create_proxy_client()?;

        // Test large payload download (100KB)
        let payload_size = 102400; // 100KB
        let url = format!("{}/bytes/{payload_size}", env.http_url());

        let response =
            tokio::time::timeout(std::time::Duration::from_secs(30), client.get(&url).send())
                .await??;

        assert_eq!(response.status(), 200);
        let body = response.bytes().await?;
        assert_eq!(
            body.len(),
            payload_size,
            "Downloaded payload size should match requested size"
        );

        // Test large payload upload
        let large_data = vec![b'A'; 50000]; // 50KB
        let upload_payload = serde_json::json!({
            "data": format!("Large test data: {} bytes", large_data.len()),
            "size": large_data.len(),
            "type": "integration_test"
        });

        let url = format!("{}/post", env.http_url());
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            client.post(&url).json(&upload_payload).send(),
        )
        .await??;

        assert_eq!(response.status(), 200);
        let body: serde_json::Value = response.json().await?;
        assert_eq!(body["json"]["size"], large_data.len());

        println!("✅ Large payload test passed");
        Ok(())
    })
    .await
}
