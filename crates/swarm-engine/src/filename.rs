use percent_encoding::percent_decode_str;
use std::path::{Path, PathBuf};

const RESERVED_WINDOWS_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL",
    "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
    "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

const ILLEGAL_CHARS: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];

/// Parses and extracts a safe filename from Content-Disposition header, falling back to the URL.
pub fn resolve_filename(content_disposition: Option<&str>, url_str: &str) -> String {
    if let Some(cd) = content_disposition {
        if let Some(parsed) = parse_content_disposition(cd) {
            let sanitized = sanitize_filename(&parsed);
            if !sanitized.is_empty() {
                return sanitized;
            }
        }
    }

    // Fallback to URL path
    if let Ok(parsed_url) = url::Url::parse(url_str) {
        if let Some(mut segments) = parsed_url.path_segments() {
            // Find the last non-empty segment
            while let Some(segment) = segments.next_back() {
                if !segment.is_empty() {
                    let decoded = percent_decode_str(segment).decode_utf8_lossy().to_string();
                    let sanitized = sanitize_filename(&decoded);
                    if !sanitized.is_empty() {
                        return sanitized;
                    }
                }
            }
        }
    }

    "download".to_string()
}

/// Parses Content-Disposition header according to RFC 6266 and RFC 5987.
pub fn parse_content_disposition(header: &str) -> Option<String> {
    let mut filename_star = None;
    let mut filename_normal = None;

    for param in header.split(';') {
        let trimmed = param.trim();
        if let Some(rest) = trimmed.strip_prefix("filename*=") {
            let val = rest.trim_matches('"');
            if let Some(decoded) = parse_rfc5987(val) {
                filename_star = Some(decoded);
            }
        } else if let Some(rest) = trimmed.strip_prefix("filename=") {
            let val = rest.trim();
            let unquoted = if val.starts_with('"') && val.ends_with('"') && val.len() >= 2 {
                &val[1..val.len() - 1]
            } else {
                val
            };
            filename_normal = Some(unquoted.to_string());
        }
    }

    // RFC 6266: filename* takes precedence over filename
    filename_star.or(filename_normal)
}

fn parse_rfc5987(val: &str) -> Option<String> {
    // Format: UTF-8''encoded_filename or ISO-8859-1''encoded_filename
    let parts: Vec<&str> = val.splitn(3, '\'').collect();
    if parts.len() == 3 {
        let charset = parts[0].to_uppercase();
        let encoded_val = parts[2];
        if charset == "UTF-8" {
            return Some(percent_decode_str(encoded_val).decode_utf8_lossy().to_string());
        }
    }
    // Fallback direct percent-decode
    Some(percent_decode_str(val).decode_utf8_lossy().to_string())
}

/// Sanitizes a string to make it a valid, safe Windows filename.
pub fn sanitize_filename(name: &str) -> String {
    // Replace illegal characters with underscore '_'
    let mut clean: String = name
        .chars()
        .map(|c| {
            if ILLEGAL_CHARS.contains(&c) || (c as u32) < 32 {
                '_'
            } else {
                c
            }
        })
        .collect();

    // Trim trailing dots and spaces (illegal in Windows file systems)
    while clean.ends_with('.') || clean.ends_with(' ') {
        clean.pop();
    }

    // Avoid empty filename
    if clean.is_empty() {
        return "download".to_string();
    }

    // Check Windows reserved names (CON, PRN, AUX, NUL, COM1-9, LPT1-9)
    let stem = match clean.find('.') {
        Some(idx) => &clean[..idx],
        None => &clean,
    };

    if RESERVED_WINDOWS_NAMES.iter().any(|&res| stem.eq_ignore_ascii_case(res)) {
        clean = format!("_{}", clean);
    }

    clean
}

/// Generates a non-colliding filename if `target_path` already exists:
/// `file.ext` -> `file (1).ext` -> `file (2).ext` ...
pub fn resolve_collision(target_path: &Path) -> PathBuf {
    if !target_path.exists() {
        return target_path.to_path_buf();
    }

    let parent = target_path.parent().unwrap_or_else(|| Path::new("."));
    let stem = target_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("download");
    let ext = target_path
        .extension()
        .and_then(|e| e.to_str());

    let mut counter = 1;
    loop {
        let new_filename = match ext {
            Some(e) => format!("{} ({}).{}", stem, counter, e),
            None => format!("{} ({})", stem, counter),
        };
        let candidate = parent.join(&new_filename);
        if !candidate.exists() {
            return candidate;
        }
        counter += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_content_disposition_normal() {
        let cd = "attachment; filename=\"ubuntu-24.04.iso\"";
        assert_eq!(parse_content_disposition(cd), Some("ubuntu-24.04.iso".to_string()));
    }

    #[test]
    fn test_content_disposition_rfc5987() {
        let cd = "attachment; filename*=UTF-8''%e2%82%ac%20rates.pdf";
        assert_eq!(parse_content_disposition(cd), Some("€ rates.pdf".to_string()));
    }

    #[test]
    fn test_content_disposition_both() {
        let cd = "attachment; filename=\"fallback.pdf\"; filename*=UTF-8''real%20doc.pdf";
        assert_eq!(parse_content_disposition(cd), Some("real doc.pdf".to_string()));
    }

    #[test]
    fn test_sanitize_illegal_characters() {
        let bad = "test/file:name*with?illegal<chars>|quote\".zip";
        let sanitized = sanitize_filename(bad);
        assert_eq!(sanitized, "test_file_name_with_illegal_chars__quote_.zip");
    }

    #[test]
    fn test_sanitize_reserved_device_names() {
        assert_eq!(sanitize_filename("CON.txt"), "_CON.txt");
        assert_eq!(sanitize_filename("NUL"), "_NUL");
        assert_eq!(sanitize_filename("com1.tar.gz"), "_com1.tar.gz");
    }

    #[test]
    fn test_resolve_from_url() {
        let url = "https://example.com/downloads/package-v1.0.zip?auth=xyz";
        assert_eq!(resolve_filename(None, url), "package-v1.0.zip");
    }
}
