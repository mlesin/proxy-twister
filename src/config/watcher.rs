use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use super::Config;

/// Spawns a config watcher task that reloads config on file changes and exits on shutdown signal.
pub fn spawn_config_watcher(
    config_path: PathBuf,
    config: Arc<RwLock<Config>>,
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
                            match Config::load(config_path.to_str().unwrap()) {
                                Ok(new_config) => {
                                    info!("Config reloaded successfully");
                                    let mut guard = config.write().await;
                                    *guard = new_config;
                                }
                                Err(e) => {
                                    error!("Failed to reload config: {}. Keeping old config.", e);
                                }
                            }
                        }
                    }
                }
            }
        }
    })
}
