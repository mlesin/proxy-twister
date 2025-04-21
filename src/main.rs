use clap::Parser;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::info;

mod config;
mod protocols;
mod server;
mod utils;

use config::Config;
use config::watcher::spawn_config_watcher;

/// SOCKS5 proxy switcher that routes traffic based on target host patterns
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Config file path
    #[arg(short, long)]
    config: String,

    /// Addresses to listen on (can be specified multiple times)
    #[arg(short = 'l', long = "listen", default_value = "127.0.0.1:1080")]
    addresses: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let args = Args::parse();
    let config_path = args.config.clone();
    let config = Arc::new(RwLock::new(match Config::load(&config_path) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("Configuration error: {}", e);
            std::process::exit(1);
        }
    }));

    // Use a shared holder for the current CancellationToken
    let connections_token = Arc::new(Mutex::new(CancellationToken::new()));
    let watcher_token = CancellationToken::new(); // Separate token for graceful shutdown
    let watcher_handle = spawn_config_watcher(
        PathBuf::from(config_path.clone()),
        config.clone(),
        connections_token.clone(),
        watcher_token.clone(),
    );

    let mut join_handles = vec![watcher_handle];
    for addr in &args.addresses {
        let config = config.clone();
        let token = connections_token.clone();
        let shutdown_token = watcher_token.clone();
        let addr = addr.clone();
        join_handles.push(tokio::spawn(async move {
            server::run_listener(addr, config, token, shutdown_token).await;
        }));
    }

    tokio::signal::ctrl_c()
        .await
        .expect("Failed to listen for Ctrl-C");
    info!("Ctrl-C received, shutting down...");

    // Cancel the watcher first to stop config reloading
    watcher_token.cancel();
    // Cancel all connections
    connections_token.lock().unwrap().cancel();

    for handle in join_handles {
        let _ = handle.await;
    }
    Ok(())
}
