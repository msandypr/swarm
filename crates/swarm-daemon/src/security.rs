#![allow(dead_code)]
use axum::extract::Request;
use axum::http::{header, StatusCode};
use axum::middleware::Next;
use axum::response::Response;

/// Middleware to prevent DNS Rebinding attacks.
/// Enforces that the Host header is 127.0.0.1[:port] or localhost[:port].
pub async fn validate_host(req: Request, next: Next) -> Result<Response, StatusCode> {
    if let Some(host) = req.headers().get(header::HOST).and_then(|h| h.to_str().ok()) {
        let is_valid = host == "127.0.0.1"
            || host.starts_with("127.0.0.1:")
            || host == "localhost"
            || host.starts_with("localhost:");

        if !is_valid {
            return Err(StatusCode::FORBIDDEN);
        }
    }

    Ok(next.run(req).await)
}

/// Helper to extract token from Request headers, cookies, or query parameters.
pub fn extract_token(req: &Request) -> Option<String> {
    // 1. Authorization: Bearer <token>
    if let Some(auth) = req.headers().get(header::AUTHORIZATION).and_then(|h| h.to_str().ok()) {
        if let Some(token) = auth.strip_prefix("Bearer ") {
            return Some(token.trim().to_string());
        }
    }

    // 2. X-Swarm-Token: <token>
    if let Some(token) = req.headers().get("x-swarm-token").and_then(|h| h.to_str().ok()) {
        return Some(token.trim().to_string());
    }

    // 3. Cookie: swarm_token=<token>
    if let Some(cookie) = req.headers().get(header::COOKIE).and_then(|h| h.to_str().ok()) {
        for part in cookie.split(';') {
            let part = part.trim();
            if let Some(token) = part.strip_prefix("swarm_token=") {
                return Some(token.trim().to_string());
            }
        }
    }

    // 4. Query string: ?t=<token>
    if let Some(query) = req.uri().query() {
        for pair in query.split('&') {
            if let Some(token) = pair.strip_prefix("t=") {
                return Some(token.to_string());
            }
        }
    }

    None
}
