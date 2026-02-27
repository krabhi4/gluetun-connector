use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde_json::{json, Value};

use crate::state::AppState;

pub async fn vpn_action(
    State(s): State<AppState>,
    Path(action): Path<String>,
) -> impl IntoResponse {
    let status_value = match action.as_str() {
        "start" => "running",
        "stop" => "stopped",
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"ok": false, "error": "Invalid action. Use start or stop."})),
            )
                .into_response();
        }
    };

    let url = match s.config.gluetun_url.join("/v1/vpn/status") {
        Ok(u) => u,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"ok": false, "error": "Internal server error"})),
            )
                .into_response();
        }
    };

    let mut req = s
        .http_client
        .put(url)
        .json(&json!({"status": status_value}));

    for (k, v) in s.auth_headers.iter() {
        req = req.header(k, v);
    }

    match req.send().await {
        Ok(res) if res.status().is_success() => {
            let data: Value = res.json().await.unwrap_or(Value::Null);
            (StatusCode::OK, Json(json!({"ok": true, "data": data}))).into_response()
        }
        Ok(res) => {
            tracing::error!("[upstream] Gluetun returned {}", res.status());
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({"ok": false, "error": "Upstream error"})),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!("[upstream] {e}");
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({"ok": false, "error": "Upstream error"})),
            )
                .into_response()
        }
    }
}
