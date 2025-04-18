use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{error, info};

use super::Config;

pub fn spawn_config_watcher(config_path: PathBuf, config: Arc<RwLock<Config>>) {
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
            if let Some(Ok(event)) = rx.recv().await {
                if matches!(event.kind, EventKind::Modify(_)) {
                    tokio::time::sleep(Duration::from_millis(100)).await;
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
    });
}
