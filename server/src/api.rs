//! HTTP API used by the browser extension (see spec v2 § API).
//! Every route except `/health` requires the `X-Bridge-Token` header.

use axum::extract::{Path as UrlPath, Query, Request, State};
use axum::http::{header, HeaderName, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use log::warn;
use serde::{Deserialize, Serialize};
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::extractor::Mode;
use crate::jobs::{CancelError, Job};
use crate::worker::AppState;

pub const TOKEN_HEADER: &str = "x-bridge-token";

const ALLOWED_HOSTS: &[&str] = &[
    "youtube.com",
    "www.youtube.com",
    "m.youtube.com",
    "music.youtube.com",
    "youtu.be",
];

pub fn router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|origin: &HeaderValue, _| {
            let o = origin.as_bytes();
            o.starts_with(b"chrome-extension://") || o.starts_with(b"moz-extension://")
        }))
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([header::CONTENT_TYPE, HeaderName::from_static(TOKEN_HEADER)]);

    let protected = Router::new()
        .route("/jobs", get(list_jobs).post(submit).delete(clear_jobs))
        .route("/jobs/clear", post(clear_jobs))
        .route("/extract", post(submit_legacy))
        .route("/jobs/{id}", get(get_job))
        .route("/jobs/{id}/log", get(get_log))
        .route("/jobs/{id}/cancel", post(cancel_job))
        .route("/plex/scan", post(plex_scan))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_token));

    Router::new()
        .route("/health", get(health))
        .merge(protected)
        .with_state(state)
        // Outermost, so CORS preflight is answered before token auth runs.
        .layer(cors)
}

/// Constant-time comparison so the token can't be guessed byte-by-byte via timing.
fn token_matches(expected: &str, given: &[u8]) -> bool {
    let expected = expected.as_bytes();
    expected.len() == given.len() && expected.iter().zip(given).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
}

async fn require_token(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let ok = req
        .headers()
        .get(TOKEN_HEADER)
        .is_some_and(|v| token_matches(&state.cfg.bridge_token, v.as_bytes()));
    if !ok {
        warn!("Rejected request to {} (missing or bad token)", req.uri().path());
        return error_response(StatusCode::UNAUTHORIZED, "Missing or invalid token.");
    }
    next.run(req).await
}

fn error_response(status: StatusCode, msg: &str) -> Response {
    (status, Json(serde_json::json!({ "error": msg }))).into_response()
}

#[derive(Serialize)]
struct Health {
    ok: bool,
    engine_ready: bool,
    engine_version: Option<String>,
    version: &'static str,
    queued: usize,
    running: usize,
}

async fn health(State(state): State<AppState>) -> Json<Health> {
    let (queued, running) = state.jobs.counts();
    Json(Health {
        ok: true,
        engine_ready: state.engine.is_ready(),
        engine_version: state.engine.version(),
        version: env!("CARGO_PKG_VERSION"),
        queued,
        running,
    })
}

/// Returns the normalized URL if it is an http(s) YouTube / YouTube Music link.
pub fn validate_url(raw: &str) -> Result<String, &'static str> {
    let parsed = url::Url::parse(raw.trim()).map_err(|_| "Not a valid URL.")?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("URL must use http or https.");
    }
    let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
    if !ALLOWED_HOSTS.contains(&host.as_str()) {
        return Err("Only YouTube and YouTube Music URLs are supported.");
    }
    Ok(parsed.to_string())
}

#[derive(Deserialize)]
struct SubmitBody {
    url: String,
    #[serde(default)]
    mode: Mode,
}

async fn submit(State(state): State<AppState>, Json(body): Json<SubmitBody>) -> Response {
    let url = match validate_url(&body.url) {
        Ok(u) => u,
        Err(msg) => return error_response(StatusCode::BAD_REQUEST, msg),
    };
    let job = state.jobs.submit(url, body.mode);
    (StatusCode::ACCEPTED, Json(job)).into_response()
}

#[derive(Serialize)]
struct JobList {
    jobs: Vec<Job>,
}

