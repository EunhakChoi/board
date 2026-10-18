//! Black-box tests of the router, plus the contract check.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use board_db::{CaptureStore, Db};
use board_engine::{
    Build, BuildState, BuiltFile, Engine, Ports, PreviewSettings, RenderJob, Renderer, Shot,
    Workspace, WorkspaceError,
};
use board_store::GitStore;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

fn engine(local_mode: bool) -> Engine {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let root = std::env::temp_dir().join(format!(
        "board-api-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&root);
    let db = Arc::new(Db::memory().unwrap());
    Engine::new(
        Ports {
            settings: db.clone(),
            records: db.clone(),
            store: Arc::new(GitStore::new(root.join("projects"))),
            workspace: Arc::new(ReadyWorkspace),
            renderer: Arc::new(FakeRenderer),
            captures: Arc::new(CaptureStore::new(db, root.join("captures"))),
        },
        local_mode,
    )
}

/// Every build is ready at once and has one page.
struct ReadyWorkspace;

impl Workspace for ReadyWorkspace {
    fn build(&self, _: &str, _: &str) -> Result<Option<Build>, WorkspaceError> {
        Ok(None)
    }
    fn start_build(
        &self,
        _: &str,
        _: &str,
        base: &str,
        source: Vec<u8>,
        _: bool,
    ) -> Result<Build, WorkspaceError> {
        assert!(base.starts_with('/') && base.ends_with('/'));
        assert!(source.starts_with(&[0x1f, 0x8b]), "gzip source");
        Ok(Build {
            state: BuildState::Ready,
            steps: Vec::new(),
            error: String::new(),
        })
    }
    fn file(&self, _: &str, _: &str, path: &str) -> Result<Option<BuiltFile>, WorkspaceError> {
        let body: &[u8] = match path {
            ".board/frame.html" => b"<!doctype html>",
            ".board/manifest.json" => {
                br#"{"manifestVersion":1,"screens":[{"id":"home","size":{"w":800,"h":600}}],"stories":[]}"#
            }
            _ => return Ok(None),
        };
        Ok(Some(BuiltFile {
            content_type: "text/html; charset=utf-8".into(),
            cache_control: "no-cache".into(),
            body: body.to_vec(),
        }))
    }
}

/// Every page is ready; dark pages report a missing mock.
struct FakeRenderer;

impl Renderer for FakeRenderer {
    fn render(&self, job: &RenderJob) -> Result<Shot, WorkspaceError> {
        let issues = if job.url.contains("theme=dark") {
            vec!["[board] missing mock GET /api/x".to_owned()]
        } else {
            Vec::new()
        };
        Ok(Shot {
            ready: Some("true".into()),
            issues,
            image: job.image.map(|_| b"\xff\xd8jpeg".to_vec()),
            elements: job
                .map
                .then(|| json!([{ "x": 0, "y": 0, "w": 10, "h": 10, "chain": ["Home"], "sel": "body", "tag": "main", "text": "", "src": "src/home.tsx:1:1" }])),
        })
    }
}

fn preview_settings() -> PreviewSettings {
    PreviewSettings {
        origin: "http://127.0.0.1:5281".into(),
        render_origin: "http://127.0.0.1:5281".into(),
        secret: [3; 32],
        ttl_secs: 600,
    }
}

async fn send(engine: Engine, request: Request<Body>) -> (StatusCode, Vec<u8>, Option<String>) {
    let response = board_api::router(engine).oneshot(request).await.unwrap();
    let status = response.status();
    let commit = response
        .headers()
        .get("x-board-commit")
        .map(|v| v.to_str().unwrap().to_owned());
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, bytes.to_vec(), commit)
}

async fn post(engine: Engine, path: &str, body: &Value) -> (StatusCode, Value) {
    let request = Request::post(path)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let (status, bytes, _) = send(engine, request).await;
    (status, serde_json::from_slice(&bytes).unwrap())
}

fn b64(text: &str) -> String {
    STANDARD.encode(text)
}

async fn call(engine: Engine, path: &str) -> (StatusCode, Value) {
    let (status, bytes, _) = send(engine, Request::get(path).body(Body::empty()).unwrap()).await;
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn import_checkout_push_versions_diff() {
    let engine = engine(true);
    let import = json!({
        "key": "web",
        "title": "Web",
        "import": { "source": "test", "files": [
            { "path": "src/a.ts", "content": b64("export const a = 1;\n") },
            { "path": "run.sh", "content": b64("#!/bin/sh\n"), "executable": true }
        ]}
    });
    let (status, created) = post(engine.clone(), "/api/projects", &import).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["project"]["origin"], "import");
    assert_eq!(created["version"]["number"], 1);
    let v1 = created["version"]["commit"].as_str().unwrap().to_owned();

    let (status, again) = post(engine.clone(), "/api/projects", &json!({ "key": "web" })).await;
    assert_eq!(
        (status, again["code"].as_str()),
        (StatusCode::CONFLICT, Some("exists"))
    );

    let (status, bytes, commit) = send(
        engine.clone(),
        Request::get("/api/projects/web/checkout")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(commit.as_deref(), Some(v1.as_str()));
    assert_eq!(&bytes[..2], &[0x1f, 0x8b]);

    let push = json!({ "base": v1, "message": "edit", "files": [
        { "path": "src/a.ts", "content": b64("export const a = 2;\n") },
        { "path": "run.sh" }
    ]});
    let (status, v2) = post(engine.clone(), "/api/projects/web/push", &push).await;
    assert_eq!(status, StatusCode::CREATED, "{v2}");
    assert_eq!(
        (v2["number"].as_u64(), v2["kind"].as_str()),
        (Some(2), Some("push"))
    );

    let (status, stale) = post(engine.clone(), "/api/projects/web/push", &push).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(stale["code"], "conflict");
    assert_eq!(stale["detail"]["head"], v2["commit"]);

    let (_, versions) = call(engine.clone(), "/api/projects/web/versions").await;
    assert_eq!(versions["versions"].as_array().unwrap().len(), 2);
    let (_, diff) = call(engine.clone(), "/api/projects/web/diff?from=v1").await;
    assert_eq!(
        diff,
        json!({ "changes": [
            { "path": "run.sh", "kind": "deleted" },
            { "path": "src/a.ts", "kind": "modified" }
        ]})
    );
    let (_, list) = call(engine.clone(), "/api/projects").await;
    assert_eq!(list["projects"][0]["key"], "web");
}

#[tokio::test]
async fn bad_requests() {
    let engine = engine(true);
    let (status, body) = post(engine.clone(), "/api/projects", &json!({ "key": "Bad" })).await;
    assert_eq!(
        (status, body["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("invalid"))
    );
    let traversal = json!({ "key": "x", "import": { "source": "t", "files": [
        { "path": "../etc/passwd", "content": b64("no") }
    ]}});
    let (status, _) = post(engine.clone(), "/api/projects", &traversal).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = call(engine.clone(), "/api/projects/missing").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(engine, "/api/projects/missing/checkout?rev=v1").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
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

#[tokio::test]
async fn preview_builds_and_serves_with_a_scoped_token() {
    let engine = engine(true).with_preview(preview_settings());
    let (status, _) = post(engine.clone(), "/api/projects", &json!({ "key": "web" })).await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, preview) = call(engine.clone(), "/api/projects/web/preview").await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["state"], "ready");
    let commit = preview["commit"].as_str().unwrap().to_owned();
    assert_eq!(
        preview["baseUrl"],
        format!("http://127.0.0.1:5281/web/{commit}/")
    );
    let token = preview["token"].as_str().unwrap().to_owned();

    let origin = board_api::PreviewOrigin {
        frame_ancestors: vec!["http://localhost:5280".into()],
    };
    let open = |path: String, cookie: Option<String>| {
        let router = board_api::preview_router(engine.clone(), &origin);
        let mut request = Request::get(path);
        if let Some(cookie) = cookie {
            request = request.header("cookie", cookie);
        }
        router.oneshot(request.body(Body::empty()).unwrap())
    };

    let page = open(format!("/web/{commit}/.board/frame.html?t={token}"), None)
        .await
        .unwrap();
    assert_eq!(page.status(), StatusCode::OK);
    let headers = page.headers();
    let cookie = headers["set-cookie"].to_str().unwrap().to_owned();
    assert!(
        cookie.starts_with(&format!("board_preview={token}; Path=/web/{commit}/;")),
        "{cookie}"
    );
    assert!(cookie.contains("HttpOnly") && cookie.contains("Partitioned"));
    assert_eq!(
        headers["content-security-policy"],
        "frame-ancestors http://localhost:5280"
    );
    assert_eq!(headers["referrer-policy"], "no-referrer");

    // Later requests carry the cookie instead of the token.
    let with_cookie = open(
        format!("/web/{commit}/.board/frame.html"),
        Some(format!("other=1; board_preview={token}")),
    )
    .await
    .unwrap();
    assert_eq!(with_cookie.status(), StatusCode::OK);
    assert!(with_cookie.headers().get("set-cookie").is_none());
    let missing = open(
        format!("/web/{commit}/nope.js"),
        Some(format!("board_preview={token}")),
    )
    .await
    .unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);

    // No token, a token for another commit, or the API on the preview origin: refused.
    let bare = open(format!("/web/{commit}/.board/frame.html"), None)
        .await
        .unwrap();
    assert_eq!(bare.status(), StatusCode::FORBIDDEN);
    let other = "0".repeat(40);
    let wrong = open(format!("/web/{other}/.board/frame.html?t={token}"), None)
        .await
        .unwrap();
    assert_eq!(wrong.status(), StatusCode::FORBIDDEN);
    let api = open("/api/projects".into(), None).await.unwrap();
    assert_eq!(api.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn preview_without_settings_is_unavailable() {
    let engine = engine(true);
    post(engine.clone(), "/api/projects", &json!({ "key": "web" })).await;
    let (status, body) = call(engine, "/api/projects/web/preview").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["code"], "unavailable");
}

#[tokio::test]
async fn check_capture_and_read_back() {
    let engine = engine(true).with_preview(preview_settings());
    post(engine.clone(), "/api/projects", &json!({ "key": "web" })).await;

    let (status, checked) = post(
        engine.clone(),
        "/api/projects/web/check",
        &json!({ "themes": ["light", "dark"] }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{checked}");
    assert_eq!(checked["results"][0]["issues"], json!([]));
    assert_eq!(
        checked["results"][1]["issues"][0],
        "[board] missing mock GET /api/x"
    );

    let (status, captured) = post(engine.clone(), "/api/projects/web/captures", &json!({})).await;
    assert_eq!(status, StatusCode::OK, "{captured}");
    assert_eq!(captured["results"][0]["captured"], true);
    let (_, again) = post(engine.clone(), "/api/projects/web/captures", &json!({})).await;
    assert_eq!(again["results"][0]["captured"], false);

    let (status, list) = call(engine.clone(), "/api/projects/web/captures").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["captures"][0]["target"], "home");
    assert_eq!(list["captures"][0]["stale"], false);
    assert_eq!(list["captures"][0]["sources"], json!(["src/home.tsx"]));

    let (status, image, _) = send(
        engine.clone(),
        Request::get("/api/projects/web/captures/screen/home/light/image")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(image.starts_with(&[0xff, 0xd8]));
    let (status, elements) = call(
        engine.clone(),
        "/api/projects/web/captures/screen/home/light/elements",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(elements[0]["chain"], json!(["Home"]));
    let (status, _) = call(
        engine.clone(),
        "/api/projects/web/captures/screen/home/dark/image",
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, body) = post(
        engine,
        "/api/projects/web/check",
        &json!({ "targets": ["ghost"] }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}
