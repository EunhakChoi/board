//! Sign-in through an `OpenID` Connect provider (Google first): discovery, authorization code flow
//! with PKCE.
//!
//! The ID token comes straight from the provider's token endpoint over TLS, so its claims are
//! trusted without checking the signature (OIDC Core 3.1.3.7); issuer, audience, nonce and expiry
//! are still checked.

use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use board_engine::{AuthRequest, Claims, Identity, WorkspaceError};
use serde::Deserialize;
use ureq::Agent;

/// Google's issuer, the default provider.
pub const GOOGLE: &str = "https://accounts.google.com";

/// A provider at `issuer` with a registered client.
pub struct Oidc {
    issuer: String,
    client_id: String,
    client_secret: String,
    redirect_uri: String,
    provider: String,
    agent: Agent,
    discovery: OnceLock<Discovery>,
}

impl std::fmt::Debug for Oidc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Oidc")
            .field("issuer", &self.issuer)
            .field("client_id", &self.client_id)
            .field("redirect_uri", &self.redirect_uri)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Deserialize)]
struct Discovery {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
}

#[derive(Deserialize)]
struct TokenResponse {
    id_token: String,
}

#[derive(Deserialize)]
struct IdToken {
    iss: String,
    sub: String,
    aud: Audience,
    exp: u64,
    nonce: Option<String>,
    email: Option<String>,
    #[serde(default)]
    email_verified: Verified,
    name: Option<String>,
    picture: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Audience {
    One(String),
    Many(Vec<String>),
}

/// `email_verified` is a boolean, but some providers send the string `"true"`.
#[derive(Deserialize, Default)]
#[serde(untagged)]
enum Verified {
    Bool(bool),
    Text(String),
    #[default]
    Missing,
}

impl Oidc {
    pub fn new(issuer: &str, client_id: &str, client_secret: &str, redirect_uri: &str) -> Self {
        let issuer = issuer.trim_end_matches('/').to_owned();
        let provider = if issuer == GOOGLE {
            "google".to_owned()
        } else {
            issuer
                .split("://")
                .nth(1)
                .unwrap_or(&issuer)
                .trim_end_matches('/')
                .to_owned()
        };
        let agent = Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(15)))
            .build()
            .new_agent();
        Self {
            issuer,
            client_id: client_id.to_owned(),
            client_secret: client_secret.to_owned(),
            redirect_uri: redirect_uri.to_owned(),
            provider,
            agent,
            discovery: OnceLock::new(),
        }
    }

    /// Where the provider sends people back (register this at the provider).
    pub fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }

    fn discovery(&self) -> Result<&Discovery, WorkspaceError> {
        if let Some(found) = self.discovery.get() {
            return Ok(found);
        }
        let url = format!("{}/.well-known/openid-configuration", self.issuer);
        let mut response = self
            .agent
            .get(&url)
            .call()
            .map_err(|e| WorkspaceError::Unavailable(format!("{url}: {e}")))?;
        if !response.status().is_success() {
            return Err(WorkspaceError::Failed(format!(
                "{url} answered {}",
                response.status()
            )));
        }
        let found: Discovery = response
            .body_mut()
            .read_json()
            .map_err(|e| WorkspaceError::Failed(format!("{url}: {e}")))?;
        Ok(self.discovery.get_or_init(|| found))
    }
}

impl Identity for Oidc {
    fn provider(&self) -> &str {
        &self.provider
    }
    fn health(&self) -> Result<(), WorkspaceError> {
        self.discovery().map(|_| ())
    }
    fn callback_url(&self) -> &str {
        &self.redirect_uri
    }

    fn authorize_url(&self, request: &AuthRequest) -> Result<String, WorkspaceError> {
        let endpoint = &self.discovery()?.authorization_endpoint;
        let query = [
            ("response_type", "code"),
            ("client_id", self.client_id.as_str()),
            ("redirect_uri", self.redirect_uri.as_str()),
            ("scope", "openid email profile"),
            ("state", request.state.as_str()),
            ("nonce", request.nonce.as_str()),
            ("code_challenge", request.code_challenge.as_str()),
            ("code_challenge_method", "S256"),
            ("prompt", "select_account"),
        ]
        .iter()
        .map(|(k, v)| format!("{k}={}", encode(v)))
        .collect::<Vec<_>>()
        .join("&");
        let sep = if endpoint.contains('?') { '&' } else { '?' };
        Ok(format!("{endpoint}{sep}{query}"))
    }

