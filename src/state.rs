use std::{collections::HashMap, sync::Arc};
use reqwest::header::HeaderMap;
use tokio::sync::RwLock;

use crate::config::Config;

#[derive(Debug, Default)]
pub struct MonitorState {
    pub is_monitoring: bool,
    pub check_interval_secs: u64,
    pub dependent_containers: String,
    pub site_failures: HashMap<String, u32>,
}

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub http_client: reqwest::Client,
    pub docker: bollard::Docker,
    pub auth_headers: Arc<HeaderMap>,
    pub monitor_state: Arc<RwLock<MonitorState>>,
}
