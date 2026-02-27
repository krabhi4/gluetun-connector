use base64::{engine::general_purpose::STANDARD, Engine};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};

use crate::config::Config;

pub fn build_auth_headers(config: &Config) -> HeaderMap {
    let mut headers = HeaderMap::new();
    if let Some(key) = &config.gluetun_api_key {
        match HeaderValue::from_str(key) {
            Ok(val) => { headers.insert("x-api-key", val); }
            Err(e) => tracing::warn!("[AUTH] GLUETUN_API_KEY contains invalid header characters, skipping: {e}"),
        }
    } else if let (Some(user), Some(pass)) = (&config.gluetun_user, &config.gluetun_password) {
        let encoded = STANDARD.encode(format!("{user}:{pass}"));
        match HeaderValue::from_str(&format!("Basic {encoded}")) {
            Ok(val) => { headers.insert(AUTHORIZATION, val); }
            Err(e) => tracing::warn!("[AUTH] Basic auth header value is invalid, skipping: {e}"),
        }
    }
    headers
}