    fn exchange(
        &self,
        code: &str,
        code_verifier: &str,
        nonce: &str,
    ) -> Result<Claims, WorkspaceError> {
        let discovery = self.discovery()?;
        let mut response = self
            .agent
            .post(&discovery.token_endpoint)
            .send_form([
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", self.redirect_uri.as_str()),
                ("client_id", self.client_id.as_str()),
                ("client_secret", self.client_secret.as_str()),
                ("code_verifier", code_verifier),
            ])
            .map_err(|e| WorkspaceError::Unavailable(format!("token endpoint: {e}")))?;
        if !response.status().is_success() {
            let body = response.body_mut().read_to_string().unwrap_or_default();
            return Err(WorkspaceError::Failed(format!(
                "token endpoint answered {}: {}",
                response.status(),
                body.chars().take(300).collect::<String>()
            )));
        }
        let tokens: TokenResponse = response
            .body_mut()
            .read_json()
            .map_err(|e| WorkspaceError::Failed(format!("token response: {e}")))?;
        let token = decode_id_token(&tokens.id_token)?;
        check(&token, &discovery.issuer, &self.client_id, nonce, now())?;
        Ok(Claims {
            subject: token.sub,
            email: token.email.unwrap_or_default(),
            email_verified: match token.email_verified {
                Verified::Bool(b) => b,
                Verified::Text(t) => t == "true",
                Verified::Missing => false,
            },
            name: token.name,
            picture: token.picture,
        })
    }
}

fn decode_id_token(token: &str) -> Result<IdToken, WorkspaceError> {
    let payload = token
        .split('.')
        .nth(1)
        .ok_or_else(|| WorkspaceError::Failed("ID token is not a JWT".into()))?;
    let bytes = URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .map_err(|e| WorkspaceError::Failed(format!("ID token: {e}")))?;
    serde_json::from_slice(&bytes).map_err(|e| WorkspaceError::Failed(format!("ID token: {e}")))
}

