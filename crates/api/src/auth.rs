//! Working out who is calling, and coming in with a token (`docs/design/security.md` §2).
//!
//! In order: the local door of a local charet (the owner), a token the CLI sends, a browser's
//! session cookie, a visitor's cookie. Every address but the local door follows the same rules.

use axum::Json;
use axum::extract::{FromRequestParts, OptionalFromRequestParts, State};
use axum::http::header::{COOKIE, SET_COOKIE};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use charet_engine::{Engine, EngineError, SESSION_SECS, SessionState};
use charet_model::{Actor, Role};
use serde::{Deserialize, Serialize};

use crate::ApiError;
use crate::projects::blocking;

/// The session cookie on the tool origin.
pub const SESSION_COOKIE: &str = "charet_session";
/// The id a visitor's browser keeps, for their made-up name.
pub const VISITOR_COOKIE: &str = "charet_visitor";
/// Changing requests that rely on a cookie must carry this header. Browsers only send custom
/// headers cross-origin after a CORS preflight, which charet never allows, so a page on another
/// site cannot forge such a request.
pub const CLIENT_HEADER: &str = "x-charet-client";

/// Marks requests that came through the remote door (what tunnels and reverse proxies reach).
#[derive(Debug, Clone, Copy)]
pub struct Remote;

/// True for a request through the local door of a local charet: it acts as the owner.
#[derive(Debug, Clone, Copy)]
pub struct LocalDoor(pub bool);

impl FromRequestParts<Engine> for LocalDoor {
    type Rejection = std::convert::Infallible;

    fn from_request_parts(
        parts: &mut Parts,
        engine: &Engine,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> {
        std::future::ready(Ok(LocalDoor(local_door(parts, engine))))
    }
}

fn local_door(parts: &Parts, engine: &Engine) -> bool {
    engine.local_mode() && parts.extensions.get::<Remote>().is_none()
}

/// Who is calling.
#[derive(Debug)]
pub struct Caller(pub Actor);

impl FromRequestParts<Engine> for Caller {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, engine: &Engine) -> Result<Self, ApiError> {
        if local_door(parts, engine) {
            return Ok(Caller(engine.local_actor()));
        }
        // The CLI: no cookie, so no cross-site forgery to guard against.
        if let Some(token) = bearer(&parts.headers) {
            let engine = engine.clone();
            return match blocking(move || engine.token_actor(&token)).await? {
                Some(actor) => Ok(Caller(actor)),
                None => Err(invalid_token()),
            };
        }
        let session = cookie(&parts.headers, SESSION_COOKIE);
        let visitor = cookie(&parts.headers, VISITOR_COOKIE);
        if session.is_none() && visitor.is_none() {
            return Err(ApiError::unauthorized());
        }
        if !safe(&parts.method) && !parts.headers.contains_key(CLIENT_HEADER) {
            return Err(csrf());
        }
        let engine = engine.clone();
        blocking(move || browser(&engine, session.as_deref(), visitor.as_deref()))
            .await?
            .map(Caller)
    }
}

/// Nobody known is fine (`None`); a bad token or a replaced session still fails.
impl OptionalFromRequestParts<Engine> for Caller {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        engine: &Engine,
    ) -> Result<Option<Self>, ApiError> {
        match <Caller as FromRequestParts<Engine>>::from_request_parts(parts, engine).await {
            Ok(caller) => Ok(Some(caller)),
            Err(error) if error.code() == "unauthorized" => Ok(None),
            Err(error) => Err(error),
        }
    }
}

/// A browser: its member session, else the visitor it is.
fn browser(
    engine: &Engine,
    session: Option<&str>,
    visitor: Option<&str>,
) -> Result<Result<Actor, ApiError>, EngineError> {
    if let Some(session) = session {
        match engine.session(session)? {
            Some(SessionState::Active(actor)) => return Ok(Ok(actor)),
            Some(SessionState::Replaced) => return Ok(Err(replaced())),
            // Ended (the token was reissued): carry on as a visitor.
            None => {}
        }
    }
    Ok(match visitor {
        Some(id) => engine.visitor(id)?.ok_or_else(ApiError::unauthorized),
        None => Err(ApiError::unauthorized()),
    })
}

fn safe(method: &Method) -> bool {
    matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

fn csrf() -> ApiError {
    ApiError::new(
        StatusCode::FORBIDDEN,
        "csrf",
        format!("send the {CLIENT_HEADER} header with changing requests"),
    )
}

fn invalid_token() -> ApiError {
    ApiError::new(
        StatusCode::UNAUTHORIZED,
        "invalid_token",
        "This token was reissued or removed; ask an admin for the current one",
    )
}

fn replaced() -> ApiError {
    ApiError::new(
        StatusCode::UNAUTHORIZED,
        "replaced",
        "Your token was used to sign in on another browser",
    )
}

/// The token of an `Authorization: Bearer` header, if sent.
pub fn bearer(headers: &HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(|t| t.trim().to_owned())
        .filter(|t| !t.is_empty())
}

/// The value of cookie `name`, if sent.
pub fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v.to_owned())
        .filter(|v| !v.is_empty())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Me {
    /// `local` (the built-in admin), `user` (a member) or `visitor`.
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    name: String,
    role: Role,
    /// True through the local door: this machine's admin, no token needed.
    local_mode: bool,
}

