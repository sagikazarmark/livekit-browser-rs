//! LiveKit access tokens, minted on every target the example builds for.
//!
//! `livekit-api` cannot be used here. `AccessToken::with_api_key` reads
//! `std::time::SystemTime::now()`, which on `wasm32-unknown-unknown` is a stub
//! that panics with "time not implemented on this platform", so the Cloudflare
//! Worker build would abort on its first request. This module mints the same
//! HS256 JWT directly and takes its clock from `web-time`, which resolves to
//! `Date.now()` on wasm and to `std::time` everywhere else.
//!
//! The claim shape mirrors `livekit_api::access_token::Claims` field for field;
//! the tests below pin our output against that crate on native targets.

use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

/// Lifetime of an issued token, matching `livekit_api`'s `DEFAULT_TTL`.
pub const DEFAULT_TTL: Duration = Duration::from_secs(6 * 60 * 60);

/// Seconds since the Unix epoch, from `Date.now()` on wasm and the system clock
/// elsewhere.
pub fn unix_now() -> Duration {
    web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
}

#[derive(Deserialize)]
pub struct TokenRequest {
    pub room: String,
    pub identity: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenResponse {
    pub participant_token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoGrants {
    pub room_create: bool,
    pub room_list: bool,
    pub room_record: bool,
    pub room_admin: bool,
    pub room_join: bool,
    pub room: String,
    pub destination_room: String,
    pub can_publish: bool,
    pub can_subscribe: bool,
    pub can_publish_data: bool,
    pub can_publish_sources: Vec<String>,
    pub can_update_own_metadata: bool,
    pub ingress_admin: bool,
    pub hidden: bool,
    pub recorder: bool,
}

impl Default for VideoGrants {
    fn default() -> Self {
        Self {
            room_create: false,
            room_list: false,
            room_record: false,
            room_admin: false,
            room_join: false,
            room: String::new(),
            destination_room: String::new(),
            can_publish: true,
            can_subscribe: true,
            can_publish_data: true,
            can_publish_sources: Vec::new(),
            can_update_own_metadata: false,
            ingress_admin: false,
            hidden: false,
            recorder: false,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SipGrants {
    pub admin: bool,
    pub call: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Claims {
    pub exp: u64,
    pub iss: String,
    pub nbf: u64,
    pub sub: String,
    pub name: String,
    pub video: VideoGrants,
    pub sip: SipGrants,
    pub sha256: String,
    pub metadata: String,
    pub attributes: HashMap<String, String>,
    pub room_config: Option<serde_json::Value>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum TokenError {
    MissingKeys,
    MissingIdentityOrRoom,
    Encoding,
}

impl fmt::Display for TokenError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingKeys => formatter.write_str("invalid API key or secret key"),
            Self::MissingIdentityOrRoom => {
                formatter.write_str("token grants room_join but doesn't have an identity or room")
            }
            Self::Encoding => formatter.write_str("failed to encode jwt"),
        }
    }
}

impl Error for TokenError {}

/// Mints an HS256 JWT for `claims`, signed with the LiveKit API secret.
pub fn encode_jwt(api_key: &str, api_secret: &str, claims: &Claims) -> Result<String, TokenError> {
    if api_key.is_empty() || api_secret.is_empty() {
        return Err(TokenError::MissingKeys);
    }
    if claims.video.room_join && (claims.sub.is_empty() || claims.video.room.is_empty()) {
        return Err(TokenError::MissingIdentityOrRoom);
    }

    let header = URL_SAFE_NO_PAD.encode(br#"{"typ":"JWT","alg":"HS256"}"#);
    let payload =
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims).map_err(|_| TokenError::Encoding)?);
    let message = format!("{header}.{payload}");

    // HMAC accepts keys of any length, so this cannot fail.
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(api_secret.as_bytes())
        .map_err(|_| TokenError::Encoding)?;
    mac.update(message.as_bytes());
    let signature = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());

    Ok(format!("{message}.{signature}"))
}

/// Issues participant tokens for the example's room, denying individual
/// permissions to the identities the browser tests use to exercise failures.
///
/// The identity lists stay in their raw comma-separated form so the native
/// server and the Cloudflare Worker can populate them straight from their own
/// environment without duplicating the parsing.
#[derive(Debug, Clone, Default)]
pub struct TokenService {
    pub api_key: String,
    pub api_secret: String,
    pub server_url: Option<String>,
    pub non_publishers: Option<String>,
    pub non_data_publishers: Option<String>,
    pub non_metadata_updaters: Option<String>,
}

#[derive(Debug)]
pub enum IssueError {
    InvalidRequest(&'static str),
    Token(TokenError),
}

impl IssueError {
    /// HTTP status this failure should be reported as.
    pub fn status(&self) -> u16 {
        match self {
            Self::InvalidRequest(_) => 400,
            Self::Token(_) => 500,
        }
    }
}

impl fmt::Display for IssueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest(message) => formatter.write_str(message),
            Self::Token(error) => error.fmt(formatter),
        }
    }
}