fn check(
    token: &IdToken,
    issuer: &str,
    client_id: &str,
    nonce: &str,
    now: u64,
) -> Result<(), WorkspaceError> {
    let fail = |what: &str| Err(WorkspaceError::Failed(format!("ID token: {what}")));
    if token.iss.trim_end_matches('/') != issuer.trim_end_matches('/') {
        return fail("wrong issuer");
    }
    let audience = match &token.aud {
        Audience::One(a) => a == client_id,
        Audience::Many(all) => all.iter().any(|a| a == client_id),
    };
    if !audience {
        return fail("issued to another client");
    }
    if token.nonce.as_deref() != Some(nonce) {
        return fail("nonce does not match");
    }
    // A minute of clock skew is tolerated.
    if token.exp + 60 < now {
        return fail("expired");
    }
    Ok(())
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Percent-encodes a query value.
fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                char::from(b).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    use super::*;

    /// A provider on localhost that serves discovery and a token endpoint returning `claims`.
    fn provider(claims: serde_json::Value) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let (issuer, log) = (base.clone(), seen.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let mut length = 0;
                loop {
                    let mut header = String::new();
                    reader.read_line(&mut header).unwrap();
                    if header.trim().is_empty() {
                        break;
                    }
                    if let Some(v) = header.to_lowercase().strip_prefix("content-length:") {
                        length = v.trim().parse().unwrap();
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                log.lock().unwrap().push(format!(
                    "{} {}",
                    line.trim(),
                    String::from_utf8_lossy(&body)
                ));
                let reply = if line.contains("openid-configuration") {
                    serde_json::json!({
                        "issuer": issuer,
                        "authorization_endpoint": format!("{issuer}/auth"),
                        "token_endpoint": format!("{issuer}/token"),
                    })
                } else {
                    let mut claims = claims.clone();
                    if claims.get("iss").is_none() {
                        claims["iss"] = issuer.clone().into();
                    }
                    let payload = URL_SAFE_NO_PAD.encode(claims.to_string());
                    serde_json::json!({ "id_token": format!("e30.{payload}.sig"), "access_token": "a" })
                };
                let text = reply.to_string();
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{text}",
                    text.len()
                )
                .unwrap();
            }
        });
        (base, seen)
    }

    fn request() -> AuthRequest {
        AuthRequest {
            state: "st".into(),
            nonce: "n1".into(),
            code_challenge: "ch".into(),
        }
    }

    #[test]
    fn authorize_url_and_exchange() {
        let (issuer, seen) = provider(serde_json::json!({
            "sub": "123", "aud": ["client", "other"], "exp": now() + 300,
            "nonce": "n1", "email": "ann@example.com", "email_verified": "true", "name": "Ann"
        }));
        let oidc = Oidc::new(
            &issuer,
            "client",
            "secret",
            "https://board.test/api/auth/oidc/callback",
        );
        let url = oidc.authorize_url(&request()).unwrap();
        assert!(url.starts_with(&format!(
            "{issuer}/auth?response_type=code&client_id=client"
        )));
        assert!(
            url.contains("redirect_uri=https%3A%2F%2Fboard.test%2Fapi%2Fauth%2Foidc%2Fcallback")
        );
        assert!(url.contains("scope=openid%20email%20profile"));
        assert!(url.contains("code_challenge=ch&code_challenge_method=S256"));
        let claims = oidc.exchange("the-code", "verifier", "n1").unwrap();
        assert_eq!(claims.subject, "123");
        assert_eq!(claims.email, "ann@example.com");
        assert!(claims.email_verified);
        assert_eq!(claims.name.as_deref(), Some("Ann"));
        let token_call = seen.lock().unwrap().last().unwrap().clone();
        assert!(token_call.starts_with("POST /token"));
        assert!(token_call.contains("code=the-code"));
        assert!(token_call.contains("code_verifier=verifier"));
        // The discovery document is fetched once.
        let discoveries = seen
            .lock()
            .unwrap()
            .iter()
            .filter(|l| l.contains("openid-configuration"))
            .count();
        assert_eq!(discoveries, 1);
        // Another nonce is refused.
        assert!(oidc.exchange("the-code", "verifier", "n2").is_err());
    }

    #[test]
    fn checks_issuer_audience_nonce_and_expiry() {
        let token = |iss: &str, aud: &str, nonce: &str, exp: u64| IdToken {
            iss: iss.into(),
            sub: "s".into(),
            aud: Audience::One(aud.into()),
            exp,
            nonce: Some(nonce.into()),
            email: None,
            email_verified: Verified::Missing,
            name: None,
            picture: None,
        };
        let ok = check(
            &token("https://i", "c", "n", 1000),
            "https://i/",
            "c",
            "n",
            900,
        );
        assert!(ok.is_ok());
        assert!(
            check(
                &token("https://x", "c", "n", 1000),
                "https://i",
                "c",
                "n",
                900
            )
            .is_err()
        );
        assert!(
            check(
                &token("https://i", "x", "n", 1000),
                "https://i",
                "c",
                "n",
                900
            )
            .is_err()
        );
        assert!(
            check(
                &token("https://i", "c", "x", 1000),
                "https://i",
                "c",
                "n",
                900
            )
            .is_err()
        );
        assert!(
            check(
                &token("https://i", "c", "n", 1000),
                "https://i",
                "c",
                "n",
                1100
            )
            .is_err()
        );
    }

    #[test]
    fn google_is_named_google() {
        assert_eq!(Oidc::new(GOOGLE, "c", "s", "r").provider(), "google");
        assert_eq!(
            Oidc::new("https://id.example/", "c", "s", "r").provider(),
            "id.example"
        );
    }
}
