//! Browser Bridge (see CONTEXT.md): a localhost-only HTTP server that runs inside
//! the open Audio Extractor app so a browser extension can submit URLs and poll
//! job status. Jobs go through the same extraction core and Engine Lock as the GUI.

use std::collections::BTreeMap;
use std::io::Read;
use std::net::{Ipv4Addr, SocketAddr};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use axum::extract::{Path as UrlPath, Request, State};
use axum::http::{header, HeaderName, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::engine::EngineLock;
use crate::extractor::{extract, ExtractionRequest};

pub const DEFAULT_PORT: u16 = 47821;
/// Overrides the listening port (until there is a settings UI).
pub const PORT_ENV: &str = "YTDLM_BRIDGE_PORT";
pub const TOKEN_HEADER: &str = "x-bridge-token";
const TOKEN_FILE: &str = "bridge_token";
/// Finished jobs beyond this count are pruned, oldest first.
const MAX_JOBS: usize = 100;

const ALLOWED_HOSTS: &[&str] = &[
    "youtube.com",
    "www.youtube.com",
    "m.youtube.com",
    "music.youtube.com",
    "youtu.be",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
}

#[derive(Clone, Debug, Serialize)]
pub struct Job {
    pub id: u64,
    pub url: String,
    pub status: JobStatus,
    /// The most recent Engine output line.
    pub progress: Option<String>,
    /// Success message or error text once the job has finished.
    pub message: Option<String>,
}

/// What the Bridge needs from its surroundings. The Tauri app implements this
/// for real; tests implement it with a fake Engine.
pub trait BridgeHost: Send + Sync + 'static {
    fn engine_ready(&self) -> bool;
    fn engine_path(&self) -> PathBuf;
    fn nas_root(&self) -> PathBuf;
    fn config(&self) -> crate::config::AppConfig;
    /// Called for each Engine output line of a Bridge job.
    fn on_progress(&self, line: String);
    /// Called whenever a Bridge job changes status.
    fn on_job_update(&self, job: &Job);
}

#[derive(Clone)]
pub struct BridgeState {
    token: Arc<str>,
    jobs: Arc<Mutex<BTreeMap<u64, Job>>>,
    next_id: Arc<AtomicU64>,
    lock: EngineLock,
    host: Arc<dyn BridgeHost>,
}

impl BridgeState {
    pub fn new(token: String, lock: EngineLock, host: Arc<dyn BridgeHost>) -> Self {
        Self {
            token: token.into(),
            jobs: Arc::default(),
            next_id: Arc::new(AtomicU64::new(1)),
            lock,
            host,
        }
    }

    /// Applies `f` to job `id` under the jobs lock and returns the updated snapshot.
    fn update_job(&self, id: u64, f: impl FnOnce(&mut Job)) -> Option<Job> {
        let mut jobs = self.jobs.lock().unwrap_or_else(PoisonError::into_inner);
        let job = jobs.get_mut(&id)?;
        f(job);
        Some(job.clone())
    }

    fn insert_job(&self, url: String) -> Job {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let job = Job { id, url, status: JobStatus::Queued, progress: None, message: None };
        let mut jobs = self.jobs.lock().unwrap_or_else(PoisonError::into_inner);
        jobs.insert(id, job.clone());
        while jobs.len() > MAX_JOBS {
            let oldest_finished = jobs
                .iter()
                .find(|(_, j)| matches!(j.status, JobStatus::Succeeded | JobStatus::Failed))
                .map(|(id, _)| *id);
            match oldest_finished {
                Some(old) => {
                    jobs.remove(&old);
                }
                None => break,
            }
        }
        job
    }
}

/// Builds the Bridge HTTP router. Exposed for in-process testing.
pub fn router(state: BridgeState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|origin: &HeaderValue, _| {
            let o = origin.as_bytes();
            o.starts_with(b"chrome-extension://") || o.starts_with(b"moz-extension://")
        }))
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([header::CONTENT_TYPE, HeaderName::from_static(TOKEN_HEADER)]);

    Router::new()
        .route("/health", get(health))
        .route("/extract", post(submit))
        .route("/jobs/{id}", get(job_status))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_token))
        .with_state(state)
        // Outermost, so CORS preflight is answered before token auth runs.
        .layer(cors)
}

/// Constant-time comparison so the token can't be guessed byte-by-byte via timing.
fn token_matches(expected: &str, given: &[u8]) -> bool {
    let expected = expected.as_bytes();
    expected.len() == given.len()
        && expected.iter().zip(given).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
}

