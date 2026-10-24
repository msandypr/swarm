use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter};
use std::path::Path;
use serde::{Deserialize, Serialize};

use crate::preflight::PreflightInfo;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SegmentMeta {
    pub index: usize,
    pub start: u64,
    pub end: u64, // inclusive
    pub downloaded: u64,
}

impl SegmentMeta {
    pub fn is_completed(&self) -> bool {
        self.downloaded >= (self.end - self.start + 1)
    }

    pub fn remaining_bytes(&self) -> u64 {
        let total = self.end - self.start + 1;
        total.saturating_sub(self.downloaded)
    }

    pub fn current_offset(&self) -> u64 {
        self.start + self.downloaded
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DownloadMeta {
    pub url: String,
    pub final_url: String,
    pub filename: String,
    pub total_size: Option<u64>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub segments: Vec<SegmentMeta>,
    pub supports_range: bool,
}

impl DownloadMeta {
    /// Initializes new download metadata from preflight information and connection count.
    pub fn new(preflight: &PreflightInfo, connection_count: usize) -> Self {
        let mut segments = Vec::new();

        if let Some(total) = preflight.total_size {
            if preflight.supports_range && total > 0 && connection_count > 1 {
                let count = connection_count.min(16).max(1);
                let chunk_size = total / count as u64;

                for i in 0..count {
                    let start = i as u64 * chunk_size;
                    let end = if i == count - 1 {
                        total - 1 // Last segment takes the remainder
                    } else {
                        (i as u64 + 1) * chunk_size - 1
                    };

                    segments.push(SegmentMeta {
                        index: i,
                        start,
                        end,
                        downloaded: 0,
                    });
                }
            } else {
                // Single connection
                segments.push(SegmentMeta {
                    index: 0,
                    start: 0,
                    end: total.saturating_sub(1),
                    downloaded: 0,
                });
            }
        } else {
            // Unknown length (streaming)
            segments.push(SegmentMeta {
                index: 0,
                start: 0,
                end: u64::MAX,
                downloaded: 0,
            });
        }

        Self {
            url: preflight.url.clone(),
            final_url: preflight.final_url.clone(),
            filename: preflight.filename.clone(),
            total_size: preflight.total_size,
            etag: preflight.etag.clone(),
            last_modified: preflight.last_modified.clone(),
            segments,
            supports_range: preflight.supports_range,
        }
    }

    /// Checks if existing metadata is valid for resuming with fresh preflight.
    pub fn is_resumable(&self, preflight: &PreflightInfo) -> bool {
        if !self.supports_range || !preflight.supports_range {
            return false;
        }

        if self.total_size != preflight.total_size {
            return false;
        }

        // Validate ETag if both available
        if let (Some(saved_etag), Some(new_etag)) = (&self.etag, &preflight.etag) {
            if saved_etag != new_etag {
                return false;
            }
        }

        // Validate Last-Modified if both available
        if let (Some(saved_lm), Some(new_lm)) = (&self.last_modified, &preflight.last_modified) {
            if saved_lm != new_lm {
                return false;
            }
        }

        true
    }

    /// Computes total bytes downloaded across all segments.
    pub fn total_downloaded(&self) -> u64 {
        self.segments.iter().map(|s| s.downloaded).sum()
    }

    /// Saves metadata to disk atomically (write to .tmp then rename).
    pub fn save_to_file(&self, path: &Path) -> io::Result<()> {
        let tmp_path = path.with_extension("meta.tmp");
        {
            let file = File::create(&tmp_path)?;
            let writer = BufWriter::new(file);
            serde_json::to_writer(writer, self)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        }
        fs::rename(tmp_path, path)?;
        Ok(())
    }

    /// Loads metadata from a `.part.meta` file.
    pub fn load_from_file(path: &Path) -> io::Result<Self> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        serde_json::from_reader(reader)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_segment_splitting() {
        let preflight = PreflightInfo {
            url: "https://example.com/file.zip".to_string(),
            final_url: "https://example.com/file.zip".to_string(),
            supports_range: true,
            total_size: Some(1000),
            etag: Some("\"xyz\"".to_string()),
            last_modified: None,
            filename: "file.zip".to_string(),
            mime_type: None,
        };

        let meta = DownloadMeta::new(&preflight, 4);
        assert_eq!(meta.segments.len(), 4);
        assert_eq!(meta.segments[0].start, 0);
        assert_eq!(meta.segments[0].end, 249);
        assert_eq!(meta.segments[1].start, 250);
        assert_eq!(meta.segments[1].end, 499);
        assert_eq!(meta.segments[2].start, 500);
        assert_eq!(meta.segments[2].end, 749);
        assert_eq!(meta.segments[3].start, 750);
        assert_eq!(meta.segments[3].end, 999);
    }
}
