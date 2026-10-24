#![allow(dead_code)]
use std::convert::Infallible;

use axum::extract::{Path, Query, Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use futures_util::stream::{self, Stream};
use serde::Deserialize;

use crate::queue::{AddDownloadRequest, ExtAddDownloadRequest, QueueManager};
use crate::runtime::RuntimeInfo;
use crate::security::{extract_token, validate_host};

#[derive(Clone)]
pub struct AppState {
    pub runtime: RuntimeInfo,
    pub queue: QueueManager,
}

#[derive(Debug, Deserialize)]
pub struct ActionRequest {
    pub action: String, // "pause", "resume", "cancel"
}

#[derive(Debug, Deserialize)]
pub struct DeleteQuery {
    pub delete_file: Option<bool>,
}

pub fn create_router(state: AppState) -> Router {
    let cors = tower_http::cors::CorsLayer::permissive();

    let api_routes = Router::new()
        .route("/ping", get(handle_ping))
        .route("/ext/handshake", get(handle_ext_handshake))
        .route("/preflight", post(handle_preflight))
        .route("/downloads", get(handle_list_downloads).post(handle_add_download))
        .route("/ext/add", post(handle_ext_add))
        .route("/ext/open-folder", post(handle_open_extension_folder))
        .route("/ext/open-browser-extensions", post(handle_open_browser_extensions))
        .route("/ext/install", post(handle_one_click_install))
        .route("/ext/save-to-downloads", post(handle_save_ext_to_downloads))
        .route("/downloads/:id", patch(handle_action).delete(handle_delete))
        .route("/downloads/:id/open-folder", post(handle_open_download_folder))
        .route("/downloads/:id/open-file", post(handle_open_download_file))
        .route("/events", get(handle_events))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_token_middleware,
        ));

    Router::new()
        .route("/", get(handle_root))
        .route("/logo.png", get(handle_logo))
        .route("/icon-128.png", get(handle_icon_128))
        .route("/favicon.png", get(handle_favicon))
        .route("/swarm-extension.zip", get(handle_download_extension_zip))
        .nest("/api", api_routes)
        .layer(cors)
        .layer(middleware::from_fn(validate_host))
        .with_state(state)
}

async fn auth_token_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // 1. Exempt ping and handshake so extension can auto-discover
    let path = req.uri().path();
    if path == "/api/ping" || path == "/ping" || path.ends_with("/ping")
        || path == "/api/ext/handshake" || path.ends_with("/ext/handshake")
    {
        return Ok(next.run(req).await);
    }

    // 2. Auto-authenticate extension requests via Origin header (unforgeable by web pages)
    if let Some(origin) = req.headers().get(header::ORIGIN).and_then(|h| h.to_str().ok()) {
        if origin.starts_with("chrome-extension://") || origin.starts_with("moz-extension://") {
            return Ok(next.run(req).await);
        }
    }

    // 3. Normal token check
    if let Some(token) = extract_token(&req) {
        if token == state.runtime.token {
            return Ok(next.run(req).await);
        }
    }
    Err(StatusCode::UNAUTHORIZED)
}

static INDEX_HTML: &[u8] = include_bytes!("../../../web/dist/index.html");
static LOGO_PNG: &[u8] = include_bytes!("../../../static/logo.png");
static ICON_128_PNG: &[u8] = include_bytes!("../../../static/icon-128.png");
static FAVICON_PNG: &[u8] = include_bytes!("../../../static/icon-32.png");
static EXTENSION_ZIP: &[u8] = include_bytes!("../../../extension/swarm-extension.zip");
async fn handle_root() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        INDEX_HTML,
    )
}

async fn handle_logo() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "image/png")],
        LOGO_PNG,
    )
}

async fn handle_icon_128() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "image/png")],
        ICON_128_PNG,
    )
}

async fn handle_download_extension_zip() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "application/zip"),
            (header::CONTENT_DISPOSITION, "attachment; filename=\"swarm-extension.zip\""),
        ],
        EXTENSION_ZIP,
    )
}

async fn handle_favicon() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "image/png")],
        FAVICON_PNG,
    )
}

async fn handle_ping(State(state): State<AppState>) -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "ok",
        "app": "Swarm",
        "version": "0.1.0",
        "port": state.runtime.port,
        "pid": state.runtime.pid,
    }))
}

