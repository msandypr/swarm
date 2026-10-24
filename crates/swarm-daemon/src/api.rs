#![allow(dead_code)]
use std::convert::Infallible;

use axum::extract::{Path, Query, Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
use reqwest::header::{HeaderMap, HeaderValue, COOKIE, REFERER, USER_AGENT};
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
    let api_routes = Router::new()
        .route("/ping", get(handle_ping))
        .route("/downloads", get(handle_list_downloads).post(handle_add_download))
        .route("/ext/add", post(handle_ext_add))
        .route("/downloads/:id", patch(handle_action).delete(handle_delete))
        .route("/events", get(handle_events))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_token_middleware,
        ));

    Router::new()
        .route("/", get(handle_root))
        .nest("/api", api_routes)
        .layer(middleware::from_fn(validate_host))
        .with_state(state)
}

async fn auth_token_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Exempt ping so extension / UI can check connectivity
    let path = req.uri().path();
    if path == "/api/ping" || path == "/ping" || path.ends_with("/ping") {
        return Ok(next.run(req).await);
    }

    if let Some(token) = extract_token(&req) {
        if token == state.runtime.token {
            return Ok(next.run(req).await);
        }
    }

    Err(StatusCode::UNAUTHORIZED)
}

async fn handle_root() -> impl IntoResponse {
    // Placeholder until Web UI SPA assets are embedded
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        "<!DOCTYPE html><html><head><title>Swarm</title></head><body><h1>Swarm Download Manager</h1><p>Daemon is running.</p></body></html>",
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
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let mut headers = HeaderMap::new();

    if let Some(ua) = payload.user_agent {
        if let Ok(val) = HeaderValue::from_str(&ua) {
            headers.insert(USER_AGENT, val);
        }
    }
    if let Some(ref_url) = payload.referrer {
        if let Ok(val) = HeaderValue::from_str(&ref_url) {
            headers.insert(REFERER, val);
        }
    }
    if let Some(cookie) = payload.cookie {
        if let Ok(val) = HeaderValue::from_str(&cookie) {
            headers.insert(COOKIE, val);
        }
    }

    let id = state
        .queue
        .add_download(payload.url, Some(headers), Some(8))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    Ok(Json(serde_json::json!({ "id": id, "status": "intercepted" })))
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
    let rx = state.queue.subscribe_progress();

    let stream = stream::unfold(rx, |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(progress) => {
                    let json = serde_json::to_string(&progress).unwrap_or_default();
                    let event = Event::default().event("progress").data(json);
                    return Some((Ok(event), rx));
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    // Consumer lagged, continue to next message
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
