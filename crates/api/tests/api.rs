//! Black-box tests of the router, plus the contract check.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use board_db::{CaptureStore, CommentStore, Db};
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
            captures: Arc::new(CaptureStore::new(db.clone(), root.join("captures"))),
            comments: Arc::new(CommentStore::new(db.clone(), root.join("comments"))),
            accounts: db,
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
async fn delete_project() {
    let engine = engine(true);
    let (status, _) = post(engine.clone(), "/api/projects", &json!({ "key": "web" })).await;
    assert_eq!(status, StatusCode::CREATED);
    let delete = || {
        Request::delete("/api/projects/web")
            .body(Body::empty())
            .unwrap()
    };
    let (status, _, _) = send(engine.clone(), delete()).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = call(engine.clone(), "/api/projects/web").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, _) = send(engine, delete()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
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

#[tokio::test]
async fn system_reports_services() {
    let engine = engine(true).with_tool_url("http://localhost:5280/");
    let (status, body) = call(engine, "/api/system").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["toolUrl"], "http://localhost:5280");
    assert_eq!(body["localMode"], true);
    assert_eq!(body["services"]["workspace"]["ok"], true);
    assert_eq!(body["services"]["render"]["ok"], true);
}

#[tokio::test]
async fn sheet_of_placeholders() {
    let engine = engine(true).with_preview(preview_settings());
    post(engine.clone(), "/api/projects", &json!({ "key": "web" })).await;
    let (status, image, _) = send(
        engine.clone(),
        Request::get("/api/projects/web/sheet?items=planned,screen:ghost&theme=dark")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(image.starts_with(&[0xff, 0xd8]));
    let (status, body) = call(engine, "/api/projects/web/sheet?items=page:x").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn comment_thread() {
    let engine = engine(true).with_preview(preview_settings());
    post(engine.clone(), "/api/projects", &json!({ "key": "web" })).await;
    post(engine.clone(), "/api/projects/web/captures", &json!({})).await;

    let (status, comment) = post(
        engine.clone(),
        "/api/projects/web/comments",
        &json!({ "kind": "screen", "target": "home", "at": { "x": 5, "y": 5 }, "body": "Hi" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{comment}");
    assert_eq!(comment["anchor"]["src"], "src/home.tsx:1:1");
    assert_eq!(comment["status"], "open");
    let id = comment["id"].as_u64().unwrap();

    let (status, replied) = post(
        engine.clone(),
        &format!("/api/projects/web/comments/{id}/replies"),
        &json!({ "rev": 1, "body": "Done?" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{replied}");
    let (status, stale) = post(
        engine.clone(),
        &format!("/api/projects/web/comments/{id}/resolve"),
        &json!({ "rev": 1 }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(stale["code"], "changed");
    assert_eq!(stale["detail"]["rev"], 2);
    let (status, resolved) = post(
        engine.clone(),
        &format!("/api/projects/web/comments/{id}/resolve"),
        &json!({ "rev": 2, "body": "fixed in v2" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(resolved["status"], "resolved");

    let (_, open) = call(engine.clone(), "/api/projects/web/comments?status=open").await;
    assert_eq!(open["comments"], json!([]));
    let (_, all) = call(engine.clone(), "/api/projects/web/comments").await;
    assert_eq!(all["comments"][0]["replies"].as_array().unwrap().len(), 2);
    let (status, image, _) = send(
        engine.clone(),
        Request::get(format!("/api/projects/web/comments/{id}/snapshot"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(image.starts_with(&[0xff, 0xd8]));
    let (status, _) = call(engine, "/api/projects/web/comments?status=nope").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn manifest_files_and_web() {
    let engine = engine(true).with_preview(preview_settings());
    let import = json!({
        "key": "web",
        "import": { "source": "t", "files": [{ "path": "src/a.tsx", "content": b64("export {}\n") }] }
    });
    post(engine.clone(), "/api/projects", &import).await;
    let (status, manifest) = call(engine.clone(), "/api/projects/web/manifest").await;
    assert_eq!(status, StatusCode::OK, "{manifest}");
    assert_eq!(manifest["manifest"]["screens"][0]["id"], "home");
    let (_, files) = call(engine.clone(), "/api/projects/web/files").await;
    assert_eq!(files["paths"], json!(["src/a.tsx"]));
    let (status, body, commit) = send(
        engine.clone(),
        Request::get("/api/projects/web/file?path=src/a.tsx")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, b"export {}\n");
    assert_eq!(commit.as_deref(), files["commit"].as_str());

    // The app: API paths stay JSON 404s, other paths fall back to the web tool.
    let web = std::env::temp_dir().join(format!("board-web-{}", std::process::id()));
    std::fs::create_dir_all(web.join("assets")).unwrap();
    std::fs::write(web.join("index.html"), "<!doctype html>app").unwrap();
    std::fs::write(web.join("assets/a.js"), "x").unwrap();
    let app = |path: &str| {
        board_api::app(engine.clone(), Some(web.clone()))
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
    };
    let page = app("/p/web/screens").await.unwrap();
    assert_eq!(page.status(), StatusCode::OK);
    assert_eq!(page.headers()["cache-control"], "no-cache");
    let asset = app("/assets/a.js").await.unwrap();
    assert_eq!(
        asset.headers()["content-type"],
        "text/javascript; charset=utf-8"
    );
    assert!(
        asset.headers()["cache-control"]
            .to_str()
            .unwrap()
            .contains("immutable")
    );
    let api = app("/api/nope").await.unwrap();
    assert_eq!(api.status(), StatusCode::NOT_FOUND);
    std::fs::remove_dir_all(&web).unwrap();
}

/// An identity provider that signs in whoever the test names next.
#[derive(Default)]
struct ScriptedIdentity {
    state: std::sync::Mutex<String>,
    next: std::sync::Mutex<Option<String>>,
}

impl board_engine::Identity for ScriptedIdentity {
    fn provider(&self) -> &'static str {
        "scripted"
    }
    fn authorize_url(&self, request: &board_engine::AuthRequest) -> Result<String, WorkspaceError> {
        self.state.lock().unwrap().clone_from(&request.state);
        Ok(format!("https://idp.test/auth?state={}", request.state))
    }
    fn exchange(&self, _: &str, _: &str, _: &str) -> Result<board_engine::Claims, WorkspaceError> {
        let who = self.next.lock().unwrap().take().unwrap();
        Ok(board_engine::Claims {
            subject: who.clone(),
            email: format!("{who}@example.com"),
            email_verified: true,
            name: Some(who),
            picture: None,
        })
    }
}

fn signed_in_engine() -> (Engine, Arc<ScriptedIdentity>) {
    let idp = Arc::new(ScriptedIdentity::default());
    let engine = engine(false)
        .with_tool_url("https://board.test")
        .with_sign_in(board_engine::SignInSettings {
            identity: idp.clone(),
            admin_emails: vec!["boss@example.com".into()],
        });
    (engine, idp)
}

/// Runs the browser side of a sign-in as `who`; returns the session cookie value (if any) and
/// where the callback redirected.
async fn browser_sign_in(
    engine: &Engine,
    idp: &ScriptedIdentity,
    who: &str,
    query: &str,
) -> (Option<String>, String) {
    let (status, location) = redirect_of(
        engine.clone(),
        Request::get(format!("/api/auth/oidc/start{query}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FOUND);
    assert!(location.unwrap().starts_with("https://idp.test/auth"));
    *idp.next.lock().unwrap() = Some(who.into());
    let state = idp.state.lock().unwrap().clone();
    let response = board_api::router(engine.clone())
        .oneshot(
            Request::get(format!("/api/auth/oidc/callback?state={state}&code=c"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FOUND);
    let location = response.headers()["location"].to_str().unwrap().to_owned();
    let cookie = response.headers().get("set-cookie").map(|c| {
        let text = c.to_str().unwrap();
        assert!(text.contains("HttpOnly") && text.contains("Secure"));
        text.split(';').next().unwrap().to_owned()
    });
    (cookie, location)
}

#[tokio::test]
async fn sign_in_session_csrf_and_logout() {
    let (engine, idp) = signed_in_engine();
    let (cookie, location) = browser_sign_in(&engine, &idp, "boss", "?returnTo=%2Fp%2Fweb").await;
    let cookie = cookie.unwrap();
    assert_eq!(location, "/p/web");
    let me = board_api::router(engine.clone())
        .oneshot(
            Request::get("/api/auth/me")
                .header("cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(me.status(), StatusCode::OK);
    let body = http_body_util::BodyExt::collect(me.into_body())
        .await
        .unwrap()
        .to_bytes();
    let me: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        (me["kind"].as_str(), me["role"].as_str()),
        (Some("user"), Some("admin"))
    );

    // Changing requests need the client header.
    let create = |with_header: bool| {
        let mut request = Request::post("/api/projects")
            .header("cookie", &cookie)
            .header("content-type", "application/json");
        if with_header {
            request = request.header("x-board-client", "web");
        }
        request
            .body(Body::from(r#"{"key":"web","title":"Web"}"#))
            .unwrap()
    };
    let (status, body, _) = send(engine.clone(), create(false)).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "{}",
        String::from_utf8_lossy(&body)
    );
    let (status, _, _) = send(engine.clone(), create(true)).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _, _) = send(
        engine.clone(),
        Request::post("/api/auth/logout")
            .header("cookie", &cookie)
            .header("x-board-client", "web")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _, _) = send(
        engine,
        Request::get("/api/projects")
            .header("cookie", &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn strangers_wait_and_nobody_gets_in_before_setup() {
    let (engine, idp) = signed_in_engine();
    let (cookie, location) = browser_sign_in(&engine, &idp, "eve", "").await;
    assert_eq!(
        (cookie, location.as_str()),
        (None, "/signin?error=setup_required")
    );

    let token = engine.issue_setup_token().unwrap().unwrap();
    let (cookie, _) = browser_sign_in(&engine, &idp, "ann", &format!("?setup={token}")).await;
    assert!(cookie.is_some(), "the setup link lets the first admin in");

    let (cookie, _) = browser_sign_in(&engine, &idp, "eve", "").await;
    let cookie = cookie.unwrap();
    let call = |path: &str| {
        Request::get(path.to_owned())
            .header("cookie", &cookie)
            .body(Body::empty())
            .unwrap()
    };
    let (status, body, _) = send(engine.clone(), call("/api/auth/me")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap()["kind"],
        "pending"
    );
    let (status, body, _) = send(engine, call("/api/projects")).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap()["code"],
        "pending"
    );
}

#[tokio::test]
async fn callback_without_a_code_goes_back_to_sign_in() {
    let (engine, _) = signed_in_engine();
    let (status, location) = redirect_of(
        engine,
        Request::get("/api/auth/oidc/callback?error=access_denied")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FOUND);
    assert_eq!(location.as_deref(), Some("/signin?error=cancelled"));
}

async fn redirect_of(engine: Engine, request: Request<Body>) -> (StatusCode, Option<String>) {
    let response = board_api::router(engine).oneshot(request).await.unwrap();
    let location = response
        .headers()
        .get("location")
        .map(|v| v.to_str().unwrap().to_owned());
    (response.status(), location)
}