impl Error for IssueError {}

impl TokenService {
    pub fn issue(
        &self,
        request: &TokenRequest,
        issued_at: Duration,
    ) -> Result<TokenResponse, IssueError> {
        let room = request.room.trim();
        let identity = request.identity.trim();
        if room.is_empty() || identity.is_empty() || room.len() > 128 || identity.len() > 128 {
            return Err(IssueError::InvalidRequest(
                "room and identity must contain 1-128 characters",
            ));
        }

        let claims = Claims {
            exp: (issued_at + DEFAULT_TTL).as_secs(),
            iss: self.api_key.clone(),
            nbf: issued_at.as_secs(),
            sub: identity.to_owned(),
            name: identity.to_owned(),
            video: VideoGrants {
                room_join: true,
                room: room.to_owned(),
                can_publish: identity_not_listed(&self.non_publishers, identity),
                can_publish_data: identity_not_listed(&self.non_data_publishers, identity),
                can_update_own_metadata: identity_not_listed(&self.non_metadata_updaters, identity),
                ..VideoGrants::default()
            },
            ..Claims::default()
        };

        let participant_token =
            encode_jwt(&self.api_key, &self.api_secret, &claims).map_err(IssueError::Token)?;

        Ok(TokenResponse {
            participant_token,
            server_url: self.server_url.clone(),
        })
    }
}

