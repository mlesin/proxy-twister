mod it_support;
use futures::future::join_all;
use it_support::{
    STANDARD_TIMEOUT, TestEnvironment, test_http_get, test_http_post, with_http_test_environment,
};
use std::time::Duration;
use tokio::time::timeout;

/// Test HTTP direct routing with the optimized test environment
#[tokio::test]
async fn test_http_direct_routing() -> Result<(), Box<dyn std::error::Error>> {
    with_http_test_environment(|env| async move {
        // Get a client that uses the proxy with direct routing
        let client = env.create_proxy_client()?;

        // Test a basic GET request through direct routing
        let url = format!("{}/get", env.http_url());
        let response = test_http_get(&client, &url).await?;

        // Verify the response
        assert_eq!(response.status(), 200);
        let body = response.text().await?;
        assert!(!body.is_empty(), "Response body should not be empty");

        // Compare with direct client to ensure equivalent behavior
        let direct_client = env.create_direct_client()?;
        let direct_response = test_http_get(&direct_client, &url).await?;

        assert_eq!(direct_response.status(), 200);
        let direct_body = direct_response.text().await?;
        assert!(
            !direct_body.is_empty(),
            "Direct response body should not be empty"
        );

        Ok(())
    })
    .await
}

/// Test HTTP POST request through direct routing
#[tokio::test]
async fn test_http_direct_post_request() -> Result<(), Box<dyn std::error::Error>> {
    with_http_test_environment(|env| async move {
        // Get a client that uses the proxy with direct routing
        let client = env.create_proxy_client()?;

        // Create a test payload
        let payload = serde_json::json!({
            "test": "data",
            "number": 42,
            "routing": "direct"
        });

        // Test a POST request through direct routing
        let url = format!("{}/post", env.http_url());
        let response = test_http_post(&client, &url, &payload).await?;

        // Verify the response
        assert_eq!(response.status(), 200);

        // Parse the JSON response
        let body: serde_json::Value = response.json().await?;

        // Verify the payload was sent correctly
        assert_eq!(body["json"]["test"], "data");
        assert_eq!(body["json"]["number"], 42);
        assert_eq!(body["json"]["routing"], "direct");

        Ok(())
    })
    .await
}

/// Test HTTP POST request through direct routing with alternative environment setup
/// This test was originally in test_http.rs and tests the same functionality
/// with the older TestEnvironment approach for comparison
#[tokio::test]
async fn test_http_direct_post_request_alt_setup() -> Result<(), Box<dyn std::error::Error>> {
    // Create a test environment with HTTP server and direct proxy
    let env = TestEnvironment::new()
        .with_http_server()
        .await?
        .with_direct_proxy()
        .await?;

    // Get a client that uses the proxy
    let client = env.create_proxy_client()?;

    // Create test payload
    let payload = serde_json::json!({
        "test": "data",
        "number": 42
    });

    // Test a POST request
    let url = format!("{}/post", env.http_url());
    let response =
        tokio::time::timeout(STANDARD_TIMEOUT, client.post(&url).json(&payload).send()).await??;

    // Verify the response
    assert_eq!(response.status(), 200);
    let body: serde_json::Value = response.json().await?;

    // Verify payload was received correctly
    assert_eq!(body["json"]["test"], "data");
    assert_eq!(body["json"]["number"], 42);

    // Clean up resources
    env.teardown().await?;

    Ok(())
}

/// Test HTTP large payload through direct routing
#[tokio::test]
async fn test_http_direct_large_payload() -> Result<(), Box<dyn std::error::Error>> {
    with_http_test_environment(|env| async move {
        // Get a client that uses the proxy with direct routing
        let client = env.create_proxy_client()?;

        // Test downloading a large payload (100KB)
        let payload_size = 102400; // 100KB
        let url = format!("{}/bytes/{payload_size}", env.http_url());

        let response = tokio::time::timeout(STANDARD_TIMEOUT, client.get(&url).send()).await??;

        // Verify the response
        assert_eq!(response.status(), 200);
        let body = response.bytes().await?;
        assert_eq!(
            body.len(),
            payload_size,
            "Downloaded payload size should match requested size"
        );

        Ok(())
    })
    .await
}

