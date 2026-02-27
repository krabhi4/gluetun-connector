use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde_json::{json, Value};

use crate::state::AppState;

pub async fn status(State(s): State<AppState>) -> impl IntoResponse {
    proxy_get(&s, "/v1/vpn/status").await
}

pub async fn publicip(State(s): State<AppState>) -> impl IntoResponse {
    proxy_get(&s, "/v1/publicip/ip").await
}

pub async fn portforwarded(State(s): State<AppState>) -> impl IntoResponse {
    proxy_get(&s, "/v1/portforward").await
}

pub async fn dns(State(s): State<AppState>) -> impl IntoResponse {
    proxy_get(&s, "/v1/dns/status").await
}

pub async fn settings(State(s): State<AppState>) -> impl IntoResponse {
    proxy_get(&s, "/v1/vpn/settings").await
}

async fn proxy_get(state: &AppState, path: &str) -> impl IntoResponse {
    match gluetun_fetch(state, path).await {
        Ok(data) => (StatusCode::OK, Json(json!({"ok": true, "data": data}))).into_response(),
        Err(_) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({"ok": false, "error": "Upstream error"})),
        )
            .into_response(),
    }
}

pub async fn gluetun_fetch(state: &AppState, path: &str) -> Result<Value, ()> {
    let url = state
        .config
        .gluetun_url
        .join(path)
        .map_err(|_| ())?;

    let mut req = state.http_client.get(url);
    for (k, v) in state.auth_headers.iter() {
        req = req.header(k, v);
    }

    let res = req.send().await.map_err(|e| {
        tracing::error!("[upstream] {e}");
    })?;

    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        tracing::error!("[upstream] Gluetun returned {status}: {}", body.chars().take(200).collect::<String>());
        return Err(());
    }

    res.json::<Value>().await.map_err(|e| {
        tracing::error!("[upstream] Failed to parse response: {e}");
    })
}
