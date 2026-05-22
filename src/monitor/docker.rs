use std::time::{Duration, Instant};

use bollard::{
    container::LogOutput,
    exec::{CreateExecOptions, StartExecResults},
    models::HealthStatusEnum,
    Docker,
};
use futures_util::StreamExt;

use crate::config::DependentContainers;

#[derive(Debug)]
pub struct SiteResult {
    pub site: String,
    pub status: SiteStatus,
    pub reason: Option<String>,
}

#[derive(Debug, PartialEq)]
pub enum SiteStatus {
    Pass,
    Fail,
}

impl SiteResult {
    fn pass(site: &str, reason: Option<String>) -> Self {
        Self { site: site.to_string(), status: SiteStatus::Pass, reason }
    }
    fn fail(site: &str, reason: impl Into<String>) -> Self {
        Self { site: site.to_string(), status: SiteStatus::Fail, reason: Some(reason.into()) }
    }
}

/// Run `wget --spider` inside the Gluetun container to test connectivity to `site`.
pub async fn test_site(docker: &Docker, container_id: &str, site: &str, timeout_secs: u64) -> SiteResult {
    let start = Instant::now();

    // Commands are passed as Vec<String> — exec form, no shell, no injection risk
    let cmd = vec![
        "wget".to_string(),
        "--spider".to_string(),
        "-S".to_string(),
        format!("--timeout={timeout_secs}"),
        "--tries=1".to_string(),
        "-q".to_string(),
        site.to_string(),
    ];

    let exec_result = docker
        .create_exec(
            container_id,
            CreateExecOptions {
                cmd: Some(cmd),
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                ..Default::default()
            },
        )
        .await;

    let exec_id = match exec_result {
        Ok(e) => e.id,
        Err(e) => {
            tracing::error!("[MONITOR] Failed to create exec for {site}: {e}");
            return SiteResult::fail(site, "Failed to create exec instance");
        }
    };

    // Start exec and collect output (stdout + stderr muxed)
    let output_result = docker.start_exec(&exec_id, None).await;

    let mut output_buf = String::new();
    match output_result {
        Ok(StartExecResults::Attached { mut output, .. }) => {
            while let Some(chunk) = output.next().await {
                match chunk {
                    Ok(LogOutput::StdOut { message }) | Ok(LogOutput::StdErr { message }) => {
                        output_buf.push_str(&String::from_utf8_lossy(&message));
                    }
                    _ => {}
                }
            }
        }
        Ok(StartExecResults::Detached) => {}
        Err(e) => {
            tracing::error!("[MONITOR] Failed to start exec for {site}: {e}");
            return SiteResult::fail(site, "Failed to start exec command");
        }
    }

    // Inspect for exit code AFTER stream is fully drained
    let exit_code = match docker.inspect_exec(&exec_id).await {
        Ok(info) => info.exit_code.unwrap_or(-1),
        Err(_) => -1,
    };

    let http_code = output_buf.find("HTTP/").and_then(|pos| {
        output_buf[pos..].split_whitespace().nth(1).map(|s| s.to_string())
    });

    let elapsed = start.elapsed();
    match exit_code {
        0 => {
            tracing::info!("[MONITOR] Site {site} PASS ({}ms)", elapsed.as_millis());
            SiteResult::pass(site, None)
        }
        6 | 8 => {
            let reason = format!("HTTP {} (VPN working)", http_code.as_deref().unwrap_or("?"));
            tracing::info!("[MONITOR] Site {site} PASS — {reason}");
            SiteResult::pass(site, Some(reason))
        }
        4 => SiteResult::fail(site, "Network failure (DNS or connection)"),
        5 => SiteResult::fail(site, "SSL verification failure"),
        code => SiteResult::fail(site, format!("wget exited with code {code}")),
    }
}