async fn list_jobs(State(state): State<AppState>) -> Json<JobList> {
    Json(JobList { jobs: state.jobs.list() })
}

async fn get_job(State(state): State<AppState>, UrlPath(id): UrlPath<u64>) -> Response {
    match state.jobs.get(id) {
        Some(job) => Json(job).into_response(),
        None => error_response(StatusCode::NOT_FOUND, "Unknown job id."),
    }
}

#[derive(Deserialize)]
struct LogQuery {
    #[serde(default)]
    since: u64,
}

async fn get_log(State(state): State<AppState>, UrlPath(id): UrlPath<u64>, Query(q): Query<LogQuery>) -> Response {
    match state.jobs.log_since(id, q.since) {
        Some(page) => Json(page).into_response(),
        None => error_response(StatusCode::NOT_FOUND, "Unknown job id."),
    }
}

async fn cancel_job(State(state): State<AppState>, UrlPath(id): UrlPath<u64>) -> Response {
    match state.jobs.cancel(id) {
        Ok(status) => Json(serde_json::json!({ "status": status })).into_response(),
        Err(CancelError::NotFound) => error_response(StatusCode::NOT_FOUND, "Unknown job id."),
        Err(CancelError::AlreadyFinished(_)) => error_response(StatusCode::CONFLICT, "Job already finished."),
    }
}

/// Backwards-compatible alias for `POST /jobs` returning `{ job_id, id }`.
async fn submit_legacy(State(state): State<AppState>, Json(body): Json<SubmitBody>) -> Response {
    let url = match validate_url(&body.url) {
        Ok(u) => u,
        Err(msg) => return error_response(StatusCode::BAD_REQUEST, msg),
    };
    let job = state.jobs.submit(url, body.mode);
    (StatusCode::ACCEPTED, Json(serde_json::json!({ "job_id": job.id, "id": job.id }))).into_response()
}

async fn plex_scan(State(state): State<AppState>) -> Response {
    match crate::plex::trigger_plex_refresh(&state.cfg.plex).await {
        Ok(()) => Json(serde_json::json!({ "ok": true, "message": "Plex library scan triggered." })).into_response(),
        Err(msg) => error_response(StatusCode::BAD_GATEWAY, &msg),
    }
}

