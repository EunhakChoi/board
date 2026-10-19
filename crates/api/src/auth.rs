//! Sign-in routes and working out who is calling (`docs/design/security.md` §2).

use axum::extract::{FromRequestParts, Query, State};
use axum::http::header::{COOKIE, LOCATION, SET_COOKIE};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use board_engine::{Engine, SESSION_SECS, SessionState, SignIn};
use board_model::Actor;
use serde::Deserialize;

use crate::ApiError;
use crate::projects::blocking;

/// The session cookie on the tool origin.
pub const SESSION_COOKIE: &str = "board_session";
/// Changing requests that rely on the cookie must carry this header. Browsers only send custom
/// headers cross-origin after a CORS preflight, which board never allows, so a page on another
/// site cannot forge such a request.
pub const CLIENT_HEADER: &str = "x-board-client";

/// Who is calling: the built-in admin in local mode, otherwise the session's member.
#[derive(Debug)]
pub struct Caller(pub Actor);

impl FromRequestParts<Engine> for Caller {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, engine: &Engine) -> Result<Self, ApiError> {
        if engine.local_mode() {
            return Ok(Caller(Actor::Local));
        }
        let Some(token) = cookie(&parts.headers, SESSION_COOKIE) else {
            return Err(ApiError::unauthorized());
        };
        if !safe(&parts.method) && !parts.headers.contains_key(CLIENT_HEADER) {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "csrf",
                format!("send the {CLIENT_HEADER} header with changing requests"),
            ));
        }
        let engine = engine.clone();
        match blocking(move || engine.session(&token)).await? {
            Some(SessionState::Active(actor)) => Ok(Caller(actor)),
            Some(SessionState::Waiting(_)) => Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "pending",
                "An admin has not approved your access yet",
            )),
            Some(SessionState::Refused(refusal)) => Err(ApiError::new(
                StatusCode::FORBIDDEN,
                refusal.code(),
                "You do not have access to this board",
            )),
            None => Err(ApiError::unauthorized()),
        }
    }
}

fn safe(method: &Method) -> bool {
    matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartQuery {
    setup: Option<String>,
    return_to: Option<String>,
}

/// `GET /api/auth/oidc/start`: sends the browser to the provider.
pub async fn start(
    State(engine): State<Engine>,
    Query(query): Query<StartQuery>,
) -> Result<Response, ApiError> {
    let url =
        blocking(move || engine.begin_sign_in(query.setup.as_deref(), query.return_to.as_deref()))
            .await?;
    Ok(redirect(&url, None))
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    state: Option<String>,
    code: Option<String>,
    error: Option<String>,
}

/// `GET /api/auth/oidc/callback`: the provider sends the browser back here.
pub async fn callback(
    State(engine): State<Engine>,
    Query(query): Query<CallbackQuery>,
) -> Response {
    let (Some(state), Some(code)) = (query.state, query.code) else {
        // The person cancelled at the provider, or it reported an error.
        let reason = if query.error.as_deref() == Some("access_denied") {
            "cancelled"
        } else {
            "failed"
        };
        return redirect(&format!("/signin?error={reason}"), None);
    };
    let secure = engine.tool_url().is_some_and(|u| u.starts_with("https://"));
    match blocking(move || engine.finish_sign_in(&state, &code)).await {
        Ok(SignIn::Session {
            token, return_to, ..
        }) => redirect(
            &return_to,
            Some(session_cookie(&token, secure, SESSION_SECS)),
        ),
        Ok(SignIn::Refused(refusal)) => {
            redirect(&format!("/signin?error={}", refusal.code()), None)
        }
        Err(error) => {
            let code = match error.status() {
                StatusCode::BAD_REQUEST => "expired",
                StatusCode::SERVICE_UNAVAILABLE => "unavailable",
                _ => "failed",
            };
            redirect(&format!("/signin?error={code}"), None)
        }
    }
}

/// `POST /api/auth/logout`: ends the session (fine without one).
pub async fn logout(
    State(engine): State<Engine>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let secure = engine.tool_url().is_some_and(|u| u.starts_with("https://"));
    if let Some(token) = cookie(&headers, SESSION_COOKIE) {
        if !headers.contains_key(CLIENT_HEADER) {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "csrf",
                format!("send the {CLIENT_HEADER} header with changing requests"),
            ));
        }
        blocking(move || engine.sign_out(&token)).await?;
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    if let Ok(value) = HeaderValue::from_str(&session_cookie("", secure, 0)) {
        response.headers_mut().insert(SET_COOKIE, value);
    }
    Ok(response)
}

fn session_cookie(token: &str, secure: bool, max_age: u64) -> String {
    format!(
        "{SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}{}",
        if secure { "; Secure" } else { "" }
    )
}

fn redirect(location: &str, cookie: Option<String>) -> Response {
    let mut response = StatusCode::FOUND.into_response();
    if let Ok(value) = HeaderValue::from_str(location) {
        response.headers_mut().insert(LOCATION, value);
    }
    if let Some(value) = cookie.and_then(|c| HeaderValue::from_str(&c).ok()) {
        response.headers_mut().insert(SET_COOKIE, value);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_one_cookie_among_many() {
        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            HeaderValue::from_static("a=1; board_session=tok; b=2"),
        );
        assert_eq!(cookie(&headers, SESSION_COOKIE).as_deref(), Some("tok"));
        assert_eq!(cookie(&headers, "missing"), None);
    }

    #[test]
    fn cookie_is_http_only_and_secure_over_https() {
        let text = session_cookie("t", true, 60);
        assert!(text.contains("HttpOnly") && text.contains("SameSite=Lax"));
        assert!(text.ends_with("; Secure"));
        assert!(!session_cookie("t", false, 60).contains("Secure"));
    }
}