async fn require_token(State(state): State<BridgeState>, req: Request, next: Next) -> Response {
    let ok = req
        .headers()
        .get(TOKEN_HEADER)
        .is_some_and(|v| token_matches(&state.token, v.as_bytes()));
    if !ok {
        warn!("Browser Bridge rejected request to {} (missing or bad token)", req.uri().path());
        return error_response(StatusCode::UNAUTHORIZED, "Missing or invalid bridge token.");
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
    version: &'static str,
}

async fn health(State(state): State<BridgeState>) -> Json<Health> {
    Json(Health {
        ok: true,
        engine_ready: state.host.engine_ready(),
        version: env!("CARGO_PKG_VERSION"),
    })
}

#[derive(Deserialize)]
struct SubmitBody {
    url: String,
}

#[derive(Serialize)]
struct Submitted {
    job_id: u64,
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

async fn submit(State(state): State<BridgeState>, Json(body): Json<SubmitBody>) -> Response {
    let url = match validate_url(&body.url) {
        Ok(u) => u,
        Err(msg) => return error_response(StatusCode::BAD_REQUEST, msg),
    };
    let job = state.insert_job(url);
    info!("Browser Bridge queued job {} for {}", job.id, job.url);
    state.host.on_job_update(&job);

    let id = job.id;
    let worker = state.clone();
    tokio::task::spawn_blocking(move || run_job(&worker, id));

    (StatusCode::ACCEPTED, Json(Submitted { job_id: id })).into_response()
}

async fn job_status(State(state): State<BridgeState>, UrlPath(id): UrlPath<u64>) -> Response {
    let jobs = state.jobs.lock().unwrap_or_else(PoisonError::into_inner);
    match jobs.get(&id) {
        Some(job) => Json(job.clone()).into_response(),
        None => error_response(StatusCode::NOT_FOUND, "Unknown job id."),
    }
}

/// Runs a queued job to completion. Blocks; call from a blocking thread.
fn run_job(state: &BridgeState, id: u64) {
    let _guard = state.lock.acquire();
    let Some(job) = state.update_job(id, |j| j.status = JobStatus::Running) else { return };
    state.host.on_job_update(&job);

    let engine_path = state.host.engine_path();
    let nas_root = state.host.nas_root();
    let config = state.host.config();
    let req = ExtractionRequest {
        engine_path: &engine_path,
        nas_root: &nas_root,
        url: &job.url,
        config: &config,
    };
    let result = extract(&req, |line| {
        state.update_job(id, |j| j.progress = Some(line.clone()));
        state.host.on_progress(line);
    });

    let finished = state.update_job(id, |j| match result {
        Ok(msg) => {
            j.status = JobStatus::Succeeded;
            j.message = Some(msg);
        }
        Err(err) => {
            j.status = JobStatus::Failed;
            j.message = Some(err);
        }
    });
    if let Some(job) = finished {
        info!("Browser Bridge job {} finished: {:?}", job.id, job.status);
        state.host.on_job_update(&job);
    }
}

/// Reads the Bridge token from `dir`, creating a new random one (mode 0600) on first run.
pub fn load_or_create_token(dir: &Path) -> std::io::Result<String> {
    let path = dir.join(TOKEN_FILE);
    if let Ok(existing) = std::fs::read_to_string(&path) {
        let existing = existing.trim();
        if existing.len() >= 32 {
            return Ok(existing.to_string());
        }
    }
    let mut bytes = [0u8; 24];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    let token: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
    std::fs::create_dir_all(dir)?;
    std::fs::write(&path, &token)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    info!("Generated new Browser Bridge token at {:?}", path);
    Ok(token)
}

/// Resolves the port from [`PORT_ENV`], falling back to [`DEFAULT_PORT`].
pub fn configured_port() -> u16 {
    match std::env::var(PORT_ENV) {
        Ok(v) => v.parse().unwrap_or_else(|_| {
            warn!("Ignoring invalid {}={:?}; using {}", PORT_ENV, v, DEFAULT_PORT);
            DEFAULT_PORT
        }),
        Err(_) => DEFAULT_PORT,
    }
}

/// Binds to 127.0.0.1 only and serves until the app exits.
pub async fn serve(state: BridgeState, port: u16) -> std::io::Result<()> {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let listener = tokio::net::TcpListener::bind(addr).await.map_err(|e| {
        error!("Browser Bridge failed to bind {}: {}", addr, e);
        e
    })?;
    info!("Browser Bridge listening on http://{}", addr);
    axum::serve(listener, router(state)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    // Explicit import shadows the glob's `axum::extract::Request` alias so `Request::builder()` resolves.
    use axum::http::Request;
    use std::fs;
    use std::time::Duration;
    use tower::ServiceExt;

    const TOKEN: &str = "test-token-0123456789abcdef0123456789";

    struct FakeHost {
        dir: PathBuf,
        updates: Mutex<Vec<JobStatus>>,
    }

    impl BridgeHost for FakeHost {
        fn engine_ready(&self) -> bool {
            true
        }
        fn engine_path(&self) -> PathBuf {
            self.dir.join("fake-yt-dlp")
        }
        fn nas_root(&self) -> PathBuf {
            self.dir.join("nas")
        }
        fn config(&self) -> crate::config::AppConfig {
            crate::config::AppConfig::default()
        }
        fn on_progress(&self, _line: String) {}
        fn on_job_update(&self, job: &Job) {
            self.updates.lock().unwrap().push(job.status);
        }
    }

    fn fake_host(name: &str, script: &str) -> Arc<FakeHost> {
        let dir = std::env::temp_dir().join(format!(
            "ytdlm-bridge-{}-{}-{}",
            name,
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        fs::create_dir_all(dir.join("nas")).unwrap();
        let engine = dir.join("fake-yt-dlp");
        fs::write(&engine, format!("#!/bin/sh\n{}\n", script)).unwrap();
        fs::set_permissions(&engine, fs::Permissions::from_mode(0o755)).unwrap();
        Arc::new(FakeHost { dir, updates: Mutex::new(Vec::new()) })
    }

    fn app(host: Arc<FakeHost>) -> Router {
        router(BridgeState::new(TOKEN.into(), EngineLock::default(), host))
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

    async fn json(resp: Response) -> serde_json::Value {
        serde_json::from_slice(&to_bytes(resp.into_body(), 1 << 20).await.unwrap()).unwrap()
    }

    #[tokio::test]
    async fn rejects_missing_and_wrong_token() {
        let app = app(fake_host("auth", "exit 0"));
        let r = app.clone().oneshot(req("GET", "/health", None, None)).await.unwrap();
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
        let r = app.oneshot(req("GET", "/health", Some("nope"), None)).await.unwrap();
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn health_reports_engine_ready() {
        let r = app(fake_host("health", "exit 0"))
            .oneshot(req("GET", "/health", Some(TOKEN), None))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(json(r).await["engine_ready"], true);
    }

    #[tokio::test]
    async fn cors_preflight_allows_extension_origin_without_token() {
        let r = app(fake_host("cors", "exit 0"))
            .oneshot(
                Request::builder()
                    .method("OPTIONS")
                    .uri("/extract")
                    .header(header::ORIGIN, "chrome-extension://abcdefghijklmnop")
                    .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
                    .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "content-type,x-bridge-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(
            r.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN).unwrap(),
            "chrome-extension://abcdefghijklmnop"
        );
    }

    #[tokio::test]
    async fn cors_does_not_allow_web_origins() {
        let r = app(fake_host("cors-web", "exit 0"))
            .oneshot(
                Request::builder()
                    .method("OPTIONS")
                    .uri("/extract")
                    .header(header::ORIGIN, "https://evil.example")
                    .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(r.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN).is_none());
    }

    #[tokio::test]
    async fn rejects_non_youtube_urls() {
        let r = app(fake_host("badurl", "exit 0"))
            .oneshot(req("POST", "/extract", Some(TOKEN), Some(r#"{"url":"https://example.com/x"}"#)))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn unknown_job_is_404() {
        let r = app(fake_host("404", "exit 0"))
            .oneshot(req("GET", "/jobs/999", Some(TOKEN), None))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
    }

    async fn submit_and_wait(app: &Router, url: &str) -> serde_json::Value {
        let body = format!(r#"{{"url":"{}"}}"#, url);
        let r = app.clone().oneshot(req("POST", "/extract", Some(TOKEN), Some(&body))).await.unwrap();
        assert_eq!(r.status(), StatusCode::ACCEPTED);
        let id = json(r).await["job_id"].as_u64().unwrap();
        for _ in 0..200 {
            let r = app.clone().oneshot(req("GET", &format!("/jobs/{id}"), Some(TOKEN), None)).await.unwrap();
            let job = json(r).await;
            if job["status"] == "succeeded" || job["status"] == "failed" {
                return job;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("job {id} did not finish");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn job_lifecycle_success() {
        let host = fake_host("ok", "echo '[download] 100%'\nexit 0");
        let app = app(host.clone());
        let job = submit_and_wait(&app, "https://music.youtube.com/watch?v=abc").await;
        assert_eq!(job["status"], "succeeded");
        assert_eq!(job["progress"], "[download] 100%");
        assert_eq!(
            *host.updates.lock().unwrap(),
            vec![JobStatus::Queued, JobStatus::Running, JobStatus::Succeeded]
        );
        fs::remove_dir_all(&host.dir).unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn job_lifecycle_failure_reports_error() {
        let host = fake_host("fail", "echo 'ERROR: Video unavailable' 1>&2\nexit 1");
        let job = submit_and_wait(&app(host.clone()), "https://youtu.be/x").await;
        assert_eq!(job["status"], "failed");
        assert!(job["message"].as_str().unwrap().contains("exit code"));
        assert_eq!(job["progress"], "ERROR: Video unavailable");
        fs::remove_dir_all(&host.dir).unwrap();
    }

    #[test]
    fn token_is_persisted_and_private() {
        let dir = std::env::temp_dir().join(format!("ytdlm-token-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let first = load_or_create_token(&dir).unwrap();
        assert_eq!(first.len(), 48);
        assert_eq!(load_or_create_token(&dir).unwrap(), first);
        let mode = fs::metadata(dir.join(TOKEN_FILE)).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        fs::remove_dir_all(&dir).unwrap();
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
