use std::{path::PathBuf, time::Duration};
use url::Url;

#[derive(Debug, Clone)]
pub enum DependentContainers {
    Auto,
    Explicit(Vec<String>),
}

#[derive(Debug, Clone)]
pub struct Config {
    pub port: u16,
    pub gluetun_url: Url,
    pub gluetun_api_key: Option<String>,
    pub gluetun_user: Option<String>,
    pub gluetun_password: Option<String>,
    // Monitor
    pub check_interval: Duration,
    pub request_timeout: Duration,
    pub fail_threshold: u32,
    pub gluetun_container: String,
    pub dependent_containers: DependentContainers,
    pub healthy_wait_timeout: Duration,
    pub config_file: PathBuf,
}

impl Config {
    pub fn load() -> Self {
        let raw_url = std::env::var("GLUETUN_CONTROL_URL")
            .unwrap_or_else(|_| "http://gluetun:8000".to_string());

        let gluetun_url = Url::parse(&raw_url).unwrap_or_else(|e| {
            eprintln!("FATAL: GLUETUN_CONTROL_URL is not a valid URL: {e}");
            std::process::exit(1);
        });

        let port = std::env::var("PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3000);

        let gluetun_api_key = std::env::var("GLUETUN_API_KEY")
            .ok()
            .filter(|s| !s.is_empty());
        let gluetun_user = std::env::var("GLUETUN_USER")
            .ok()
            .filter(|s| !s.is_empty());
        let gluetun_password = std::env::var("GLUETUN_PASSWORD")
            .ok()
            .filter(|s| !s.is_empty());

        let check_interval = Duration::from_secs(
            std::env::var("CHECK_INTERVAL")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(30),
        );
        let request_timeout = Duration::from_secs(
            std::env::var("TIMEOUT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(10),
        );
        let fail_threshold = std::env::var("FAIL_THRESHOLD")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(2);
        let gluetun_container = std::env::var("GLUETUN_CONTAINER")
            .unwrap_or_else(|_| "gluetun".to_string());
        let dependent_containers =
            match std::env::var("DEPENDENT_CONTAINERS").unwrap_or_else(|_| "auto".to_string()) {
                s if s.eq_ignore_ascii_case("auto") => DependentContainers::Auto,
                s => DependentContainers::Explicit(
                    s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect(),
                ),
            };
        let healthy_wait_timeout = Duration::from_secs(
            std::env::var("HEALTHY_WAIT_TIMEOUT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(120),
        );
        let config_file = PathBuf::from(
            std::env::var("CONFIG_FILE").unwrap_or_else(|_| "/config/sites.conf".to_string()),
        );

        Config {
            port,
            gluetun_url,
            gluetun_api_key,
            gluetun_user,
            gluetun_password,
            check_interval,
            request_timeout,
            fail_threshold,
            gluetun_container,
            dependent_containers,
            healthy_wait_timeout,
            config_file,
        }
    }
}