async fn clear_jobs(State(state): State<AppState>) -> Response {
    let cleared = state.jobs.clear();
    Json(serde_json::json!({ "ok": true, "cleared": cleared })).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::engine::Engine;
    use crate::jobs::JobStore;
    use crate::worker::run_worker;
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use std::collections::HashMap;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;
    use tower::ServiceExt;

    const TOKEN: &str = "test-token-0123456789abcdef0123456789";

    struct TestApp {
        app: Router,
        dir: PathBuf,
    }

    impl Drop for TestApp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    /// Builds the app with a fake Engine script and (optionally) a running worker.
    fn test_app(name: &str, script: &str, worker: bool) -> TestApp {
        let dir = std::env::temp_dir().join(format!(
            "aes-api-{}-{}-{}",
            name,
            std::process::id(),
            crate::jobs::now_ms()
        ));
        fs::create_dir_all(dir.join("music")).unwrap();
        let engine = dir.join("fake-yt-dlp");
        fs::write(&engine, format!("#!/bin/sh\n{}\n", script)).unwrap();
        fs::set_permissions(&engine, fs::Permissions::from_mode(0o755)).unwrap();

        let env: HashMap<String, String> = [
            ("BRIDGE_TOKEN", TOKEN.to_string()),
            ("MUSIC_ROOT", dir.join("music").to_string_lossy().into_owned()),
            ("DATA_DIR", dir.join("data").to_string_lossy().into_owned()),
            ("ENGINE_PATH", engine.to_string_lossy().into_owned()),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        let cfg = Arc::new(Config::from_map(&env).unwrap());
        let engine = Arc::new(Engine::new(&cfg));
        engine.mark_ready();
        let state = AppState { cfg, engine, jobs: Arc::new(JobStore::in_memory()) };
        if worker {
            tokio::spawn(run_worker(state.clone()));
        }
        TestApp { app: router(state), dir }
    }

    fn req(method: &str, uri: &str, token: Option<&str>, body: Option<&str>) -> Request<Body> {
        let mut b = Request::builder().method(method).uri(uri);
        if let Some(t) = token {
            b = b.header(TOKEN_HEADER, t);
        }
        if body.is_some() {
            b = b.header(header::CONTENT_TYPE, "application/json");
        }
        b.body(body.map(|s| Body::from(s.to_string())).unwrap_or_default()).unwrap()
    }

    async fn call(app: &Router, r: Request<Body>) -> (StatusCode, serde_json::Value) {
        let resp = app.clone().oneshot(r).await.unwrap();
        let status = resp.status();
        let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null))
    }

    async fn submit(app: &Router, url: &str, mode: &str) -> u64 {
        let body = format!(r#"{{"url":"{}","mode":"{}"}}"#, url, mode);
        let (s, j) = call(app, req("POST", "/jobs", Some(TOKEN), Some(&body))).await;
        assert_eq!(s, StatusCode::ACCEPTED, "{j}");
        j["id"].as_u64().unwrap()
    }

    async fn wait_status(app: &Router, id: u64, want: &[&str]) -> serde_json::Value {
        for _ in 0..300 {
            let (_, job) = call(app, req("GET", &format!("/jobs/{id}"), Some(TOKEN), None)).await;
            if want.iter().any(|w| job["status"] == *w) {
                return job;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("job {id} never reached {want:?}");
    }

    #[tokio::test]
    async fn health_is_public() {
        let t = test_app("health", "exit 0", false);
        let (s, j) = call(&t.app, req("GET", "/health", None, None)).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(j["engine_ready"], true);
        assert_eq!(j["queued"], 0);
    }

    #[tokio::test]
    async fn rejects_missing_and_wrong_token() {
        let t = test_app("auth", "exit 0", false);
        for path in ["/jobs", "/jobs/1", "/jobs/1/log"] {
            let (s, _) = call(&t.app, req("GET", path, None, None)).await;
            assert_eq!(s, StatusCode::UNAUTHORIZED, "{path}");
            let (s, _) = call(&t.app, req("GET", path, Some("nope"), None)).await;
            assert_eq!(s, StatusCode::UNAUTHORIZED, "{path}");
        }
        let (s, _) = call(&t.app, req("POST", "/jobs/1/cancel", None, None)).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn cors_preflight_allows_extension_origin_only() {
        let t = test_app("cors", "exit 0", false);
        let preflight = |origin: &str| {
            Request::builder()
                .method("OPTIONS")
                .uri("/jobs")
                .header(header::ORIGIN, origin)
                .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
                .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "content-type,x-bridge-token")
                .body(Body::empty())
                .unwrap()
        };
        let r = t.app.clone().oneshot(preflight("chrome-extension://abcdefghijklmnop")).await.unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(r.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN).unwrap(), "chrome-extension://abcdefghijklmnop");
        let r = t.app.clone().oneshot(preflight("https://evil.example")).await.unwrap();
        assert!(r.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN).is_none());
    }

    #[tokio::test]
    async fn rejects_non_youtube_urls_and_bad_modes() {
        let t = test_app("badurl", "exit 0", false);
        let (s, _) = call(&t.app, req("POST", "/jobs", Some(TOKEN), Some(r#"{"url":"https://example.com/x"}"#))).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        let (s, _) = call(
            &t.app,
            req("POST", "/jobs", Some(TOKEN), Some(r#"{"url":"https://youtu.be/x","mode":"video"}"#)),
        )
        .await;
        assert!(s.is_client_error());
    }

    #[tokio::test]
    async fn unknown_job_is_404() {
        let t = test_app("404", "exit 0", false);
        for (m, p) in [("GET", "/jobs/999"), ("GET", "/jobs/999/log"), ("POST", "/jobs/999/cancel")] {
            let (s, _) = call(&t.app, req(m, p, Some(TOKEN), None)).await;
            assert_eq!(s, StatusCode::NOT_FOUND, "{m} {p}");
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn job_lifecycle_success_with_log_catch_up() {
        let t = test_app(
            "ok",
            "echo '[download] Downloading item 1 of 1'\n\
             echo 'YTDLM_TRACK:::/m/A/S/01 - T.m4a:::T:::A:::12:::NA:::NA:::NA'\n\
             exit 0",
            true,
        );
        let id = submit(&t.app, "https://music.youtube.com/watch?v=abc", "song").await;
        let job = wait_status(&t.app, id, &["succeeded", "failed"]).await;
        assert_eq!(job["status"], "succeeded", "{job}");
        assert_eq!(job["title"], "A - T");
        assert_eq!(job["tracks_done"], 1);
        assert_eq!(job["mode"], "song");
        assert_eq!(job["message"], "Saved 1 track.");

        // A client opening the panel after the fact sees the whole log.
        let (_, page) = call(&t.app, req("GET", &format!("/jobs/{id}/log?since=0"), Some(TOKEN), None)).await;
        let lines: Vec<&str> = page["lines"].as_array().unwrap().iter().map(|l| l["line"].as_str().unwrap()).collect();
        assert_eq!(lines, ["[download] Downloading item 1 of 1", "✓ Saved: A - T", "✅ Saved 1 track."]);
        assert_eq!(page["done"], true);
        let next = page["next"].as_u64().unwrap();
        let (_, page) = call(&t.app, req("GET", &format!("/jobs/{id}/log?since={next}"), Some(TOKEN), None)).await;
        assert!(page["lines"].as_array().unwrap().is_empty());

        let (_, list) = call(&t.app, req("GET", "/jobs", Some(TOKEN), None)).await;
        assert_eq!(list["jobs"][0]["id"], id);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn job_failure_reports_error() {
        let t = test_app("fail", "echo 'ERROR: Video unavailable' 1>&2\nexit 1", true);
        let id = submit(&t.app, "https://youtu.be/x", "playlist").await;
        let job = wait_status(&t.app, id, &["succeeded", "failed"]).await;
        assert_eq!(job["status"], "failed");
        assert!(job["message"].as_str().unwrap().contains("Extraction failed"));
        let (_, page) = call(&t.app, req("GET", &format!("/jobs/{id}/log"), Some(TOKEN), None)).await;
        assert_eq!(page["lines"][0]["line"], "ERROR: Video unavailable");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn jobs_run_in_order_and_can_be_cancelled() {
        let t = test_app("cancel", "echo started\nsleep 30", true);
        let first = submit(&t.app, "https://youtu.be/1", "song").await;
        let second = submit(&t.app, "https://youtu.be/2", "song").await;
        wait_status(&t.app, first, &["running"]).await;
        assert_eq!(wait_status(&t.app, second, &["queued"]).await["status"], "queued");

        // Cancel the queued one: immediate.
        let (s, j) = call(&t.app, req("POST", &format!("/jobs/{second}/cancel"), Some(TOKEN), None)).await;
        assert_eq!((s, j["status"].as_str()), (StatusCode::OK, Some("cancelled")));

        // Cancel the running one: the Engine is killed and the job ends as cancelled.
        let (s, j) = call(&t.app, req("POST", &format!("/jobs/{first}/cancel"), Some(TOKEN), None)).await;
        assert_eq!((s, j["status"].as_str()), (StatusCode::OK, Some("running")));
        let job = wait_status(&t.app, first, &["cancelled", "failed", "succeeded"]).await;
        assert_eq!(job["status"], "cancelled");

        let (s, _) = call(&t.app, req("POST", &format!("/jobs/{first}/cancel"), Some(TOKEN), None)).await;
        assert_eq!(s, StatusCode::CONFLICT);
    }

    #[test]
    fn url_validation() {
        assert!(validate_url("https://music.youtube.com/playlist?list=OLAK5uy_x").is_ok());
        assert!(validate_url("https://youtu.be/abc").is_ok());
        assert!(validate_url("https://www.youtube.com.evil.com/watch?v=1").is_err());
        assert!(validate_url("file:///etc/passwd").is_err());
        assert!(validate_url("not a url").is_err());
    }
}
