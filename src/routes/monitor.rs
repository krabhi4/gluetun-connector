use axum::{extract::State, response::IntoResponse, Json};
use serde_json::json;

use crate::state::AppState;

pub async fn monitor_status(State(s): State<AppState>) -> impl IntoResponse {
    let state = s.monitor_state.read().await;
    Json(json!({
        "ok": true,
        "data": {
            "isMonitoring": state.is_monitoring,
            "checkInterval": state.check_interval_secs,
            "dependentContainers": state.dependent_containers,
            "siteFailures": state.site_failures,
        }
    }))
}