/// Test HTTP host pattern matching with direct routing
#[tokio::test]
async fn test_http_direct_host_pattern_matching() -> Result<(), Box<dyn std::error::Error>> {
    // Create a test environment with HTTP server
    let env = TestEnvironment::new().with_http_server().await?;

    // Create config with pattern matching for different hosts
    let config = it_support::create_test_config_content(
        &[("direct", r#"{"scheme": "direct"}"#)],
        &[
            ("localhost", "direct"),
            ("127.0.0.1", "direct"),
            ("*.local", "direct"),
            ("test.*", "direct"),
        ],
    );

    let env = env.with_proxy(&config).await?;

    // Get a client that uses the proxy
    let client = env.create_proxy_client()?;

    // Test accessing through localhost pattern
    let localhost_url = format!(
        "http://localhost:{}/get",
        env.http_server.as_ref().unwrap().port
    );
    let response = test_http_get(&client, &localhost_url).await?;
    assert_eq!(response.status(), 200);

    // Test accessing through 127.0.0.1 pattern
    let ip_url = format!(
        "http://127.0.0.1:{}/get",
        env.http_server.as_ref().unwrap().port
    );
    let response = test_http_get(&client, &ip_url).await?;
    assert_eq!(response.status(), 200);

    // Clean up resources
    env.teardown().await?;

    Ok(())
}

/// Test concurrent connections through direct routing
/// This test was originally in test_integration.rs
#[tokio::test]
async fn test_concurrent_direct_connections() -> Result<(), Box<dyn std::error::Error>> {
    with_http_test_environment(|env| async move {
        // Get a client that uses the proxy
        let client = env.create_proxy_client()?;

        // Create multiple concurrent request futures
        let base_url = env.http_url();
        let request_futures = (0..5).map(|i| {
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
        let results = join_all(request_futures).await;

        // Verify all requests succeeded
        for result in results {
            result?; // Propagate any errors
        }

        Ok(())
    })
    .await
}

/// Test error response handling through direct routing
/// This test was originally in test_integration.rs
#[tokio::test]
async fn test_direct_error_handling() -> Result<(), Box<dyn std::error::Error>> {
    with_http_test_environment(|env| async move {
        // Get a client that uses the proxy
        let client = env.create_proxy_client()?;

        // Test 404 error handling
        let url = format!("{}/status/404", env.http_url());
        let response = tokio::time::timeout(STANDARD_TIMEOUT, client.get(&url).send()).await??;

        assert_eq!(response.status(), 404);

        // Test 500 error handling
        let url = format!("{}/status/500", env.http_url());
        let response = tokio::time::timeout(STANDARD_TIMEOUT, client.get(&url).send()).await??;

        assert_eq!(response.status(), 500);

        Ok(())
    })
    .await
}

/// Comprehensive direct routing test that demonstrates all key functionality
/// This test combines demo, functional, and comprehensive testing for direct routing
#[tokio::test]
async fn test_comprehensive_direct_routing() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Starting comprehensive direct routing functionality test...");

    with_http_test_environment(|env| async move {
        println!(
            "   ✓ HTTP echo server started on port {}",
            env.http_server.as_ref().unwrap().port
        );
        println!(
            "   ✓ Proxy-twister started on port {}",
            env.proxy_instance.as_ref().unwrap().port
        );

        let client = env.create_proxy_client()?;

        println!("\n📊 Running comprehensive direct routing functionality tests...");

        // Test 1: Basic GET request
        print!("   → Testing basic GET request... ");
        let url = format!("{}/get", env.http_url());
        let response = test_http_get(&client, &url).await?;
        assert_eq!(response.status(), 200);

        let body: serde_json::Value = response.json().await?;
        assert!(
            body.get("url").is_some(),
            "Response should contain URL field"
        );
        assert!(
            body.get("headers").is_some(),
            "Response should contain headers"
        );
        println!("✅");

        // Test 2: POST with JSON payload
        print!("   → Testing POST with JSON payload... ");
        let test_payload = serde_json::json!({
            "test_type": "comprehensive_direct",
            "features": ["routing", "json", "http", "direct"],
            "success": true,
            "timestamp": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
        });

        let url = format!("{}/post", env.http_url());
        let response = test_http_post(&client, &url, &test_payload).await?;
        assert_eq!(response.status(), 200);

        let body: serde_json::Value = response.json().await?;
        assert_eq!(body["json"]["test_type"], "comprehensive_direct");
        assert_eq!(body["json"]["success"], true);
        println!("✅");

        // Test 3: Large payload download
        print!("   → Testing large payload download (100KB)... ");
        let payload_size = 102400; // 100KB
        let url = format!("{}/bytes/{payload_size}", env.http_url());
        let response = timeout(Duration::from_secs(30), client.get(&url).send()).await??;
        assert_eq!(response.status(), 200);
        let body = response.bytes().await?;
        assert_eq!(
            body.len(),
            payload_size,
            "Downloaded payload size should match requested size"
        );
        println!("✅");

        // Test 4: Multiple HTTP methods
        print!("   → Testing various HTTP methods... ");

        // PUT request
        let put_response = timeout(
            STANDARD_TIMEOUT,
            client
                .put(format!("{}/put", env.http_url()))
                .header("Content-Type", "text/plain")
                .body("PUT test data")
                .send(),
        )
        .await??;
        assert_eq!(put_response.status(), 200);

        // DELETE request
        let delete_response = timeout(
            STANDARD_TIMEOUT,
            client.delete(format!("{}/delete", env.http_url())).send(),
        )
        .await??;
        assert_eq!(delete_response.status(), 200);
        println!("✅");

        // Test 5: Error status handling
        print!("   → Testing error status handling... ");
        let test_codes = [404, 500];
        for code in test_codes {
            let response = timeout(
                STANDARD_TIMEOUT,
                client
                    .get(format!("{}/status/{code}", env.http_url()))
                    .send(),
            )
            .await??;
            assert_eq!(response.status(), code);
        }
        println!("✅");

        // Test 6: Concurrent requests
        print!("   → Testing concurrent requests... ");
        let mut tasks = Vec::new();
        for i in 0..5 {
            let client = client.clone();
            let url = format!("{}/get?concurrent={i}", env.http_url());
            let task = tokio::spawn(async move {
                timeout(Duration::from_secs(15), client.get(&url).send())
                    .await
                    .unwrap()
                    .unwrap()
            });
            tasks.push(task);
        }

        let results = join_all(tasks).await;
        for result in results {
            let response = result?;
            assert_eq!(response.status(), 200);
        }
        println!("✅");

        // Test 7: Query parameters and headers
        print!("   → Testing query parameters and custom headers... ");
        let response = timeout(
            STANDARD_TIMEOUT,
            client
                .get(format!(
                    "{}/get?param1=value1&param2=value2",
                    env.http_url()
                ))
                .header("X-Test-Header", "test-value")
                .header("X-Custom", "custom-value")
                .send(),
        )
        .await??;
        assert_eq!(response.status(), 200);

        let body: serde_json::Value = response.json().await?;
        assert_eq!(body["args"]["param1"], "value1");
        assert_eq!(body["args"]["param2"], "value2");
        assert_eq!(body["headers"]["X-Test-Header"], "test-value");
        println!("✅");

        println!("\n🎉 All comprehensive direct routing functionality tests passed!");
        Ok(())
    })
    .await
}

/// Test direct routing with custom configuration setup
/// This test demonstrates how proxy-twister works with custom direct routing configurations
#[tokio::test]
async fn test_direct_routing_with_custom_config() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔧 Testing direct routing with custom configuration...");

    // Create environment with HTTP server
    let env = TestEnvironment::new().with_http_server().await?;

    // Create a custom configuration for direct routing
    let config = serde_json::json!({
        "profiles": {
            "direct": {
                "scheme": "direct"
            },
            "fallback": {
                "scheme": "direct"
            }
        },
        "switch": {
            "default": "direct",
            "rules": [
                {
                    "pattern": "127.0.0.1",
                    "profile": "direct"
                },
                {
                    "pattern": "localhost",
                    "profile": "direct"
                },
                {
                    "pattern": "*.local",
                    "profile": "direct"
                },
                {
                    "pattern": "*",
                    "profile": "fallback"
                }
            ]
        }
    })
    .to_string();

    let env = env.with_proxy(&config).await?;
    let client = env.create_proxy_client()?;

    // Test 1: Direct routing for localhost
    println!("   → Testing localhost routing...");
    let localhost_url = format!(
        "http://localhost:{}/get",
        env.http_server.as_ref().unwrap().port
    );
    let response = timeout(STANDARD_TIMEOUT, client.get(&localhost_url).send()).await??;
    assert_eq!(response.status(), 200);

    // Test 2: Direct routing for 127.0.0.1
    println!("   → Testing IP address routing...");
    let ip_url = format!(
        "http://127.0.0.1:{}/get",
        env.http_server.as_ref().unwrap().port
    );
    let response = timeout(STANDARD_TIMEOUT, client.get(&ip_url).send()).await??;
    assert_eq!(response.status(), 200);

    // Test 3: POST with comprehensive payload
    println!("   → Testing POST with comprehensive payload...");
    let comprehensive_payload = serde_json::json!({
        "integration_test": true,
        "timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        "message": "Testing proxy-twister direct routing functionality",
        "config_type": "custom_direct",
        "features_tested": [
            "direct_routing",
            "pattern_matching",
            "json_payloads",
            "custom_configuration"
        ]
    });

    let response = timeout(
        STANDARD_TIMEOUT,
        client
            .post(format!("{}/post", env.http_url()))
            .header("Content-Type", "application/json")
            .json(&comprehensive_payload)
            .send(),
    )
    .await??;

    assert_eq!(response.status(), 200);
    let body: serde_json::Value = response.json().await?;
    assert_eq!(body["json"]["integration_test"], true);
    assert_eq!(body["json"]["config_type"], "custom_direct");

    // Clean up
    env.teardown().await?;

    println!("✅ Direct routing with custom config test passed!");
    Ok(())
}

/// Test direct routing configuration robustness and edge cases
/// This test verifies that direct routing handles various configuration scenarios correctly
#[tokio::test]
async fn test_direct_routing_configuration_robustness() -> Result<(), Box<dyn std::error::Error>> {
    println!("⚙️  Testing direct routing configuration robustness...");

    // Test 1: Invalid configuration handling
    println!("   → Testing invalid configuration handling...");
    let env = TestEnvironment::new().with_http_server().await?;
    let invalid_config = r#"{"invalid": "json structure without required fields"}"#;
    let result = env.with_proxy(invalid_config).await;
    assert!(result.is_err(), "Should fail with invalid configuration");

    // Test 2: Valid minimal direct routing configuration
    println!("   → Testing minimal valid direct routing configuration...");
    let env2 = TestEnvironment::new().with_http_server().await?;
    let minimal_config = serde_json::json!({
        "profiles": {
            "direct": {
                "scheme": "direct"
            }
        },
        "switch": {
            "default": "direct",
            "rules": []
        }
    })
    .to_string();

    let env2 = env2.with_proxy(&minimal_config).await?;
    let client = env2.create_proxy_client()?;

    // Verify the minimal config works
    let response = timeout(
        STANDARD_TIMEOUT,
        client.get(format!("{}/get", env2.http_url())).send(),
    )
    .await??;
    assert_eq!(response.status(), 200);

    env2.teardown().await?;

    println!("✅ Direct routing configuration robustness test passed!");
    Ok(())
}

/// Test HTTPS direct routing with self-signed certificates
/// Note: Currently using HTTP nginx container to test the infrastructure
/// TODO: Implement actual HTTPS with SSL configuration
#[tokio::test]
async fn test_https_direct_routing() -> Result<(), Box<dyn std::error::Error>> {
    use it_support::test_helpers::with_https_test_environment;

    with_https_test_environment(|env| async move {
        // Get a client that uses the proxy with direct routing
        // This client is configured to handle HTTPS and self-signed certificates
        let client = env.create_proxy_client()?;

        // Test a basic GET request through direct routing
        // (using HTTP nginx for now, but with HTTPS-capable client)
        let url = format!("{}/", env.https_url());
        let response = timeout(STANDARD_TIMEOUT, client.get(&url).send()).await??;

        // Verify the response
        assert!(response.status().is_success(), "Request should succeed");
        let body = response.text().await?;
        assert!(!body.is_empty(), "Response body should not be empty");

        // Compare with direct client to ensure equivalent behavior
        let direct_client = env.create_direct_client()?;
        let direct_response = timeout(STANDARD_TIMEOUT, direct_client.get(&url).send()).await??;

        assert!(
            direct_response.status().is_success(),
            "Direct request should succeed"
        );
        let direct_body = direct_response.text().await?;
        assert!(
            !direct_body.is_empty(),
            "Direct response body should not be empty"
        );

        Ok(())
    })
    .await
}

/// Test HTTPS direct routing with different endpoints
/// Note: Currently using HTTP nginx container to test the infrastructure
#[tokio::test]
async fn test_https_direct_routing_endpoints() -> Result<(), Box<dyn std::error::Error>> {
    use it_support::test_helpers::with_https_test_environment;

    with_https_test_environment(|env| async move {
        // Get a client that uses the proxy with direct routing
        let client = env.create_proxy_client()?;

        // Test different endpoints that should be available in the nginx container
        let test_endpoints = vec!["/", "/index.html"];

        for endpoint in test_endpoints {
            let url = format!("{}{endpoint}", env.https_url());
            let response = timeout(STANDARD_TIMEOUT, client.get(&url).send()).await??;

            // Verify the response
            assert!(
                response.status().is_success() || response.status().as_u16() == 404,
                "Request to {endpoint} should succeed or return 404, got: {status}",
                endpoint = endpoint,
                status = response.status()
            );
        }

        Ok(())
    })
    .await
}

/// Test HTTPS direct routing with concurrent requests
/// Note: Currently using HTTP nginx container to test the infrastructure
#[tokio::test]
async fn test_https_direct_routing_concurrent() -> Result<(), Box<dyn std::error::Error>> {
    use it_support::test_helpers::with_https_test_environment;

    with_https_test_environment(|env| async move {
        // Get a client that uses the proxy with direct routing
        let client = env.create_proxy_client()?;

        // Make multiple concurrent requests
        let mut handles = Vec::new();
        for i in 0..5 {
            let client = client.clone();
            let url = format!("{}/?req={i}", env.https_url());

            let handle =
                tokio::spawn(
                    async move { timeout(STANDARD_TIMEOUT, client.get(&url).send()).await },
                );
            handles.push(handle);
        }

        // Wait for all requests to complete
        for handle in handles {
            let response = handle.await???;
            assert!(
                response.status().is_success() || response.status().as_u16() == 404,
                "Concurrent request should succeed or return 404, got: {status}",
                status = response.status()
            );
        }

        Ok(())
    })
    .await
}