fn identity_not_listed(identities: &Option<String>, identity: &str) -> bool {
    identities.as_ref().is_none_or(|identities| {
        !identities
            .split(',')
            .any(|candidate| candidate.trim() == identity)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "devkey";
    const SECRET: &str = "secret";
    const ISSUED_AT: Duration = Duration::from_secs(1_700_000_000);

    fn service() -> TokenService {
        TokenService {
            api_key: KEY.into(),
            api_secret: SECRET.into(),
            server_url: None,
            non_publishers: Some("publisher-denied".into()),
            non_data_publishers: Some("data-denied,also-data-denied".into()),
            non_metadata_updaters: None,
        }
    }

    fn request(identity: &str) -> TokenRequest {
        TokenRequest {
            room: "demo".into(),
            identity: identity.into(),
        }
    }

    fn payload(jwt: &str) -> serde_json::Value {
        let payload = jwt.split('.').nth(1).expect("jwt has three segments");
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).expect("base64url payload"))
            .expect("json payload")
    }

    #[test]
    fn issued_tokens_grant_room_join_for_the_requested_identity() {
        let token = service()
            .issue(&request("alice"), ISSUED_AT)
            .expect("token is issued");
        let claims = payload(&token.participant_token);

        assert_eq!(claims["sub"], "alice");
        assert_eq!(claims["iss"], KEY);
        assert_eq!(claims["nbf"], 1_700_000_000_u64);
        assert_eq!(claims["exp"], 1_700_000_000_u64 + DEFAULT_TTL.as_secs());
        assert_eq!(claims["video"]["roomJoin"], true);
        assert_eq!(claims["video"]["room"], "demo");
    }

    #[test]
    fn listed_identities_lose_only_their_own_permission() {
        let denied = service()
            .issue(&request("publisher-denied"), ISSUED_AT)
            .expect("token is issued");
        let claims = payload(&denied.participant_token);
        assert_eq!(claims["video"]["canPublish"], false);
        assert_eq!(claims["video"]["canPublishData"], true);

        let allowed = service()
            .issue(&request("alice"), ISSUED_AT)
            .expect("token is issued");
        let claims = payload(&allowed.participant_token);
        assert_eq!(claims["video"]["canPublish"], true);
    }

    #[test]
    fn identity_lists_match_whole_entries_only() {
        let token = service()
            .issue(&request("also-data-denied"), ISSUED_AT)
            .expect("token is issued");
        assert_eq!(
            payload(&token.participant_token)["video"]["canPublishData"],
            false
        );

        let token = service()
            .issue(&request("data-denie"), ISSUED_AT)
            .expect("token is issued");
        assert_eq!(
            payload(&token.participant_token)["video"]["canPublishData"],
            true
        );
    }

    #[test]
    fn blank_and_oversized_fields_are_rejected() {
        let oversized = "x".repeat(129);
        for (room, identity) in [("", "alice"), ("demo", "   "), ("demo", oversized.as_str())] {
            let request = TokenRequest {
                room: room.into(),
                identity: identity.into(),
            };
            let error = service()
                .issue(&request, ISSUED_AT)
                .expect_err("request is rejected");
            assert_eq!(error.status(), 400);
        }
    }

    #[test]
    fn missing_credentials_are_reported_as_a_server_failure() {
        let service = TokenService {
            api_secret: String::new(),
            ..service()
        };
        let error = service
            .issue(&request("alice"), ISSUED_AT)
            .expect_err("request is rejected");
        assert_eq!(error.status(), 500);
    }

    /// The Worker build cannot use `livekit-api`, so this pins our hand-rolled
    /// token against the crate the native server used to call. If LiveKit ever
    /// changes the claim shape, this fails rather than the demo silently
    /// issuing a token the server rejects.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn tokens_match_livekit_api() {
        use livekit_api::access_token::{AccessToken, TokenVerifier};

        let ours = service()
            .issue(&request("alice"), ISSUED_AT)
            .expect("token is issued")
            .participant_token;

        let theirs = AccessToken::with_api_key(KEY, SECRET)
            .with_identity("alice")
            .with_name("alice")
            .with_grants(livekit_api::access_token::VideoGrants {
                room_join: true,
                room: "demo".into(),
                can_publish: true,
                can_publish_data: true,
                can_update_own_metadata: true,
                ..Default::default()
            })
            .to_jwt()
            .expect("livekit-api issues a token");

        // `livekit-api` stamps `nbf`/`exp` from the wall clock, so compare
        // everything else and assert the lifetime separately.
        let mut ours_claims = payload(&ours);
        let mut theirs_claims = payload(&theirs);
        for claims in [&mut ours_claims, &mut theirs_claims] {
            let object = claims.as_object_mut().expect("claims are an object");
            assert_eq!(
                object["exp"].as_u64().expect("exp is a number")
                    - object["nbf"].as_u64().expect("nbf is a number"),
                DEFAULT_TTL.as_secs()
            );
            object.remove("exp");
            object.remove("nbf");
        }
        assert_eq!(ours_claims, theirs_claims);

        // And LiveKit's own verifier accepts our signature. This one has to be
        // stamped from the real clock, since the verifier enforces `exp`.
        let fresh = service()
            .issue(&request("alice"), unix_now())
            .expect("token is issued")
            .participant_token;
        TokenVerifier::with_api_key(KEY, SECRET)
            .verify(&fresh)
            .expect("livekit-api verifies our token");
    }
}
