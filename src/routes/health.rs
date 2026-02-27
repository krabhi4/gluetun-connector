use axum::{extract::State, response::IntoResponse, Json};
use chrono::Utc;
use serde_json::{json, Value};

use crate::state::AppState;

use super::proxy::gluetun_fetch;

pub async fn ping() -> impl IntoResponse {
    Json(json!({"ok": true}))
}

pub async fn health(State(s): State<AppState>) -> impl IntoResponse {
    let (vpn_status, public_ip, port_forwarded, dns_status, vpn_settings) = tokio::join!(
        gluetun_fetch(&s, "/v1/vpn/status"),
        gluetun_fetch(&s, "/v1/publicip/ip"),
        gluetun_fetch(&s, "/v1/portforward"),
        gluetun_fetch(&s, "/v1/dns/status"),
        gluetun_fetch(&s, "/v1/vpn/settings"),
    );

    Json(json!({
        "timestamp": Utc::now().to_rfc3339(),
        "vpnStatus":     to_result(vpn_status),
        "publicIp":      to_result(public_ip),
        "portForwarded": to_result(port_forwarded),
        "dnsStatus":     to_result(dns_status),
        "vpnSettings":   to_result(vpn_settings),
    }))
}

fn to_result(r: Result<Value, ()>) -> Value {
    match r {
        Ok(data) => json!({"ok": true, "data": data}),
        Err(_) => json!({"ok": false, "error": "Upstream error"}),
    }
}
