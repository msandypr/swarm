use reqwest::header::{HeaderMap, ACCEPT_RANGES, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, ETAG, LAST_MODIFIED, RANGE};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::filename::resolve_filename;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PreflightInfo {
    pub url: String,
    pub final_url: String,
    pub supports_range: bool,
    pub total_size: Option<u64>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub filename: String,
    pub mime_type: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum PreflightError {
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("Server returned HTTP error status: {0}")]
    HttpStatus(u16),
}

/// Performs pre-flight check using a GET request with `Range: bytes=0-0`.
/// Tests range support, obtains total content length, ETag, Last-Modified, and resolves filename.
pub async fn check_url(
    client: &Client,
    url_str: &str,
    custom_headers: Option<HeaderMap>,
) -> Result<PreflightInfo, PreflightError> {
    let mut req = client.get(url_str).header(RANGE, "bytes=0-0");
    if let Some(headers) = custom_headers {
        req = req.headers(headers);
    }

    let res = req.send().await?;
    let status = res.status();

    if !status.is_success() && status != reqwest::StatusCode::PARTIAL_CONTENT {
        return Err(PreflightError::HttpStatus(status.as_u16()));
    }

    let final_url = res.url().to_string();
    let headers = res.headers().clone();

    // 1. Check Range support
    let mut supports_range = false;
    let mut total_size = None;

    if status == reqwest::StatusCode::PARTIAL_CONTENT {
        supports_range = true;
        // Parse Content-Range: bytes 0-0/1048576
        if let Some(cr) = headers.get(CONTENT_RANGE).and_then(|h| h.to_str().ok()) {
            if let Some(slash_idx) = cr.rfind('/') {
                let size_str = &cr[slash_idx + 1..].trim();
                if let Ok(size) = size_str.parse::<u64>() {
                    total_size = Some(size);
                }
            }
        }
    } else if let Some(ar) = headers.get(ACCEPT_RANGES).and_then(|h| h.to_str().ok()) {
        if ar.eq_ignore_ascii_case("bytes") {
            supports_range = true;
        }
    }

    // Fallback to Content-Length if total_size not resolved from Content-Range
    if total_size.is_none() {
        if let Some(cl) = headers.get(CONTENT_LENGTH).and_then(|h| h.to_str().ok()) {
            if let Ok(size) = cl.parse::<u64>() {
                total_size = Some(size);
            }
        }
    }

    // 2. Extract ETag & Last-Modified
    let etag = headers
        .get(ETAG)
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    let last_modified = headers
        .get(LAST_MODIFIED)
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    // 3. Extract MIME type
    let mime_type = headers
        .get(CONTENT_TYPE)
        .and_then(|h| h.to_str().ok())
        .map(|s| {
            // Strip parameters like charset
            s.split(';').next().unwrap_or(s).trim().to_string()
        });

    // 4. Resolve filename
    let cd_str = headers.get(CONTENT_DISPOSITION).and_then(|h| h.to_str().ok());
    let filename = resolve_filename(cd_str, &final_url);

    Ok(PreflightInfo {
        url: url_str.to_string(),
        final_url,
        supports_range,
        total_size,
        etag,
        last_modified,
        filename,
        mime_type,
    })
}