async fn handle_ext_handshake(State(state): State<AppState>) -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "ok",
        "token": state.runtime.token,
        "port": state.runtime.port,
    }))
}

async fn handle_open_extension_folder() -> impl IntoResponse {
    let mut ext_dir = std::path::PathBuf::from("extension");
    if !ext_dir.exists() {
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(parent) = exe_path.parent() {
                let candidate = parent.join("extension");
                if candidate.exists() {
                    ext_dir = candidate;
                }
            }
        }
    }

    if ext_dir.exists() {
        let abs_path = ext_dir
            .canonicalize()
            .map(|p| {
                let s = p.to_string_lossy().to_string();
                s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
            })
            .unwrap_or_else(|_| ext_dir.to_string_lossy().to_string());

        crate::tray::open_folder_in_explorer(&abs_path);
        return Json(serde_json::json!({ "success": true, "path": abs_path }));
    }

    Json(serde_json::json!({ "success": false, "error": "Folder not found" }))
}

#[derive(Debug, Deserialize)]
pub struct LaunchBrowserRequest {
    pub browser: Option<String>,
}

async fn handle_launch_browser(
    Json(payload): Json<LaunchBrowserRequest>,
) -> impl IntoResponse {
    let browser = payload.browser.unwrap_or_else(|| "edge".to_string());
    let ok = crate::tray::launch_browser_with_extension(&browser);
    Json(serde_json::json!({ "success": ok }))
}

#[derive(Debug, Deserialize)]
pub struct OpenBrowserPageRequest {
    pub browser: Option<String>,
}

async fn handle_open_browser_extensions(
    Json(payload): Json<OpenBrowserPageRequest>,
) -> impl IntoResponse {
    let browser = payload.browser.unwrap_or_else(|| "edge".to_string());
    let ok = crate::tray::open_browser_extensions_page(&browser);
    Json(serde_json::json!({ "success": ok }))
}

#[derive(Debug, Deserialize)]
pub struct OneClickInstallRequest {
    pub browser: Option<String>,
}

async fn handle_one_click_install(
    Json(payload): Json<OneClickInstallRequest>,
) -> impl IntoResponse {
    let browser = payload.browser.unwrap_or_else(|| "edge".to_string());
    match crate::tray::install_extension_to_browser(&browser) {
        Ok(msg) => Json(serde_json::json!({ "success": true, "message": msg })),
        Err(err) => Json(serde_json::json!({ "success": false, "error": err })),
    }
}

async fn handle_save_ext_to_downloads() -> impl IntoResponse {
    let mut dl_dir = std::path::PathBuf::from(r"C:\Downloads");
    if let Ok(profile) = std::env::var("USERPROFILE") {
        let p = std::path::PathBuf::from(profile).join("Downloads");
        if p.exists() {
            dl_dir = p;
        }
    }

    let target_path = dl_dir.join("swarm-extension.zip");
    if let Err(e) = std::fs::write(&target_path, EXTENSION_ZIP) {
        return Json(serde_json::json!({ "success": false, "error": e.to_string() }));
    }

    crate::tray::open_folder_in_explorer(&target_path.to_string_lossy());
    Json(serde_json::json!({ "success": true, "path": target_path.to_string_lossy() }))
}

async fn handle_open_download_folder(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let list = state.queue.list_downloads().await;
    if let Some(item) = list.iter().find(|d| d.id == id) {
        if !item.output_path.is_empty() {
            let path = std::path::Path::new(&item.output_path);
            let folder = path.parent().unwrap_or(path);
            crate::tray::open_folder_in_explorer(&folder.to_string_lossy());
            return Json(serde_json::json!({ "success": true }));
        }
    }
    crate::tray::open_downloads_folder();
    Json(serde_json::json!({ "success": true }))
}

#[derive(Debug, Deserialize)]
pub struct PreflightRequest {
    pub url: String,
}

async fn handle_preflight(
    Json(payload): Json<PreflightRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let client = reqwest::Client::new();
    let info = swarm_engine::preflight::check_url(&client, &payload.url, None)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    Ok(Json(serde_json::json!({
        "filename": info.filename,
        "total_size": info.total_size,
        "supports_range": info.supports_range,
        "mime_type": info.mime_type,
    })))
}

