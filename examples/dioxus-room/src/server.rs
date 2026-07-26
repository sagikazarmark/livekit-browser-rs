use std::env;

use dioxus::prelude::*;
use dioxus::server::axum::{
    http::StatusCode,
    response::Html,
    routing::{get, post},
    Json,
};
use livekit_api::access_token::{AccessToken, VideoGrants};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct TokenRequest {
    room: String,
    identity: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TokenResponse {
    participant_token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    server_url: Option<String>,
}

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
    let room = request.room.trim();
    let identity = request.identity.trim();
    if room.is_empty() || identity.is_empty() || room.len() > 128 || identity.len() > 128 {
        return Err((
            StatusCode::BAD_REQUEST,
            "room and identity must contain 1-128 characters".into(),
        ));
    }

    let api_key = env::var("LIVEKIT_API_KEY").unwrap_or_else(|_| "devkey".into());
    let api_secret = env::var("LIVEKIT_API_SECRET").unwrap_or_else(|_| "secret".into());
    let server_url = env::var("LIVEKIT_URL")
        .ok()
        .map(|server_url| server_url.trim().to_owned())
        .filter(|server_url| !server_url.is_empty());
    let can_publish = identity_not_listed("LIVEKIT_NON_PUBLISHER_IDENTITIES", identity);
    let can_publish_data = identity_not_listed("LIVEKIT_NON_DATA_PUBLISHER_IDENTITIES", identity);
    let can_update_own_metadata =
        identity_not_listed("LIVEKIT_NON_METADATA_UPDATER_IDENTITIES", identity);
    let participant_token = AccessToken::with_api_key(&api_key, &api_secret)
        .with_identity(identity)
        .with_name(identity)
        .with_grants(VideoGrants {
            room_join: true,
            room: room.into(),
            can_publish,
            can_publish_data,
            can_update_own_metadata,
            ..Default::default()
        })
        .to_jwt()
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;

    Ok(Json(TokenResponse {
        participant_token,
        server_url,
    }))
}

fn identity_not_listed(variable: &str, identity: &str) -> bool {
    env::var(variable)
        .map(|identities| {
            !identities
                .split(',')
                .any(|candidate| candidate.trim() == identity)
        })
        .unwrap_or(true)
}
