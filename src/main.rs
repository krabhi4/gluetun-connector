mod auth;
mod config;
mod middleware;
mod monitor;
mod routes;
mod state;

use std::{net::SocketAddr, sync::Arc, time::Duration};

use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::info;

use auth::build_auth_headers;
use config::Config;
use state::{AppState, MonitorState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Check for --health-check subcommand
    if std::env::args().any(|a| a == "--health-check") {
        return health_check().await;
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let config = Arc::new(Config::load());
    info!("Starting gluetun-connector on port {}", config.port);
    info!("Proxying to Gluetun at: {}", config.gluetun_url);

    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;

    let docker = bollard::Docker::connect_with_defaults()?;

    let auth_headers = Arc::new(build_auth_headers(&config));

    let monitor_state = Arc::new(RwLock::new(MonitorState {
        is_monitoring: false,
        check_interval_secs: config.check_interval.as_secs(),
        dependent_containers: match &config.dependent_containers {
            config::DependentContainers::Auto => "auto".to_string(),
            config::DependentContainers::Explicit(v) => v.join(", "),
        },
        site_failures: Default::default(),
    }));

    let state = AppState {
        config: Arc::clone(&config),
        http_client,
        docker,
        auth_headers,
        monitor_state: Arc::clone(&monitor_state),
    };

    let monitor_token = monitor::spawn_monitor(
        Arc::clone(&config),
        Arc::clone(&monitor_state),
        state.docker.clone(),
        state.http_client.clone(),
        Arc::clone(&state.auth_headers),
    );

    let app = routes::build_router(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!("Listening on {addr}");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal(monitor_token))
    .await?;

    Ok(())
}

async fn shutdown_signal(monitor_token: CancellationToken) {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }

    info!("Shutdown signal received, stopping monitor...");
    monitor_token.cancel();
}

/// --health-check: TCP connect to localhost:PORT, exit 0/1.
async fn health_check() -> anyhow::Result<()> {
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3000);
    match tokio::net::TcpStream::connect(format!("127.0.0.1:{port}")).await {
        Ok(_) => std::process::exit(0),
        Err(_) => std::process::exit(1),
    }
}
