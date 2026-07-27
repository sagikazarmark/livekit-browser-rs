use std::env;

use dioxus::prelude::*;
use dioxus::server::axum::{
    Json,
    http::StatusCode,
    response::Html,
    routing::{get, post},
};

use crate::token::{TokenRequest, TokenResponse, TokenService, unix_now};

pub fn launch() {
    dioxus::serve(|| async move {
        Ok(dioxus::server::router(ServerApp)
            .route("/healthz", get(|| async { "ok" }))
            .route("/api/token", post(issue_token))
            .fallback(get(index)))
    });
}

#[component]
fn ServerApp() -> Element {
    rsx! {}
}

async fn index() -> Result<Html<String>, StatusCode> {
    let public_path = env::var_os("DIOXUS_PUBLIC_PATH")
        .map(Into::into)
        .or_else(|| {
            env::current_exe()
                .ok()?
                .parent()
                .map(|parent| parent.join("public"))
        })
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    std::fs::read_to_string(public_path.join("index.html"))
        .map(Html)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn issue_token(
    Json(request): Json<TokenRequest>,
) -> Result<Json<TokenResponse>, (StatusCode, String)> {
    token_service()
        .issue(&request, unix_now())
        .map(Json)
        .map_err(|error| {
            let status =
                StatusCode::from_u16(error.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            (status, error.to_string())
        })
}

fn token_service() -> TokenService {
    TokenService {
        api_key: env::var("LIVEKIT_API_KEY").unwrap_or_else(|_| "devkey".into()),
        api_secret: env::var("LIVEKIT_API_SECRET").unwrap_or_else(|_| "secret".into()),
        server_url: env::var("LIVEKIT_URL")
            .ok()
            .map(|server_url| server_url.trim().to_owned())
            .filter(|server_url| !server_url.is_empty()),
        non_publishers: env::var("LIVEKIT_NON_PUBLISHER_IDENTITIES").ok(),
        non_data_publishers: env::var("LIVEKIT_NON_DATA_PUBLISHER_IDENTITIES").ok(),
        non_metadata_updaters: env::var("LIVEKIT_NON_METADATA_UPDATER_IDENTITIES").ok(),
    }
}
