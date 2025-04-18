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
