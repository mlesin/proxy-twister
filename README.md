# Proxy Twister

A flexible HTTP proxy switcher that intelligently routes traffic through different proxies (SOCKS5 or HTTP) based on target host patterns.

## Features

- Route traffic through different proxies based on domain/IP patterns
- Support for both SOCKS5 and HTTP proxies
- Direct connection option for local or trusted networks
- Pattern matching with wildcards for flexible routing rules
- Handles both HTTP and HTTPS (via CONNECT) connections

## Installation

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
proxy-twister --config config.json [--address 127.0.0.1] [--port 1080]
```

Options:

- `--config`: Path to the configuration file (required)
- `--address`: Address to listen on (default: 127.0.0.1)
- `--port`: Port to listen on (default: 1080)

Then configure your applications to use the proxy at the address and port you specified.

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
