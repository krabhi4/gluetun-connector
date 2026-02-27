pub mod health;
pub mod monitor;
pub mod proxy;
pub mod vpn;

use std::{net::{IpAddr, SocketAddr}, num::NonZeroU32, sync::Arc, time::Duration};

use axum::{
    extract::{ConnectInfo, Request},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, put},
    Extension, Json, Router,
};
use governor::{DefaultKeyedRateLimiter, Quota, RateLimiter};
use serde_json::json;
use tower_http::services::{ServeDir, ServeFile};

use crate::{middleware as sec_headers, state::AppState};

type Limiter = DefaultKeyedRateLimiter<IpAddr>;

fn make_limiter(period: Duration, burst: u32) -> Arc<Limiter> {
    let quota = Quota::with_period(period)
        .unwrap()
        .allow_burst(NonZeroU32::new(burst).unwrap());
    Arc::new(RateLimiter::keyed(quota))
}

async fn rate_limit(
    Extension(limiter): Extension<Arc<Limiter>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Response {
    if limiter.check_key(&addr.ip()).is_ok() {
        next.run(req).await
    } else {
        (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({"ok": false, "error": "Too many requests, please try again later."})),
        )
            .into_response()
    }
}

pub fn build_router(state: AppState) -> Router {
    // JS equivalents: 120/60s, 10/60s, 100/900s
    let read_lim: Arc<Limiter> = make_limiter(Duration::from_millis(500), 120);
    let vpn_lim: Arc<Limiter> = make_limiter(Duration::from_secs(6), 10);
    let static_lim: Arc<Limiter> = make_limiter(Duration::from_secs(9), 100);

    let api_read = Router::new()
        .route("/ping", get(health::ping))
        .route("/health", get(health::health))
        .route("/status", get(proxy::status))
        .route("/publicip", get(proxy::publicip))
        .route("/portforwarded", get(proxy::portforwarded))
        .route("/dns", get(proxy::dns))
        .route("/settings", get(proxy::settings))
        .route("/monitor", get(monitor::monitor_status))
        .fallback(api_not_found)
        .layer(middleware::from_fn(rate_limit))
        .layer(Extension(read_lim));

    let api_vpn = Router::new()
        .route("/vpn/{action}", put(vpn::vpn_action))
        .layer(middleware::from_fn(rate_limit))
        .layer(Extension(vpn_lim));

    let static_svc = ServeDir::new("public").fallback(ServeFile::new("public/index.html"));

    // Static files with rate limiting via a wrapping closure at the fallback level.
    // We use a separate inner router to apply the limit as middleware.
    let static_router = Router::new()
        .fallback_service(static_svc)
        .layer(middleware::from_fn(rate_limit))
        .layer(Extension(static_lim));

    Router::new()
        .nest("/api", Router::new().merge(api_read).merge(api_vpn))
        .fallback_service(static_router)
        .layer(sec_headers::permissions_policy())
        .layer(sec_headers::referrer_policy())
        .layer(sec_headers::x_frame_options())
        .layer(sec_headers::x_content_type_options())
        .layer(sec_headers::content_security_policy())
        .with_state(state)
}

async fn api_not_found() -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        Json(json!({"ok": false, "error": "Not found"})),
    )
}
