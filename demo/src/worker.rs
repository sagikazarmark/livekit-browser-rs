//! Cloudflare Worker entry point.
//!
//! Only the routes that need the API secret run here. `wrangler.toml` lists
//! them under `run_worker_first`; every other path is served straight from the
//! static asset bundle at the edge and never reaches this code.

use worker::{Context, Env, Method, Request, Response, Result, event};

use crate::token::{TokenRequest, TokenService, unix_now};

#[event(fetch)]
async fn fetch(mut request: Request, env: Env, _context: Context) -> Result<Response> {
    match (request.method(), request.path().as_str()) {
        (Method::Get, "/healthz") => Response::ok("ok"),
        (Method::Post, "/api/token") => issue_token(&mut request, &env).await,
        _ => Response::error("Not Found", 404),
    }
}

async fn issue_token(request: &mut Request, env: &Env) -> Result<Response> {
    let Ok(payload) = request.json::<TokenRequest>().await else {
        return Response::error("expected a JSON body with room and identity", 400);
    };

    match token_service(env).issue(&payload, unix_now()) {
        Ok(response) => Response::from_json(&response),
        Err(error) => Response::error(error.to_string(), error.status()),
    }
}

fn token_service(env: &Env) -> TokenService {
    TokenService {
        api_key: binding(env, "LIVEKIT_API_KEY").unwrap_or_else(|| "devkey".into()),
        api_secret: binding(env, "LIVEKIT_API_SECRET").unwrap_or_else(|| "secret".into()),
        server_url: binding(env, "LIVEKIT_URL").filter(|server_url| !server_url.is_empty()),
        // The permission-denial lists only exist so the Playwright suite can
        // drive failure paths against the Docker stack, which never runs this
        // build. Every participant here gets the default grants.
        ..TokenService::default()
    }
}

/// Reads a `[vars]` entry, falling back to a value deployed with
/// `wrangler secret put`, so the same names work in both deployment styles.
fn binding(env: &Env, name: &str) -> Option<String> {
    env.var(name)
        .map(|value| value.to_string())
        .or_else(|_| env.secret(name).map(|value| value.to_string()))
        .ok()
        .map(|value| value.trim().to_owned())
}