/// Get the container ID for the Gluetun container by name.
pub async fn find_container(docker: &Docker, name: &str) -> Option<String> {
    docker
        .list_containers::<String>(Some(bollard::container::ListContainersOptions {
            all: true,
            ..Default::default()
        }))
        .await
        .ok()?
        .into_iter()
        .find(|c| {
            c.names.as_ref().map_or(false, |ns| ns.iter().any(|n| n == &format!("/{name}")))
        })
        .and_then(|c| c.id)
}

/// Poll until the Gluetun container reports healthy (or running if no HEALTHCHECK), or timeout expires.
pub async fn wait_for_healthy(docker: &Docker, container_name: &str, timeout: Duration) -> bool {
    tracing::info!("[MONITOR] Waiting for {container_name} to become healthy...");
    let result = tokio::time::timeout(timeout, async {
        loop {
            if let Ok(info) = docker.inspect_container(container_name, None).await {
                let health_status = info
                    .state
                    .as_ref()
                    .and_then(|s| s.health.as_ref())
                    .and_then(|h| h.status.as_ref());
                match health_status {
                    Some(&HealthStatusEnum::HEALTHY) => {
                        tracing::info!("[MONITOR] {container_name} is healthy");
                        return;
                    }
                    None => {
                        // No HEALTHCHECK configured — fall back to checking running state
                        let running = info
                            .state
                            .as_ref()
                            .and_then(|s| s.running)
                            .unwrap_or(false);
                        if running {
                            tracing::info!(
                                "[MONITOR] {container_name} has no HEALTHCHECK but is running"
                            );
                            return;
                        }
                    }
                    _ => {}
                }
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    })
    .await;

    if result.is_err() {
        tracing::error!(
            "[MONITOR] {container_name} did not become healthy within {}s",
            timeout.as_secs()
        );
        false
    } else {
        true
    }
}

/// Restart the Gluetun container and wait for it to become healthy.
pub async fn restart_gluetun(docker: &Docker, container_name: &str, healthy_wait: Duration) -> bool {
    tracing::warn!("[MONITOR] Restarting {container_name} to force new endpoint...");
    if let Err(e) = docker.restart_container(container_name, None).await {
        tracing::error!("[MONITOR] Failed to restart {container_name}: {e}");
        return false;
    }
    let healthy = wait_for_healthy(docker, container_name, healthy_wait).await;
    if healthy {
        tracing::info!("[MONITOR] Waiting 10s for DNS to stabilize...");
        tokio::time::sleep(Duration::from_secs(10)).await;
    }
    healthy
}

/// Discover containers that use Gluetun as their network (`network_mode: container:<id|name>`).
pub async fn discover_dependents(docker: &Docker, gluetun_name: &str) -> Vec<String> {
    // Use all:true to include stopped containers — dependents may have stopped because
    // gluetun went down, and we need to restart them even if they're not running.
    let all = match docker.list_containers::<String>(Some(bollard::container::ListContainersOptions {
        all: true,
        ..Default::default()
    })).await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("[MONITOR] Failed to list containers: {e}");
            return vec![];
        }
    };

    // Extract Gluetun ID from already-fetched list (avoids a second list_containers call)
    let gluetun_id = all.iter().find(|c| {
        c.names.as_ref().map_or(false, |ns| ns.iter().any(|n| n == &format!("/{gluetun_name}")))
    }).and_then(|c| c.id.clone());
    let short_id = gluetun_id.as_deref().map(|id| id[..id.len().min(12)].to_string());

    let mut dependents = vec![];
    for c in all {
        let name = c.names.as_ref()
            .and_then(|ns| ns.first())
            .map(|n| n.trim_start_matches('/').to_string())
            .unwrap_or_default();
        if name == gluetun_name { continue; }

        let network_mode = c.host_config
            .as_ref()
            .and_then(|h| h.network_mode.as_deref())
            .unwrap_or("");

        let is_dep = network_mode == format!("container:{gluetun_name}")
            || gluetun_id.as_deref().map_or(false, |id| network_mode == format!("container:{id}"))
            || short_id.as_deref().map_or(false, |sid| network_mode.starts_with(&format!("container:{sid}")));

        if is_dep { dependents.push(name); }
    }
    dependents
}

