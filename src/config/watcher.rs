use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use super::Config;

/// Spawns a config watcher task that reloads config on file changes and exits on shutdown signal.
pub fn spawn_config_watcher(
    config_path: PathBuf,
    config: Arc<RwLock<Config>>,
    connections_token: Arc<Mutex<CancellationToken>>,
    cancel_token: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let mut watcher = RecommendedWatcher::new(
            move |res| {
                let _ = tx.blocking_send(res);
            },
            notify::Config::default(),
        )
        .expect("Failed to create watcher");
        watcher
            .watch(&config_path, RecursiveMode::NonRecursive)
            .expect("Failed to watch config file");
        loop {
            tokio::select! {
                _ = cancel_token.cancelled() => {
                    info!("Config watcher received shutdown signal");
                    break;
                }
                maybe_event = rx.recv() => {
                    if let Some(Ok(event)) = maybe_event {
                        if matches!(event.kind, EventKind::Modify(_)) {
                            // Debounce: wait 200ms, drain any further events
                            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                            while let Ok(Some(Ok(ev))) = tokio::time::timeout(
                                std::time::Duration::from_millis(10),
                                rx.recv()
                            ).await {
                                if !matches!(ev.kind, EventKind::Modify(_)) {
                                    break;
                                }
                            }

                            // First load the new config
                            let new_config = match Config::load(config_path.to_str().unwrap()) {
                                Ok(cfg) => {
                                    debug!("Config loaded successfully from disk");
                                    cfg
                                },
                                Err(e) => {
                                    error!("Failed to reload config: {}. Keeping old config.", e);
                                    continue;
                                }
                            };

                            // Cancel existing connections to free up any read locks - important fix:
                            // We must not hold the MutexGuard across an await point
                            {
                                // Scope for MutexGuard to ensure it's dropped before any awaits
                                match connections_token.lock() {
                                    Ok(mut token_guard) => {
                                        debug!("Cancelling all active connections before config update");
                                        token_guard.cancel();
                                        *token_guard = CancellationToken::new();
                                        // MutexGuard is dropped at the end of this scope
                                    },
                                    Err(e) => {
                                        error!("Failed to acquire lock on connections token: {:?}", e);
                                    }
                                }
                            } // MutexGuard is definitely dropped here

                            // Now we can safely await without holding the MutexGuard
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

                            // Now try to update the config with a timeout
                            match tokio::time::timeout(
                                std::time::Duration::from_secs(3),
                                config.write()
                            ).await {
                                Ok(mut guard) => {
                                    debug!("Acquired write lock for config");
                                    *guard = new_config;
                                    info!("Config updated successfully");
                                },
                                Err(_) => {
                                    error!("Timeout while acquiring write lock for config");
                                    warn!("The new config is loaded but not applied, connections were reset anyway");

                                    // Try one more time with a shorter timeout after giving more time for locks to clear
                                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

                                    // Using a direct approach instead of try_write()
                                    match tokio::time::timeout(
                                        std::time::Duration::from_millis(500),
                                        config.write()
                                    ).await {
                                        Ok(mut guard) => {
                                            debug!("Acquired write lock for config on second attempt");
                                            *guard = new_config;
                                            info!("Config updated successfully on second attempt");
                                        },
                                        Err(_) => {
                                            error!("Timeout on second attempt to acquire write lock");
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    })
}
