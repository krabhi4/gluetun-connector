use std::{collections::HashMap, sync::Arc};

use bollard::Docker;
use futures_util::future::join_all;
use reqwest::header::HeaderMap;
use serde_json::Value;
use tokio::sync::RwLock;

use crate::{config::Config, state::MonitorState};

use super::{
    docker::{check_and_heal_namespaces, find_container, resolve_dependents, restart_containers, restart_gluetun, test_site},
    sites::load_sites,
};

/// Check if the VPN tunnel is actually established by querying gluetun's public IP endpoint.
/// Returns true only when gluetun responds with a non-empty IP address, confirming the tunnel is up.
async fn is_tunnel_up(config: &Config, http_client: &reqwest::Client, auth_headers: &HeaderMap) -> bool {
    let url = match config.gluetun_url.join("/v1/publicip/ip") {
        Ok(u) => u,
        Err(_) => return false,
    };
    let mut req = http_client.get(url);
    for (k, v) in auth_headers.iter() {
        req = req.header(k, v);
    }
    match req.send().await {
        Ok(res) if res.status().is_success() => {
            let body = res.json::<Value>().await.unwrap_or_default();
            let ip = body.get("public_ip")
                .or_else(|| body.get("ip"))
                .or_else(|| body.get("IP"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            !ip.is_empty()
        }
        _ => false,
    }
}

pub async fn perform_checks(
    config: &Config,
    state: &Arc<RwLock<MonitorState>>,
    docker: &Docker,
    http_client: &reqwest::Client,
    auth_headers: &HeaderMap,
) {
    // Check and heal namespaces first, so we don't run tests on dead namespaces.
    if let Err(e) = check_and_heal_namespaces(docker, &config.gluetun_container, &config.dependent_containers).await {
        tracing::error!("[MONITOR] Failed to check/heal namespaces: {e}");
    }

    // Pre-flight: only run connectivity tests when the VPN tunnel is confirmed up.
    // If gluetun is still connecting (no public IP yet), skip — don't count as failure.
    // This prevents the monitor from restarting gluetun during its own connection phase,
    // which would cause an infinite restart loop.
    if !is_tunnel_up(config, http_client, auth_headers).await {
        tracing::info!("[MONITOR] VPN tunnel not yet established — skipping connectivity checks this interval");
        return;
    }

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

    // Always reset failure counters after a recovery attempt — success or failure.
    // Without this, counters stay at/above the threshold and trigger recovery on every
    // subsequent check indefinitely until gluetun eventually comes back.
    state.write().await.site_failures = HashMap::new();

    // Even if gluetun didn't become healthy within the timeout, it was still restarted,
    // which means the dependents are now using a dead network namespace.
    // We MUST restart the dependents so they connect to the new namespace.
    tracing::info!("[MONITOR] Restarting dependent containers to sync network namespaces...");
    let dependents =
        resolve_dependents(docker, &config.gluetun_container, &config.dependent_containers)
            .await;
    restart_containers(docker, &dependents).await;

    if success {
        tracing::info!("[MONITOR] Recovery complete");
    } else {
        tracing::warn!("[MONITOR] Recovery partially complete (gluetun restarted but not yet healthy)");
    }
}
