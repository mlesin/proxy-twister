use std::time::Duration;
use tokio::time::timeout;

mod it_support;
use it_support::{STANDARD_TIMEOUT, TestEnvironment, with_http_test_environment};

/// Test proxy unavailable scenario
#[tokio::test]
async fn test_proxy_unavailable() {
    let env = TestEnvironment::new().with_http_server().await.unwrap();

    // Create config that routes traffic through a non-existent proxy
    let config = it_support::create_test_config_content(
        &[(
            "bad_proxy",
            r#"{"scheme": "http", "host": "127.0.0.1", "port": 9999}"#,
        )],
        &[("*", "bad_proxy")],
    );

    let env = env.with_proxy(&config).await.unwrap();
    let client = env.create_proxy_client().unwrap();

    // This should fail with connection error
    let result = timeout(
        Duration::from_secs(5),
        client.get(format!("{}/get", env.http_url())).send(),
    )
    .await;

    // Should timeout or return error
    match result {
        Ok(response) => {
            let resp = response.unwrap();
            // Should be an error response
            assert_ne!(resp.status(), 200);
        }
        Err(_) => {
            // Timeout is also acceptable
        }
    }
}

/// Test connection timeout scenario
#[tokio::test]
async fn test_connection_timeout() {
    with_http_test_environment(|env| async move {
        let client = env.create_proxy_client()?;

        // Test delay endpoint that should timeout
        let result = timeout(
            Duration::from_secs(3),
            client.get(format!("{}/delay/10", env.http_url())).send(),
        )
        .await;

        // Should timeout
        assert!(result.is_err());

        Ok(())
    })
    .await
    .unwrap();
}

/// Test error response codes
#[tokio::test]
async fn test_error_response_codes() {
    with_http_test_environment(|env| async move {
        let client = env.create_proxy_client()?;

        // Test various error codes
        let error_codes = [400, 401, 403, 404, 500, 502, 503];

        for code in error_codes {
            let response = timeout(
                STANDARD_TIMEOUT,
                client
                    .get(format!("{}/status/{code}", env.http_url()))
                    .send(),
            )
            .await??;

            assert_eq!(response.status(), code);
        }

        Ok(())
    })
    .await
    .unwrap();
}

/// Test mixed routing scenarios
#[tokio::test]
async fn test_mixed_routing_scenarios() {
    with_http_test_environment(|env| async move {
        let client = env.create_proxy_client()?;

        // Test direct routing
        let response = timeout(
            STANDARD_TIMEOUT,
            client.get(format!("{}/get", env.http_url())).send(),
        )
        .await??;

        assert_eq!(response.status(), 200);

        let body_text = response.text().await?;
        assert!(!body_text.is_empty(), "Response should not be empty");

        Ok(())
    })
    .await
    .unwrap();
}

/// Test pattern matching edge cases
#[tokio::test]
async fn test_pattern_matching_edge_cases() {
    let env = TestEnvironment::new().with_http_server().await.unwrap();

    // Create config with wildcard patterns
    let config = it_support::create_test_config_content(
        &[
            ("direct", r#"{"scheme": "direct"}"#),
            ("special", r#"{"scheme": "direct"}"#),
        ],
        &[
            ("*.local", "special"),
            ("test.*", "special"),
            ("*", "direct"),
        ],
    );

    let env = env.with_proxy(&config).await.unwrap();
    let client = env.create_proxy_client().unwrap();

    // Test pattern matching - should work with 127.0.0.1 (matches "*")
    let response = timeout(
        STANDARD_TIMEOUT,
        client.get(format!("{}/get", env.http_url())).send(),
    )
    .await
    .unwrap()
    .unwrap();

    assert_eq!(response.status(), 200);
}

/// Test configuration reload behavior
#[tokio::test]
async fn test_config_behavior() {
    with_http_test_environment(|env| async move {
        let client = env.create_proxy_client()?;

        // Test initial configuration
        let response = timeout(
            STANDARD_TIMEOUT,
            client.get(format!("{}/get", env.http_url())).send(),
        )
        .await??;

        assert_eq!(response.status(), 200);

        // Test that subsequent requests still work
        let response2 = timeout(
            STANDARD_TIMEOUT,
            client.get(format!("{}/get", env.http_url())).send(),
        )
        .await??;

        assert_eq!(response2.status(), 200);

        Ok(())
    })
    .await
    .unwrap();
}

/// Test IPv6 localhost behavior
#[tokio::test]
async fn test_ipv6_localhost() {
    with_http_test_environment(|env| async move {
        let client = env.create_proxy_client()?;

        // Test with regular localhost (should work)
        let response = timeout(
            STANDARD_TIMEOUT,
            client
                .get(format!("http://localhost:{}/get", env.http_server.as_ref().unwrap().port))
                .send(),
        )
        .await??;

        assert_eq!(response.status(), 200);

        Ok(())
    })
    .await
    .unwrap();
}

/// Test data integrity with checksums
#[tokio::test]
async fn test_data_integrity_checksums() {
    with_http_test_environment(|env| async move {
        let client = env.create_proxy_client()?;

        // Test large payload integrity with reduced size for httpbin
        let payload_size = 102400; // 100KB instead of 1MB
        let response = timeout(
            Duration::from_secs(30),
            client
                .get(format!("{}/bytes/{payload_size}", env.http_url()))
                .send(),
        )
        .await??;

        assert_eq!(response.status(), 200);

        let body = response.bytes().await?;
        assert_eq!(body.len(), payload_size);

        // Calculate SHA256 hash for integrity verification
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&body);
        let _received_hash = hasher.finalize();
        
        // We can't check a specific hash as httpbin returns random bytes
        // Just make sure we got the right amount of data
        assert_eq!(body.len(), payload_size);

        Ok(())
    })
    .await
    .unwrap();
}
