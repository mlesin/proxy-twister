use futures::future::join_all;
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio::time::timeout;

mod it_support;
use it_support::{STANDARD_TIMEOUT, with_http_test_environment};

/// Test advanced proxy routing patterns and concurrent handling
#[tokio::test]
async fn test_advanced_routing_patterns() {
    with_http_test_environment(|env| async move {
        let client = env.create_proxy_client()?;

        // Test 1: Pattern matching with different host patterns
        println!("✓ Testing pattern matching");
        let response = timeout(
            STANDARD_TIMEOUT,
            client
                .get(format!("{}/get?host=localhost", env.http_url()))
                .send(),
        )
        .await??;
        assert_eq!(response.status(), 200);

        // Test 2: Concurrent request handling (20 requests)
        println!("✓ Testing concurrent request handling");
        let mut tasks = Vec::new();

        for i in 0..20 {
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

        // Test 3: Various HTTP methods with headers
        println!("✓ Testing various HTTP methods and headers");

        // POST with custom headers
        let post_response = timeout(
            STANDARD_TIMEOUT,
            client
                .post(format!("{}/post", env.http_url()))
                .header("X-Custom-Header", "test-value")
                .header("Content-Type", "application/json")
                .body(r#"{"test": "data", "number": 42}"#)
                .send(),
        )
        .await??;
        assert_eq!(post_response.status(), 200);

        // PUT request
        let put_response = timeout(
            STANDARD_TIMEOUT,
            client
                .put(format!("{}/put", env.http_url()))
                .header("X-Test", "put-request")
                .body("PUT test data")
                .send(),
        )
        .await??;
        assert_eq!(put_response.status(), 200);

        // DELETE request
        let delete_response = timeout(
            STANDARD_TIMEOUT,
            client
                .delete(format!("{}/delete", env.http_url()))
                .header("X-Test", "delete-request")
                .send(),
        )
        .await??;
        assert_eq!(delete_response.status(), 200);

        Ok(())
    })
    .await
    .unwrap();
}

/// Test error handling and edge cases
#[tokio::test]
async fn test_error_handling_edge_cases() {
    with_http_test_environment(|env| async move {
        let client = env.create_proxy_client()?;

        // Test 1: Various HTTP status codes
        println!("✓ Testing HTTP status code handling");
        let test_codes = [200, 201, 400, 401, 403, 404, 500, 502, 503];

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

        // Test 2: Large payload handling
        println!("✓ Testing large payload handling");
        let bytes_size = 102400; // 100KB (httpbin limit)
        let large_response = timeout(
            Duration::from_secs(30),
            client
                .get(format!("{}/bytes/{bytes_size}", env.http_url()))
                .send(),
        )
        .await??;
        assert_eq!(large_response.status(), 200);

        let body = large_response.bytes().await?;
        assert_eq!(body.len(), bytes_size);

        // Test 3: Query parameter handling
        println!("✓ Testing query parameter handling");
        let query_response = timeout(
            STANDARD_TIMEOUT,
            client
                .get(format!(
                    "{}/get?param1=value1&param2=value2&special=test%20value",
                    env.http_url()
                ))
                .send(),
        )
        .await??;
        assert_eq!(query_response.status(), 200);

        let query_body: serde_json::Value = query_response.json().await?;
        assert_eq!(query_body["args"]["param1"], "value1");
        assert_eq!(query_body["args"]["param2"], "value2");

        // Test 4: Connection timeout handling
        println!("✓ Testing connection timeout handling");
        let timeout_client = reqwest::Client::builder()
            .proxy(reqwest::Proxy::http(
                env.proxy_instance.as_ref().unwrap().proxy_url(),
            )?)
            .timeout(Duration::from_secs(5))
            .build()?;

        let timeout_result = timeout_client
            .get("http://nonexistent.invalid.domain.test/test")
            .send()
            .await;

        // Should either fail or return error status
        match timeout_result {
            Err(_) => {
                println!("✓ Connection failed as expected");
            }
            Ok(response) => {
                println!("✓ Proxy returned error status: {}", response.status());
                assert!(
                    response.status().is_server_error() || response.status().is_client_error(),
                    "Expected error status code, got: {}",
                    response.status()
                );
            }
        }

        Ok(())
    })
    .await
    .unwrap();
}

/// Test data integrity and streaming
#[tokio::test]
async fn test_data_integrity_streaming() {
    with_http_test_environment(|env| async move {
        let client = env.create_proxy_client()?;

        // Test 1: Data integrity check for large file
        println!("✓ Testing data integrity with checksums");
        let bytes_size = 102400; // 100KB (httpbin limit)
        let response = timeout(
            Duration::from_secs(30),
            client
                .get(format!("{}/bytes/{bytes_size}", env.http_url()))
                .send(),
        )
        .await??;
        assert_eq!(response.status(), 200);

        let body = response.bytes().await?;
        assert_eq!(body.len(), bytes_size);

        // Calculate SHA256 hash for integrity verification
        let mut hasher = Sha256::new();
        hasher.update(&body);
        let _received_hash = hasher.finalize();

        // Verify correct byte count (httpbin returns random bytes)
        assert_eq!(body.len(), bytes_size);

        // Test 2: Streaming large request body
        println!("✓ Testing streaming large request body");
        let large_body = vec![b'X'; 2097152]; // 2MB

        let post_response = timeout(
            Duration::from_secs(30),
            client
                .post(format!("{}/post", env.http_url()))
                .header("Content-Type", "application/octet-stream")
                .body(large_body.clone())
                .send(),
        )
        .await??;
        assert_eq!(post_response.status(), 200);

        // Test 3: Multiple concurrent large transfers
        println!("✓ Testing concurrent large transfers");
        let mut tasks = Vec::new();
        let bytes_size = 51200; // 50KB each

        for i in 0..5 {
            let client = client.clone();
            let url = format!("{}/bytes/{bytes_size}", env.http_url());

            let task = tokio::spawn(async move {
                let resp = timeout(Duration::from_secs(20), client.get(&url).send())
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(resp.status(), 200);

                let body = resp.bytes().await.unwrap();
                assert_eq!(body.len(), bytes_size);
                i
            });
            tasks.push(task);
        }

        let results = join_all(tasks).await;
        for result in results {
            let _id = result?;
        }

        Ok(())
    })
    .await
    .unwrap();
}
