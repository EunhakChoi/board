//! Black-box tests of the router, plus the contract check.

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use board_engine::{Engine, Settings, StoreError};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

#[derive(Default)]
struct Memory(Mutex<HashMap<String, String>>);

impl Settings for Memory {
    fn get(&self, key: &str) -> Result<Option<String>, StoreError> {
        Ok(self.0.lock().unwrap().get(key).cloned())
    }
    fn set(&self, key: &str, value: &str) -> Result<(), StoreError> {
        self.0.lock().unwrap().insert(key.into(), value.into());
        Ok(())
    }
}

fn engine(local_mode: bool) -> Engine {
    Engine::new(Arc::new(Memory::default()), local_mode)
}

async fn call(engine: Engine, path: &str) -> (StatusCode, Value) {
    let response = board_api::router(engine)
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn health() {
    assert_eq!(
        call(engine(true), "/api/health").await,
        (StatusCode::OK, json!({ "ok": true }))
    );
}

#[tokio::test]
async fn version_reports_latest_when_known() {
    let engine = engine(true);
    let (_, body) = call(engine.clone(), "/api/version").await;
    assert_eq!(body["latest"], Value::Null);
    engine.record_latest_version("9.9.9").unwrap();
    let (_, body) = call(engine, "/api/version").await;
    assert_eq!(body["latest"], "9.9.9");
    assert_eq!(body["version"], board_model::VERSION);
}

#[tokio::test]
async fn me_is_the_local_admin_in_local_mode() {
    let (status, body) = call(engine(true), "/api/auth/me").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        json!({ "kind": "local", "name": "Local admin", "role": "admin", "localMode": true })
    );
}

#[tokio::test]
async fn me_needs_sign_in_outside_local_mode() {
    let (status, body) = call(engine(false), "/api/auth/me").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["code"], "unauthorized");
}

#[tokio::test]
async fn unknown_routes_use_the_error_shape() {
    let (status, body) = call(engine(true), "/api/nope").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "not_found");
}

/// Every route in the router is in the contract and every contract operation is routed.
#[test]
fn routes_match_the_contract() {
    let contract = include_str!("../../../contracts/openapi.yaml");
    let mut documented = BTreeSet::new();
    let mut path = None;
    for line in contract.lines() {
        if let Some(rest) = line.strip_prefix("  /") {
            path = rest.strip_suffix(':').map(|p| format!("/{p}"));
        } else if let (Some(p), Some(method)) = (&path, line.strip_prefix("    ")) {
            let method = method.trim_end_matches(':');
            if ["get", "post", "put", "patch", "delete"].contains(&method) {
                documented.insert((method.to_owned(), p.clone()));
            }
        } else if !line.starts_with(' ') {
            path = None;
        }
    }
    let routed: BTreeSet<_> = board_api::ROUTES
        .iter()
        .map(|(m, p)| ((*m).to_owned(), (*p).to_owned()))
        .collect();
    assert_eq!(routed, documented);
}
