use std::{collections::HashMap, sync::Arc};

use bollard::Docker;
use futures_util::future::join_all;
use tokio::sync::RwLock;

use crate::{config::Config, state::MonitorState};

use super::{
    docker::{find_container, resolve_dependents, restart_containers, restart_gluetun, test_site},
    sites::load_sites,
};

pub async fn perform_checks(config: &Config, state: &Arc<RwLock<MonitorState>>, docker: &Docker) {
    let container_id = match find_container(docker, &config.gluetun_container).await {
        Some(id) => id,
        None => {
            tracing::error!("[MONITOR] Container {} not found!", config.gluetun_container);
            return;
        }
    };

    let sites = load_sites(&config.config_file);
    if sites.is_empty() {
        return;
    }

    let timeout_secs = config.request_timeout.as_secs();
    let results =
        join_all(sites.iter().map(|site| test_site(docker, &container_id, site, timeout_secs)))
            .await;

    let mut any_exceeded = false;
    {
        let mut st = state.write().await;
        for r in &results {
            use super::docker::SiteStatus;
            match r.status {
                SiteStatus::Pass => {
                    st.site_failures.insert(r.site.clone(), 0);
                }
                SiteStatus::Fail => {
                    let count = st.site_failures.entry(r.site.clone()).or_insert(0);
                    *count += 1;
                    let failures = *count;
                    if failures >= config.fail_threshold {
                        any_exceeded = true;
                        tracing::warn!(
                            "[MONITOR] Site {} FAILED {}/{} times (THRESHOLD REACHED) — {}",
                            r.site,
                            failures,
                            config.fail_threshold,
                            r.reason.as_deref().unwrap_or("unknown")
                        );
                    } else {
                        tracing::warn!(
                            "[MONITOR] Site {} FAILED ({}/{}) — {}",
                            r.site,
                            failures,
                            config.fail_threshold,
                            r.reason.as_deref().unwrap_or("unknown")
                        );
                    }
                }
            }
        }
    }

    if any_exceeded {
        tracing::warn!("[MONITOR] Threshold reached! Initiating recovery...");
        handle_failure(config, state, docker).await;
    }
}

async fn handle_failure(config: &Config, state: &Arc<RwLock<MonitorState>>, docker: &Docker) {
    tracing::warn!("[MONITOR] Health check failed, initiating recovery...");
    let success =
        restart_gluetun(docker, &config.gluetun_container, config.healthy_wait_timeout).await;
    if success {
        tracing::info!("[MONITOR] Restarting dependent containers...");
        let dependents =
            resolve_dependents(docker, &config.gluetun_container, &config.dependent_containers)
                .await;
        restart_containers(docker, &dependents).await;
        state.write().await.site_failures = HashMap::new();
        tracing::info!("[MONITOR] Recovery complete");
    } else {
        tracing::error!("[MONITOR] Recovery failed — manual intervention may be required");
    }
}
