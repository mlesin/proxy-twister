# Proxy Twister

A flexible HTTP proxy switcher that intelligently routes traffic through different proxies (SOCKS5 or HTTP) based on target host patterns.

## Features

- Route traffic through different proxies based on domain/IP patterns
- Support for both SOCKS5 and HTTP proxies
- Direct connection option for local or trusted networks
- Pattern matching with wildcards for flexible routing rules
- Handles both HTTP and HTTPS (via CONNECT) connections
- Hot-reloads configuration file on changes (keeps last valid config on error)
- Graceful shutdown on Ctrl-C (all tasks terminate cleanly)
- Listen on multiple addresses simultaneously

## Installation

### From crates.io (Preferred)

Install the latest release directly from crates.io:

```sh
cargo install proxy-twister
```

This will install the `proxy-twister` binary into your Cargo bin directory (usually `~/.cargo/bin`).

### From Source

1. Ensure you have Rust and Cargo installed (<https://rustup.rs/>)
2. Clone this repository:

   ```shell
   git clone https://github.com/mlesin/proxy-twister.git
   cd proxy-twister
   ```

3. Build the project:

   ```shell
   cargo build --release
   ```

4. The binary will be available at `target/release/proxy-twister`

## Configuration

Create a configuration file in JSON format. Here's an example:

```json
{
    "switch": {
        "default": "regular",
        "rules": [
            {
                "pattern": "10.*",
                "profile": "direct"
            },
            {
                "pattern": "127.0.0.1",
                "profile": "direct"
            },
            {
                "pattern": "*.discord.gg",
                "profile": "tor"
            },
            {
                "pattern": "*.discord.com",
                "profile": "tor"
            },
            {
                "pattern": "*.medium.com",
                "profile": "monkey"
            }
        ]
    },
    "profiles": {
        "direct": {
            "scheme": "direct"
        },
        "regular": {
            "scheme": "http",
            "port": 1080,
            "host": "localhost"
        },
        "tor": {
            "scheme": "socks5",
            "host": "localhost",
            "port": 9150
        },
        "monkey": {
            "scheme": "socks5",
            "host": "localhost",
            "port": 8884
        }
    }
}
```

### Configuration Explanation

- **switch**: Contains the routing rules
  - **default**: The default profile to use when no pattern matches
  - **rules**: List of pattern-matching rules to determine which proxy to use
    - **pattern**: A domain/IP pattern (supports wildcards)
    - **profile**: The profile to use when the pattern matches

- **profiles**: Defines the available proxy configurations
  - Each profile has a unique name and configuration:
    - **direct**: No proxy, direct connection
    - **http**: HTTP proxy with host and port
    - **socks5**: SOCKS5 proxy with host and port

## Usage

Run the program with:

```shell
proxy-twister --config config.json --listen 127.0.0.1:1080 --listen 127.0.0.1:8080
```

Options:

- `--config`: Path to the configuration file (required)
- `--listen`/`-l`: Address to listen on (can be specified multiple times; default: 127.0.0.1:1080)

You can specify multiple `--listen`/`-l` options to listen on several addresses/ports at once. Example:

```shell
proxy-twister --config config.json -l 127.0.0.1:1080 -l 127.0.0.1:8080
```

Then configure your applications to use the proxy at any of the addresses and ports you specified.

### Hot Reloading

- The proxy will automatically reload its configuration file when it changes.
- If the new config is invalid, the last valid config remains active and an error is logged.

### Graceful Shutdown

- Press Ctrl-C to gracefully shut down all listeners and background tasks.

## Pattern Matching

The pattern matching supports:

- Exact matches: `example.com`
- Wildcard at beginning: `*.example.com` (matches `sub.example.com`, `example.com`)
- IP prefix matching: `192.168.*` (matches any IP starting with 192.168)

## Examples

### Route specific sites through Tor

```json
{
    "pattern": "*.onion",
    "profile": "tor"
}
```

### Use direct connection for local networks

```json
{
    "pattern": "192.168.*",
    "profile": "direct"
}
```

## Development and Testing

### Prerequisites

- Docker installed (for integration tests with external proxies)
- Rust 1.80.0 or later

### Running Tests

The project includes comprehensive integration tests that validate all routing modes and edge cases:

```shell
# Run all integration tests
cargo test

# Run specific test suites
cargo test --test test_direct_routing

# Run tests with output
cargo test -- --nocapture
```

### Integration Test Coverage

The project includes comprehensive integration tests with **48 passing tests** covering all proxy types and HTTPS functionality:

**Current Status**: ✅ **49 passing tests, 0 ignored** - Complete HTTPS/TLS support implemented

The integration tests cover:

✅ **Direct HTTP routing** - Plain HTTP traffic without proxy (10 tests)  
✅ **Direct HTTPS routing** - HTTPS with self-signed certs using `mendhak/http-https-echo` (3 tests)  
✅ **HTTP proxy routing** - Dockerized tinyproxy integration (8 tests)  
✅ **HTTP proxy HTTPS** - HTTPS through HTTP proxy with SSL termination (3 tests)  
✅ **SOCKS5 proxy routing** - Dockerized Dante proxy integration (5 tests)  
✅ **SOCKS5 proxy HTTPS** - HTTPS through SOCKS5 proxy with SSL termination (3 tests)  
✅ **POST requests** - With request bodies and data integrity validation  
✅ **Large payloads** - Multi-megabyte transfers with checksum validation  
✅ **Concurrent connections** - Multiple simultaneous requests across all proxy types  
✅ **Pattern matching** - Complex host/domain routing rules  
✅ **Error handling** - Proxy unavailable, timeouts, and malformed responses  
✅ **Data integrity** - SHA-256 checksums for large transfers  
✅ **Integration scenarios** - Mixed HTTP/HTTPS traffic and advanced routing  
✅ **Advanced features** - Performance benchmarking and edge case handling  
🔴 **HTTP proxy authentication** - Basic Auth scenarios *(not implemented)*  
🔴 **DNS-only hostnames** - Currently uses IP addresses *(not implemented)*  

**Test Suite Breakdown:**
- **Direct routing**: 13 tests (including 3 HTTPS)
- **HTTP proxy**: 11 tests (including 3 HTTPS) 
- **SOCKS5 proxy**: 8 tests (including 3 HTTPS)
- **Integration**: 5 tests
- **Advanced features**: 3 tests
- **Failure scenarios**: 8 tests

**Legend:**  
✅ Implemented and tested  
🔴 Not implemented  

### Test Infrastructure

The tests use modern containerized infrastructure:

- **Containerized HTTP/HTTPS servers** using `kennethreitz/httpbin` and `mendhak/http-https-echo:31`
- **Dockerized proxy servers** (Tinyproxy for HTTP, Dante for SOCKS5)
- **Testcontainers integration** for automatic container lifecycle management
- **Self-signed certificate support** with automatic client configuration
- **Temporary configuration files** with proper cleanup
- **Isolated proxy-twister instances** per test
- **Automatic resource cleanup** on test completion

### Running Individual Tests

```shell
# Test basic direct routing
cargo test test_basic_direct_routing

# Test large payload handling
cargo test test_direct_large_payload

# Test concurrent connections
cargo test test_concurrent_connections
```

### Known Limitations

The current test suite has some remaining limitations:

- **HTTP Proxy Authentication**: No tests for Basic Auth scenarios (407 responses, credentials)  
- **DNS Resolution**: Tests use IP addresses rather than hostnames to avoid DNS complexity
- **IPv6 Support**: No IPv6-specific tests implemented yet
- **Certificate Validation**: No tests for expired/invalid certificates, SNI, or ALPN
- **WebSocket/Streaming**: Long-lived connections not specifically tested

### Contributing

Contributions are welcome! Priority areas for improvement:

1. **Add HTTP proxy authentication** - Implement Basic Auth test scenarios  
2. **DNS-only hostname testing** - Add tests using actual DNS resolution
3. **IPv6 support** - Add IPv6 destination tests
4. **Certificate validation** - Test various certificate scenarios
5. **WebSocket/Streaming support** - Add long-lived connection tests

## License