/// Restart all containers in `names`, pausing 2s between each.
pub async fn restart_containers(docker: &Docker, names: &[String]) {
    if names.is_empty() {
        tracing::warn!("[MONITOR] No dependent containers to restart");
        return;
    }
    for name in names {
        tracing::info!("[MONITOR] Restarting {name}...");
        match docker.restart_container(name, None).await {
            Ok(_) => tracing::info!("[MONITOR] {name} restarted successfully"),
            Err(e) => tracing::error!("[MONITOR] Failed to restart {name}: {e}"),
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    tracing::info!("[MONITOR] Dependent container restart complete");
}

/// Resolve the list of dependent containers to restart, based on config.
pub async fn resolve_dependents(
    docker: &Docker,
    gluetun_name: &str,
    dep_config: &DependentContainers,
) -> Vec<String> {
    match dep_config {
        DependentContainers::Auto => discover_dependents(docker, gluetun_name).await,
        DependentContainers::Explicit(list) => list.clone(),
    }
}

/// Check if any dependent container is running but out of sync with gluetun's network namespace (StartedAt mismatch).
/// If so, restart them.
pub async fn check_and_heal_namespaces(
    docker: &Docker,
    gluetun_name: &str,
    dep_config: &DependentContainers,
) -> anyhow::Result<()> {
    // 1. Inspect gluetun to get its started_at time.
    let gluetun_info = match docker.inspect_container(gluetun_name, None).await {
        Ok(info) => info,
        Err(e) => {
            tracing::debug!("[MONITOR] Could not inspect gluetun container {gluetun_name}: {e}");
            return Ok(());
        }
    };

    let gluetun_state = match gluetun_info.state {
        Some(s) => s,
        None => return Ok(()),
    };

    let gluetun_running = gluetun_state.running.unwrap_or(false);
    if !gluetun_running {
        tracing::debug!("[MONITOR] Gluetun is not running, skipping namespace healing");
        return Ok(());
    }

    let gluetun_started_str = match gluetun_state.started_at {
        Some(s) => s,
        None => return Ok(()),
    };

    let gluetun_started = match chrono::DateTime::parse_from_rfc3339(&gluetun_started_str) {
        Ok(dt) => dt,
        Err(e) => {
            tracing::error!("[MONITOR] Failed to parse gluetun started_at timestamp '{}': {e}", gluetun_started_str);
            return Ok(());
        }
    };

    // 2. Get list of dependents
    let dependents = resolve_dependents(docker, gluetun_name, dep_config).await;
    if dependents.is_empty() {
        return Ok(());
    }

    let mut out_of_sync = Vec::new();

    // 3. For each dependent, inspect and compare StartedAt
    for dep in dependents {
        if let Ok(info) = docker.inspect_container(&dep, None).await {
            if let Some(state) = info.state {
                let running = state.running.unwrap_or(false);
                if running {
                    if let Some(started_str) = state.started_at {
                        if let Ok(started_dt) = chrono::DateTime::parse_from_rfc3339(&started_str) {
                            if started_dt < gluetun_started {
                                tracing::warn!(
                                    "[MONITOR] Dependent container {} is out of sync (started {} < gluetun {})",
                                    dep,
                                    started_str,
                                    gluetun_started_str
                                );
                                out_of_sync.push(dep);
                            }
                        }
                    }
                }
            }
        }
    }

    // 4. Restart out-of-sync dependents
    if !out_of_sync.is_empty() {
        tracing::warn!(
            "[MONITOR] Found {} out-of-sync dependent containers. Restarting them...",
            out_of_sync.len()
        );
        restart_containers(docker, &out_of_sync).await;
    }

    Ok(())
}
