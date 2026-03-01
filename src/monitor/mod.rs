pub mod checker;
pub mod docker;
pub mod sites;

use std::sync::Arc;

use bollard::Docker;
use reqwest::header::HeaderMap;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use crate::{config::Config, state::MonitorState};

/// Spawn the background monitor task. Returns a token — call `.cancel()` to stop it.
pub fn spawn_monitor(
    config: Arc<Config>,
    state: Arc<RwLock<MonitorState>>,
    docker: Docker,
    http_client: reqwest::Client,
    auth_headers: Arc<HeaderMap>,
) -> CancellationToken {
    let token = CancellationToken::new();
    let child = token.child_token();

    // Log initial auto-discovered dependents on startup
    {
        let config2 = Arc::clone(&config);
        let docker2 = docker.clone();
        tokio::spawn(async move {
            if matches!(config2.dependent_containers, crate::config::DependentContainers::Auto) {
                let deps =
                    docker::discover_dependents(&docker2, &config2.gluetun_container).await;
                tracing::info!(
                    "[MONITOR] Initial dependent containers (auto-discovery): {}",
                    if deps.is_empty() { "(none found)".to_string() } else { deps.join(", ") }
                );
            }
        });
    }

    tokio::spawn(async move {
        // Mark monitoring active
        state.write().await.is_monitoring = true;
        tracing::info!(
            "[MONITOR] Starting monitor for {} with interval {}s",
            config.gluetun_container,
            config.check_interval.as_secs()
        );

        let mut interval = tokio::time::interval(config.check_interval);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                biased;
                _ = child.cancelled() => {
                    tracing::info!("[MONITOR] Stopped monitor");
                    break;
                }
                _ = interval.tick() => {
                    checker::perform_checks(&config, &state, &docker, &http_client, &auth_headers).await;
                }
            }
        }
        state.write().await.is_monitoring = false;
    });

    token
}