async fn handle_open_download_file(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let list = state.queue.list_downloads().await;
    if let Some(item) = list.iter().find(|d| d.id == id) {
        if !item.output_path.is_empty() {
            crate::tray::open_file_in_shell(&item.output_path);
            return Json(serde_json::json!({ "success": true }));
        }
    }
    Json(serde_json::json!({ "success": false, "error": "File not found" }))
}

async fn handle_list_downloads(State(state): State<AppState>) -> impl IntoResponse {
    let list = state.queue.list_downloads().await;
    Json(list)
}

async fn handle_add_download(
    State(state): State<AppState>,
    Json(payload): Json<AddDownloadRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let id = state
        .queue
        .add_download(payload.url, None, payload.connections)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    Ok(Json(serde_json::json!({ "id": id, "status": "started" })))
}

async fn handle_ext_add(
    State(state): State<AppState>,
    Json(payload): Json<ExtAddDownloadRequest>,
) -> impl IntoResponse {
    state.queue.broadcast_incoming(
        payload.url,
        payload.suggested_filename,
        payload.referrer,
        payload.cookie,
    );

    Json(serde_json::json!({ "status": "intercepted" }))
}

async fn handle_action(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<ActionRequest>,
) -> impl IntoResponse {
    match payload.action.as_str() {
        "pause" | "cancel" => {
            let ok = state.queue.cancel_download(&id).await;
            Json(serde_json::json!({ "id": id, "success": ok }))
        }
        _ => Json(serde_json::json!({ "id": id, "error": "Unknown action" })),
    }
}

async fn handle_delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<DeleteQuery>,
) -> impl IntoResponse {
    let delete_file = query.delete_file.unwrap_or(false);
    let ok = state.queue.delete_download(&id, delete_file).await;
    Json(serde_json::json!({ "id": id, "deleted": ok }))
}

async fn handle_events(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.queue.subscribe_events();

    let stream = stream::unfold(rx, |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(app_event) => {
                    let (event_name, json) = match app_event {
                        crate::queue::AppServerEvent::Progress(p) => {
                            ("progress", serde_json::to_string(&p).unwrap_or_default())
                        }
                        crate::queue::AppServerEvent::IncomingDownload {
                            url,
                            suggested_filename,
                            referrer,
                            cookie,
                        } => {
                            (
                                "incoming_download",
                                serde_json::to_string(&serde_json::json!({
                                    "url": url,
                                    "suggested_filename": suggested_filename,
                                    "referrer": referrer,
                                    "cookie": cookie,
                                }))
                                .unwrap_or_default(),
                            )
                        }
                    };
                    let event = Event::default().event(event_name).data(json);
                    return Some((Ok(event), rx));
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    continue;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    return None;
                }
            }
        }
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{header, Request, StatusCode};
    use tower::ServiceExt;
    use crate::store::StateStore;

    #[tokio::test]
    async fn test_api_ping_and_auth() {
        let temp_dir = std::env::temp_dir().join("swarm_test_api");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let store = StateStore::open_or_create(&temp_dir).unwrap();
        let queue = QueueManager::new(store, temp_dir.clone());
        let runtime = RuntimeInfo {
            port: 52417,
            token: "secret_test_token_12345".to_string(),
            pid: 1000,
        };

        let state = AppState { runtime, queue };
        let app = create_router(state);

        // 1. /api/ping requires no auth
        let req = Request::builder()
            .uri("/api/ping")
            .header(header::HOST, "127.0.0.1:52417")
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        // 2. /api/downloads fails without token
        let req = Request::builder()
            .uri("/api/downloads")
            .header(header::HOST, "127.0.0.1:52417")
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

        // 3. /api/downloads succeeds with valid Bearer token
        let req = Request::builder()
            .uri("/api/downloads")
            .header(header::HOST, "127.0.0.1:52417")
            .header(header::AUTHORIZATION, "Bearer secret_test_token_12345")
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        // 4. DNS Rebinding protection: non-localhost Host header returns 403 Forbidden
        let req = Request::builder()
            .uri("/api/ping")
            .header(header::HOST, "attacker-domain.com")
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
