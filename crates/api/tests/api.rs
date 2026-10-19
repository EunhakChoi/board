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
            accounts: db.clone(),
            notices: db.clone(),
            invites: db,
            webhooks: Arc::new(TeapotSender),
            realtime: Arc::new(board_collab::Hub::new()),
        },
        local_mode,
    )
    .with_secret([3; 32])
}

/// Every webhook receiver answers 418, so tests can tell a delivery happened.
struct TeapotSender;

impl board_engine::WebhookSender for TeapotSender {
    fn post(&self, _: &str, _: &[(&str, String)], _: &[u8]) -> Result<u16, String> {
        Ok(418)
    }
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
        assert_eq!(base, "./", "preview builds are relative");
        assert!(source.starts_with(&[0x1f, 0x8b]), "gzip source");
        Ok(Build {
            base: base.to_owned(),
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
    let token = preview["token"].as_str().unwrap().to_owned();
    assert_eq!(
        preview["baseUrl"],
        format!("http://127.0.0.1:5281/_preview/{token}/web/{commit}/")
    );

    let origin = board_api::PreviewOrigin {
        frame_ancestors: vec!["http://localhost:5280".into()],
        sandbox: false,
    };
    let open = |path: String| {
        let router = board_api::preview_router(engine.clone(), &origin);
        router.oneshot(Request::get(path).body(Body::empty()).unwrap())
    };

    let page = open(format!("/_preview/{token}/web/{commit}/.board/frame.html"))
        .await
        .unwrap();
    assert_eq!(page.status(), StatusCode::OK);
    let headers = page.headers();
    assert!(
        headers.get("set-cookie").is_none(),
        "the token is in the path"
    );
    assert_eq!(
        headers["content-security-policy"],
        "frame-ancestors http://localhost:5280"
    );
    assert_eq!(headers["referrer-policy"], "no-referrer");
    let missing = open(format!("/_preview/{token}/web/{commit}/nope.js"))
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);

    // A bad token, a token for another commit, or the API on the preview origin: refused.
    let bad = open(format!("/_preview/x.y/web/{commit}/.board/frame.html"))
        .await
        .unwrap();
    assert_eq!(bad.status(), StatusCode::FORBIDDEN);
    let other = "0".repeat(40);
    let wrong = open(format!("/_preview/{token}/web/{other}/.board/frame.html"))
        .await
        .unwrap();
    assert_eq!(wrong.status(), StatusCode::FORBIDDEN);
    let api = open("/api/projects".into()).await.unwrap();
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
            identity: Some(idp.clone()),
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

/// A JSON request with the session cookie and the client header.
fn as_member(method: &str, path: &str, cookie: &str, body: Option<Value>) -> Request<Body> {
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header("cookie", cookie)
        .header("x-board-client", "test")
        .header("content-type", "application/json");
    request
        .body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
        .unwrap()
}

async fn json_of(engine: &Engine, request: Request<Body>) -> (StatusCode, Value) {
    let (status, body, _) = send(engine.clone(), request).await;
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

#[tokio::test]
async fn admins_manage_the_allowlist_and_access_requests() {
    let (engine, idp) = signed_in_engine();
    let (boss, _) = browser_sign_in(&engine, &idp, "boss", "").await;
    let boss = boss.unwrap();
    let (eve, _) = browser_sign_in(&engine, &idp, "eve", "").await;
    let eve = eve.unwrap();

    let (status, rule) = json_of(
        &engine,
        as_member(
            "POST",
            "/api/admin/allow-rules",
            &boss,
            Some(json!({"kind": "domain", "value": "Team.example"})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(rule["value"], "team.example");
    let (status, _) = json_of(
        &engine,
        as_member(
            "POST",
            "/api/admin/allow-rules",
            &boss,
            Some(json!({"kind": "domain", "value": "nodot"})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, list) = json_of(
        &engine,
        as_member("GET", "/api/admin/allow-rules", &boss, None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["rules"].as_array().unwrap().len(), 1);

    // eve waits; members cannot see the admin routes.
    let (status, _) = json_of(&engine, as_member("GET", "/api/admin/users", &eve, None)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (_, users) = json_of(&engine, as_member("GET", "/api/admin/users", &boss, None)).await;
    let pending = users["users"]
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["status"] == "pending")
        .unwrap()
        .clone();
    assert_eq!(pending["email"], "eve@example.com");
    let id = pending["id"].as_str().unwrap();
    let (status, user) = json_of(
        &engine,
        as_member(
            "POST",
            &format!("/api/admin/users/{id}/approve"),
            &boss,
            None,
        ),
    )
    .await;
    assert_eq!(
        (status, user["status"].as_str()),
        (StatusCode::OK, Some("active"))
    );
    let (status, _) = json_of(&engine, as_member("GET", "/api/projects", &eve, None)).await;
    assert_eq!(status, StatusCode::OK, "approved: eve is in at once");

    let (status, user) = json_of(
        &engine,
        as_member(
            "PATCH",
            &format!("/api/admin/users/{id}"),
            &boss,
            Some(json!({"status": "removed"})),
        ),
    )
    .await;
    assert_eq!(
        (status, user["status"].as_str()),
        (StatusCode::OK, Some("removed"))
    );
    let (status, _) = json_of(&engine, as_member("GET", "/api/projects", &eve, None)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "removal ends the session");

    let rule_id = rule["id"].as_i64().unwrap();
    let (status, _, _) = send(
        engine.clone(),
        as_member(
            "DELETE",
            &format!("/api/admin/allow-rules/{rule_id}"),
            &boss,
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn share_links_let_guests_into_one_project() {
    let (engine, idp) = signed_in_engine();
    let (boss, _) = browser_sign_in(&engine, &idp, "boss", "").await;
    let boss = boss.unwrap();
    for key in ["web", "other"] {
        let (status, _) = json_of(
            &engine,
            as_member(
                "POST",
                "/api/projects",
                &boss,
                Some(json!({"key": key, "title": key})),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }
    let (status, created) = json_of(
        &engine,
        as_member(
            "POST",
            "/api/projects/web/share-links",
            &boss,
            Some(json!({"role": "commenter", "label": "review"})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let token = created["token"].as_str().unwrap().to_owned();
    assert_eq!(created["url"], format!("https://board.test/l/{token}"));

    let (status, shared) = call(engine.clone(), &format!("/api/links/{token}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        (shared["projectTitle"].as_str(), shared["role"].as_str()),
        (Some("web"), Some("commenter"))
    );

    let response = board_api::router(engine.clone())
        .oneshot(
            Request::post(format!("/api/links/{token}/enter"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"Dana"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let guest = response.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();

    let (_, me) = json_of(&engine, as_member("GET", "/api/auth/me", &guest, None)).await;
    assert_eq!(
        (me["kind"].as_str(), me["project"].as_str()),
        (Some("guest"), Some("web"))
    );
    let (_, projects) = json_of(&engine, as_member("GET", "/api/projects", &guest, None)).await;
    let keys: Vec<_> = projects["projects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["key"].clone())
        .collect();
    assert_eq!(keys, [json!("web")], "a guest sees only the link's project");
    let (status, _) = json_of(
        &engine,
        as_member("GET", "/api/projects/other", &guest, None),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = json_of(
        &engine,
        as_member("GET", "/api/projects/web/share-links", &guest, None),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let id = created["link"]["id"].as_str().unwrap();
    let (status, _, _) = send(
        engine.clone(),
        as_member(
            "POST",
            &format!("/api/projects/web/share-links/{id}/revoke"),
            &boss,
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = json_of(&engine, as_member("GET", "/api/projects", &guest, None)).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "revoked: the guest is out"
    );
    let (status, _) = call(engine, &format!("/api/links/{token}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn device_login_issues_a_cli_token() {
    let (engine, idp) = signed_in_engine();
    let (boss, _) = browser_sign_in(&engine, &idp, "boss", "").await;
    let boss = boss.unwrap();
    let (status, started) = post(
        engine.clone(),
        "/api/auth/device",
        &json!({"name": "laptop"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let code = started["userCode"].as_str().unwrap().to_owned();
    assert_eq!(
        started["verificationUrlComplete"],
        format!("https://board.test/device?code={code}")
    );
    let device = json!({"deviceCode": started["deviceCode"]});
    let (status, pending) = post(engine.clone(), "/api/auth/device/token", &device).await;
    assert_eq!(
        (status, pending["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("authorization_pending"))
    );

    let (status, request) = json_of(
        &engine,
        as_member("GET", &format!("/api/auth/device/{code}"), &boss, None),
    )
    .await;
    assert_eq!(
        (status, request["name"].as_str()),
        (StatusCode::OK, Some("laptop"))
    );
    let (status, _, _) = send(
        engine.clone(),
        as_member(
            "POST",
            &format!("/api/auth/device/{code}/approve"),
            &boss,
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, issued) = post(engine.clone(), "/api/auth/device/token", &device).await;
    assert_eq!(status, StatusCode::OK);
    let token = issued["token"].as_str().unwrap().to_owned();

    // The token acts as boss, without the client header.
    let bearer = |path: &str| {
        Request::get(path.to_owned())
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap()
    };
    let (status, body, _) = send(engine.clone(), bearer("/api/auth/me")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap()["email"],
        "boss@example.com"
    );
    let created = Request::post("/api/projects")
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"key":"cli","title":"CLI"}"#))
        .unwrap();
    let (status, _, _) = send(engine.clone(), created).await;
    assert_eq!(status, StatusCode::CREATED);

    let (_, tokens) = json_of(&engine, as_member("GET", "/api/me/cli-tokens", &boss, None)).await;
    assert_eq!(tokens["tokens"][0]["via"], "device");

    let logout = Request::post("/api/auth/logout")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let (status, _, _) = send(engine.clone(), logout).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, body, _) = send(engine, bearer("/api/projects")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap()["code"],
        "invalid_token"
    );
}

#[tokio::test]
async fn system_answers_before_sign_in_without_details() {
    let (engine, _) = signed_in_engine();
    let (status, system) = call(engine, "/api/system").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(system["localMode"], false);
    assert!(system["services"]["workspace"].get("error").is_none());
    assert_eq!(
        system["signIn"],
        json!({"provider": "scripted", "callbackUrl": "", "ok": true, "hasAdmin": false})
    );
}

#[tokio::test]
async fn private_projects_hide_from_non_members() {
    let (engine, idp) = signed_in_engine();
    let (boss, _) = browser_sign_in(&engine, &idp, "boss", "").await;
    let boss = boss.unwrap();
    json_of(
        &engine,
        as_member(
            "POST",
            "/api/admin/allow-rules",
            &boss,
            Some(json!({"kind": "domain", "value": "example.com"})),
        ),
    )
    .await;
    let (ann, _) = browser_sign_in(&engine, &idp, "ann", "").await;
    let ann = ann.unwrap();
    json_of(
        &engine,
        as_member(
            "POST",
            "/api/projects",
            &boss,
            Some(json!({"key": "secret", "title": "Secret"})),
        ),
    )
    .await;
    let (status, project) = json_of(
        &engine,
        as_member(
            "PATCH",
            "/api/projects/secret",
            &boss,
            Some(json!({"visibility": "private"})),
        ),
    )
    .await;
    assert_eq!(
        (status, project["visibility"].as_str()),
        (StatusCode::OK, Some("private"))
    );
    let (status, _) = json_of(
        &engine,
        as_member("GET", "/api/projects/secret", &ann, None),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (_, me) = json_of(&engine, as_member("GET", "/api/auth/me", &ann, None)).await;
    let ann_id = me["id"].as_str().unwrap();
    let (status, _, _) = send(
        engine.clone(),
        as_member(
            "PUT",
            &format!("/api/projects/secret/members/{ann_id}"),
            &boss,
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, members) = json_of(
        &engine,
        as_member("GET", "/api/projects/secret/members", &boss, None),
    )
    .await;
    assert_eq!(members["members"], json!([ann_id]));
    let (status, _) = json_of(
        &engine,
        as_member("GET", "/api/projects/secret", &ann, None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = json_of(
        &engine,
        as_member(
            "PATCH",
            "/api/projects/secret",
            &ann,
            Some(json!({"title": "Mine"})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn realtime_room_relays_presence_and_events() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    async fn read(
        socket: &mut (
                 impl StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin
             ),
    ) -> Value {
        loop {
            if let Message::Text(text) = socket.next().await.unwrap().unwrap() {
                return serde_json::from_str(&text).unwrap();
            }
        }
    }

    let engine = engine(true);
    engine
        .create_project(&board_model::Actor::Local, "web", "Web", None)
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(axum::serve(listener, board_api::router(engine.clone())).into_future());
    let url = format!("ws://{addr}/api/ws?project=web");
    let (mut ann, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    let (mut bob, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    assert_eq!(read(&mut ann).await["type"], "hello");
    assert_eq!(read(&mut bob).await["peers"].as_array().unwrap().len(), 1);
    assert_eq!(read(&mut ann).await["type"], "join");

    bob.send(Message::Text(
        r#"{"type":"presence","data":{"cursor":[3,4]}}"#.into(),
    ))
    .await
    .unwrap();
    let presence = read(&mut ann).await;
    assert_eq!(presence["data"]["cursor"], json!([3, 4]));

    // A command elsewhere reaches both as an event.
    engine
        .update_project(&board_model::Actor::Local, "web", Some("Web 2"), None)
        .unwrap();
    let event = read(&mut ann).await;
    assert_eq!(
        (event["kind"].as_str(), event["seq"].as_u64()),
        (Some("project.updated"), Some(2)),
        "version 1 was event 1"
    );
    assert_eq!(read(&mut bob).await["kind"], "project.updated");

    // Pages on other sites cannot open the channel with the visitor's cookies.
    let mut request = url.as_str().into_client_request().unwrap();
    request
        .headers_mut()
        .insert("origin", "https://evil.test".parse().unwrap());
    assert!(tokio_tungstenite::connect_async(request).await.is_err());
}

/// Boss (admin) and Ann (editor) signed in, and a project `web`; returns their cookies.
async fn boss_and_ann() -> (Engine, String, String) {
    let (engine, idp) = signed_in_engine();
    let (boss, _) = browser_sign_in(&engine, &idp, "boss", "").await;
    let boss = boss.unwrap();
    json_of(
        &engine,
        as_member(
            "POST",
            "/api/admin/allow-rules",
            &boss,
            Some(json!({"kind": "domain", "value": "example.com"})),
        ),
    )
    .await;
    let (ann, _) = browser_sign_in(&engine, &idp, "ann", "").await;
    let ann = ann.unwrap();
    json_of(
        &engine,
        as_member(
            "POST",
            "/api/projects",
            &boss,
            Some(json!({"key": "web", "title": "Web"})),
        ),
    )
    .await;
    (engine, boss, ann)
}

#[tokio::test]
async fn inbox_gets_comments_and_marks_read() {
    let (engine, boss, ann) = boss_and_ann().await;
    let (status, _) = json_of(
        &engine,
        as_member(
            "POST",
            "/api/projects/web/comments",
            &boss,
            Some(
                json!({"kind": "screen", "target": "home", "at": {"x": 1, "y": 1}, "body": "Look"}),
            ),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, inbox) = json_of(
        &engine,
        as_member("GET", "/api/me/notifications?unread=true", &ann, None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(inbox["unread"], 1);
    assert_eq!(inbox["notifications"][0]["kind"], "comment.added");
    assert_eq!(
        inbox["notifications"][0]["text"],
        "boss commented on screen home: Look"
    );
    let (_, read) = json_of(
        &engine,
        as_member("POST", "/api/me/notifications/read", &ann, Some(json!({}))),
    )
    .await;
    assert_eq!(read["unread"], 0);
}

#[tokio::test]
async fn webhooks_are_admin_settings_with_a_secret_shown_once() {
    let (engine, boss, ann) = boss_and_ann().await;
    // Webhooks are for admins; the secret comes back once.
    let (status, _) = json_of(&engine, as_member("GET", "/api/admin/webhooks", &ann, None)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, created) = json_of(
        &engine,
        as_member(
            "POST",
            "/api/admin/webhooks",
            &boss,
            Some(json!({"url": "https://hooks.test/x", "events": ["comment.added"]})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let secret = created["secret"].as_str().unwrap();
    let id = created["webhook"]["id"].as_str().unwrap().to_owned();
    assert!(secret.ends_with(created["webhook"]["secretLast4"].as_str().unwrap()));
    let (status, tested) = json_of(
        &engine,
        as_member(
            "POST",
            &format!("/api/admin/webhooks/{id}/test"),
            &boss,
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(tested["lastStatus"], 418);
    let (_, list) = json_of(
        &engine,
        as_member("GET", "/api/admin/webhooks", &boss, None),
    )
    .await;
    assert_eq!(list["webhooks"].as_array().unwrap().len(), 1);
    assert!(list["webhooks"][0].get("secret").is_none());
    assert!(
        list["events"]
            .as_array()
            .unwrap()
            .contains(&json!("version.created"))
    );
    let (status, _) = json_of(
        &engine,
        as_member("DELETE", &format!("/api/admin/webhooks/{id}"), &boss, None),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

/// The cookie a response set, as `name=value`.
fn set_cookie(headers: &axum::http::HeaderMap) -> String {
    headers["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned()
}

async fn post_raw(engine: &Engine, path: &str, body: Value) -> axum::response::Response {
    board_api::router(engine.clone())
        .oneshot(
            Request::post(path)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn invite_links_without_a_provider() {
    let engine = engine(false)
        .with_tool_url("https://board.test")
        .with_sign_in(board_engine::SignInSettings {
            identity: None,
            admin_emails: Vec::new(),
        });
    let (_, system) = call(engine.clone(), "/api/system").await;
    assert_eq!(system["signIn"]["provider"], Value::Null);
    let setup = engine.issue_setup_token().unwrap().unwrap();
    let response = post_raw(
        &engine,
        "/api/auth/setup",
        json!({"token": setup, "name": "Boss"}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let boss = set_cookie(response.headers());
    let (_, me) = json_of(&engine, as_member("GET", "/api/auth/me", &boss, None)).await;
    assert_eq!(
        (me["name"].as_str(), me["role"].as_str()),
        (Some("Boss"), Some("admin"))
    );
    assert!(me.get("email").is_none());

    let (status, made) = json_of(
        &engine,
        as_member(
            "POST",
            "/api/admin/invites",
            &boss,
            Some(json!({"role": "viewer", "label": "Dana"})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let url = made["url"].as_str().unwrap();
    let token = url.strip_prefix("https://board.test/i/").unwrap();
    let (status, view) = call(engine.clone(), &format!("/api/invites/{token}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view, json!({"kind": "join", "role": "viewer"}));
    let response = post_raw(
        &engine,
        &format!("/api/invites/{token}/accept"),
        json!({"name": "Dana"}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let dana = set_cookie(response.headers());
    let (_, me) = json_of(&engine, as_member("GET", "/api/auth/me", &dana, None)).await;
    assert_eq!(me["role"], "viewer");
    let again = post_raw(
        &engine,
        &format!("/api/invites/{token}/accept"),
        json!({"name": "Eve"}),
    )
    .await;
    assert_eq!(again.status(), StatusCode::NOT_FOUND);

    // Dana's link for her phone opens her account.
    let (status, own) = json_of(
        &engine,
        as_member("POST", "/api/me/sign-in-link", &dana, None),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let token = own["url"]
        .as_str()
        .unwrap()
        .rsplit('/')
        .next()
        .unwrap()
        .to_owned();
    let (_, view) = call(engine.clone(), &format!("/api/invites/{token}")).await;
    assert_eq!(view["kind"], "sign-in");
    assert_eq!(view["name"], "Dana");
    let (status, _) = json_of(&engine, as_member("GET", "/api/admin/invites", &dana, None)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = redirect_of(
        engine.clone(),
        Request::get("/api/auth/oidc/start")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn one_address_serves_previews_sandboxed_from_the_tool() {
    let mut settings = preview_settings();
    settings.origin = "https://board.test".into();
    let engine = engine(true)
        .with_tool_url("https://board.test")
        .with_preview(settings);
    assert!(engine.previews_on_tool());
    let (status, _) = post(engine.clone(), "/api/projects", &json!({ "key": "web" })).await;
    assert_eq!(status, StatusCode::CREATED);
    let (_, preview) = call(engine.clone(), "/api/projects/web/preview").await;
    let commit = preview["commit"].as_str().unwrap().to_owned();
    let token = preview["token"].as_str().unwrap();
    assert_eq!(
        preview["baseUrl"],
        format!("https://board.test/_preview/{token}/web/{commit}/")
    );
    let page = board_api::app(engine.clone(), None)
        .oneshot(
            Request::get(format!("/_preview/{token}/web/{commit}/.board/frame.html"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(page.status(), StatusCode::OK);
    let csp = page.headers()["content-security-policy"].to_str().unwrap();
    assert!(csp.starts_with("sandbox allow-scripts"), "{csp}");
    assert!(!csp.contains("allow-same-origin"), "{csp}");
    assert!(csp.ends_with("frame-ancestors 'self'"), "{csp}");
    assert_eq!(page.headers()["access-control-allow-origin"], "*");
    // The API still answers next to it.
    let (status, _) = call(engine, "/api/health").await;
    assert_eq!(status, StatusCode::OK);
}