impl Me {
    fn new(actor: Actor, local_mode: bool) -> Self {
        let role = actor.role();
        let name = actor.name().to_owned();
        let (kind, id) = match actor {
            Actor::Local => ("local", None),
            Actor::User { id, .. } => ("user", Some(id)),
            Actor::Visitor { id, .. } => ("visitor", Some(id)),
        };
        Self {
            kind,
            id,
            name,
            role,
            local_mode,
        }
    }
}

/// `GET /api/auth/me`: who is calling. A browser without a session becomes a visitor here (the
/// cookie is set) while visitors are allowed; otherwise it is told to enter a token.
pub async fn me(
    State(engine): State<Engine>,
    LocalDoor(local): LocalDoor,
    headers: HeaderMap,
    caller: Option<Caller>,
) -> Result<Response, ApiError> {
    if let Some(Caller(actor)) = caller {
        return Ok(Json(Me::new(actor, local)).into_response());
    }
    if bearer(&headers).is_some() {
        return Err(ApiError::unauthorized());
    }
    let id = charet_engine::new_visitor_id();
    let found = {
        let (engine, id) = (engine.clone(), id.clone());
        blocking(move || engine.visitor(&id)).await?
    };
    let Some(actor) = found else {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "token_required",
            "This charet needs a token; ask an admin for one",
        ));
    };
    let mut response = Json(Me::new(actor, local)).into_response();
    set_cookie(
        &mut response,
        &cookie_text(VISITOR_COOKIE, &id, secure(&engine), SESSION_SECS),
    );
    Ok(response)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokenInput {
    token: String,
}

/// `POST /api/auth/token`: a browser comes in with a member's token. Another browser that came
/// in with it is signed out.
pub async fn sign_in(
    State(engine): State<Engine>,
    LocalDoor(local): LocalDoor,
    headers: HeaderMap,
    Json(input): Json<TokenInput>,
) -> Result<Response, ApiError> {
    if !headers.contains_key(CLIENT_HEADER) {
        return Err(csrf());
    }
    let secure = secure(&engine);
    let signed = match blocking(move || engine.sign_in_with_token(&input.token)).await {
        Ok(signed) => signed,
        Err(error) if error.status() == StatusCode::NOT_FOUND => return Err(invalid_token()),
        Err(error) => return Err(error),
    };
    let mut response = Json(Me::new(signed.user.actor(), local)).into_response();
    set_cookie(
        &mut response,
        &cookie_text(SESSION_COOKIE, &signed.session, secure, SESSION_SECS),
    );
    Ok(response)
}

/// `POST /api/auth/logout`: ends the browser's session (fine without one); it is a visitor again.
pub async fn logout(
    State(engine): State<Engine>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let secure = secure(&engine);
    if let Some(session) = cookie(&headers, SESSION_COOKIE) {
        if !headers.contains_key(CLIENT_HEADER) {
            return Err(csrf());
        }
        blocking(move || engine.sign_out(&session)).await?;
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    set_cookie(&mut response, &cookie_text(SESSION_COOKIE, "", secure, 0));
    Ok(response)
}

fn secure(engine: &Engine) -> bool {
    engine.tool_url().is_some_and(|u| u.starts_with("https://"))
}

fn set_cookie(response: &mut Response, text: &str) {
    if let Ok(value) = HeaderValue::from_str(text) {
        response.headers_mut().append(SET_COOKIE, value);
    }
}

pub(crate) fn cookie_text(name: &str, value: &str, secure: bool, max_age: u64) -> String {
    format!(
        "{name}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}{}",
        if secure { "; Secure" } else { "" }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_one_cookie_among_many() {
        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            HeaderValue::from_static("a=1; charet_session=tok; b=2"),
        );
        assert_eq!(cookie(&headers, SESSION_COOKIE).as_deref(), Some("tok"));
        assert_eq!(cookie(&headers, "missing"), None);
    }

    #[test]
    fn cookie_is_http_only_and_secure_over_https() {
        let text = cookie_text(SESSION_COOKIE, "t", true, 60);
        assert!(text.contains("HttpOnly") && text.contains("SameSite=Lax"));
        assert!(text.ends_with("; Secure"));
        assert!(!cookie_text(SESSION_COOKIE, "t", false, 60).contains("Secure"));
    }
}
