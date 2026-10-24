use std::fs::File;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use reqwest::header::{HeaderMap, RANGE};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::{watch, Mutex};
use tokio::time::Instant;

use crate::disk::{create_preallocated_part_file, finalize_download, write_chunk_at};
use crate::filename::resolve_collision;
use crate::limiter::SpeedLimiter;
use crate::meta::{DownloadMeta, SegmentMeta};
use crate::preflight::check_url;

const META_FLUSH_INTERVAL: Duration = Duration::from_secs(2);
const MIN_SPLIT_THRESHOLD: u64 = 5 * 1024 * 1024; // 5 MB

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DownloadStatus {
    Queued,
    Downloading,
    Paused,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentProgress {
    pub index: usize,
    pub start: u64,
    pub end: u64,
    pub downloaded: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadProgress {
    pub id: String,
    pub filename: String,
    pub total_size: Option<u64>,
    pub downloaded_bytes: u64,
    pub speed_bytes_per_sec: u64,
    pub eta_seconds: Option<u64>,
    pub status: DownloadStatus,
    pub segments: Vec<SegmentProgress>,
}

pub struct DownloadTask {
    pub id: String,
    pub url: String,
    pub output_dir: PathBuf,
    pub connection_count: usize,
    pub limiter: SpeedLimiter,
    pub headers: Option<HeaderMap>,
    client: Client,
    cancel_flag: Arc<AtomicBool>,
    status_tx: watch::Sender<DownloadProgress>,
}

impl DownloadTask {
    pub fn new(
        id: String,
        url: String,
        output_dir: PathBuf,
        connection_count: usize,
        limiter: SpeedLimiter,
        headers: Option<HeaderMap>,
    ) -> (Self, watch::Receiver<DownloadProgress>) {
        let (status_tx, status_rx) = watch::channel(DownloadProgress {
            id: id.clone(),
            filename: String::new(),
            total_size: None,
            downloaded_bytes: 0,
            speed_bytes_per_sec: 0,
            eta_seconds: None,
            status: DownloadStatus::Queued,
            segments: Vec::new(),
        });

        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_default();

        let task = Self {
            id,
            url,
            output_dir,
            connection_count: connection_count.max(1).min(16),
            limiter,
            headers,
            client,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            status_tx,
        };

        (task, status_rx)
    }

    pub fn cancel(&self) {
        self.cancel_flag.store(true, Ordering::SeqCst);
    }

    /// Executes the download to completion or until paused/cancelled.
    pub async fn run(&mut self) -> Result<PathBuf, String> {
        self.cancel_flag.store(false, Ordering::SeqCst);

        // 1. Preflight check
        let preflight = check_url(&self.client, &self.url, self.headers.clone())
            .await
            .map_err(|e| format!("Preflight error: {}", e))?;

        let final_target_path = resolve_collision(&self.output_dir.join(&preflight.filename));
        let actual_filename = final_target_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let part_path = self.output_dir.join(format!("{}.part", actual_filename));
        let meta_path = self.output_dir.join(format!("{}.part.meta", actual_filename));

        // 2. Resume check
        let meta = if meta_path.exists() {
            if let Ok(loaded) = DownloadMeta::load_from_file(&meta_path) {
                if loaded.is_resumable(&preflight) {
                    loaded
                } else {
                    DownloadMeta::new(&preflight, self.connection_count)
                }
            } else {
                DownloadMeta::new(&preflight, self.connection_count)
            }
        } else {
            DownloadMeta::new(&preflight, self.connection_count)
        };

        // 3. Preallocate / open .part file
        let total_size = preflight.total_size.unwrap_or(0);
        let part_file = create_preallocated_part_file(&part_path, total_size)
            .map_err(|e| format!("Failed to create part file: {}", e))?;
        let shared_file = Arc::new(part_file);

        let shared_meta = Arc::new(Mutex::new(meta.clone()));
        let downloaded_counter = Arc::new(AtomicU64::new(meta.total_downloaded()));

        // Initial progress publish
        self.publish_progress(
            &actual_filename,
            preflight.total_size,
            downloaded_counter.load(Ordering::Relaxed),
            0,
            None,
            DownloadStatus::Downloading,
            &meta.segments,
        );

        // 4. Spawn segment workers
        let mut join_handles = Vec::new();

        for seg in meta.segments.iter() {
            if seg.is_completed() {
                continue;
            }

            let handle = tokio::spawn(run_segment_worker(
                self.client.clone(),
                preflight.final_url.clone(),
                self.headers.clone(),
                seg.index,
                shared_meta.clone(),
                shared_file.clone(),
                downloaded_counter.clone(),
                self.limiter.clone(),
                self.cancel_flag.clone(),
            ));
            join_handles.push(handle);
        }

        // 5. Progress monitoring & meta periodic flush loop
        let mut last_downloaded = downloaded_counter.load(Ordering::Relaxed);
        let mut last_speed_check = Instant::now();
        let mut last_meta_flush = Instant::now();

        let mut speed = 0u64;

        while !join_handles.is_empty() {
            tokio::time::sleep(Duration::from_millis(250)).await;

            // Check if cancelled/paused
            if self.cancel_flag.load(Ordering::Relaxed) {
                let _ = shared_meta.lock().await.save_to_file(&meta_path);
                self.publish_progress(
                    &actual_filename,
                    preflight.total_size,
                    downloaded_counter.load(Ordering::Relaxed),
                    0,
                    None,
                    DownloadStatus::Paused,
                    &shared_meta.lock().await.segments,
                );
                return Err("Download paused".to_string());
            }

            // Calculate Speed & ETA
            let now = Instant::now();
            let elapsed = now.duration_since(last_speed_check).as_secs_f64();
            if elapsed >= 1.0 {
                let current_bytes = downloaded_counter.load(Ordering::Relaxed);
                let delta = current_bytes.saturating_sub(last_downloaded);
                speed = (delta as f64 / elapsed) as u64;
                last_downloaded = current_bytes;
                last_speed_check = now;
            }

            let current_downloaded = downloaded_counter.load(Ordering::Relaxed);
            let eta = if speed > 0 && total_size > current_downloaded {
                Some((total_size - current_downloaded) / speed)
            } else {
                None
            };

            // Flush meta periodically
            if now.duration_since(last_meta_flush) >= META_FLUSH_INTERVAL {
                let _ = shared_meta.lock().await.save_to_file(&meta_path);
                last_meta_flush = now;
            }

            // Dynamic re-splitting check:
            // Check if any handle finished
            join_handles.retain(|h| !h.is_finished());

            if preflight.supports_range && join_handles.len() < self.connection_count {
                if let Some(new_seg) = try_split_slowest_segment(&shared_meta).await {
                    let handle = tokio::spawn(run_segment_worker(
                        self.client.clone(),
                        preflight.final_url.clone(),
                        self.headers.clone(),
                        new_seg.index,
                        shared_meta.clone(),
                        shared_file.clone(),
                        downloaded_counter.clone(),
                        self.limiter.clone(),
                        self.cancel_flag.clone(),
                    ));
                    join_handles.push(handle);
                }
            }

            let locked_meta = shared_meta.lock().await;
            self.publish_progress(
                &actual_filename,
                preflight.total_size,
                current_downloaded,
                speed,
                eta,
                DownloadStatus::Downloading,
                &locked_meta.segments,
            );
        }

        // 6. Finalization
        let final_meta = shared_meta.lock().await;
        let final_downloaded = final_meta.total_downloaded();

        if let Some(expected_total) = preflight.total_size {
            if final_downloaded < expected_total && preflight.supports_range {
                let _ = final_meta.save_to_file(&meta_path);
                return Err("Download incomplete: bytes mismatch".to_string());
            }
        }

        // Rename .part -> final filename and write MOTW
        finalize_download(&part_path, &final_target_path, Some(&meta_path), &self.url)
            .map_err(|e| format!("Failed to finalize download: {}", e))?;

        self.publish_progress(
            &actual_filename,
            preflight.total_size,
            final_downloaded,
            0,
            Some(0),
            DownloadStatus::Completed,
            &final_meta.segments,
        );

        Ok(final_target_path)
    }

    fn publish_progress(
        &self,
        filename: &str,
        total_size: Option<u64>,
        downloaded_bytes: u64,
        speed_bytes_per_sec: u64,
        eta_seconds: Option<u64>,
        status: DownloadStatus,
        segments: &[SegmentMeta],
    ) {
        let seg_progress = segments
            .iter()
            .map(|s| SegmentProgress {
                index: s.index,
                start: s.start,
                end: s.end,
                downloaded: s.downloaded,
            })
            .collect();

        let _ = self.status_tx.send(DownloadProgress {
            id: self.id.clone(),
            filename: filename.to_string(),
            total_size,
            downloaded_bytes,
            speed_bytes_per_sec,
            eta_seconds,
            status,
            segments: seg_progress,
        });
    }
}

async fn run_segment_worker(
    client: Client,
    url: String,
    custom_headers: Option<HeaderMap>,
    seg_index: usize,
    shared_meta: Arc<Mutex<DownloadMeta>>,
    file: Arc<File>,
    downloaded_counter: Arc<AtomicU64>,
    limiter: SpeedLimiter,
    cancel_flag: Arc<AtomicBool>,
) -> Result<(), String> {
    // Read segment boundaries
    let (start_offset, end_offset, current_downloaded) = {
        let meta = shared_meta.lock().await;
        let seg = meta.segments.iter().find(|s| s.index == seg_index).ok_or("Segment not found")?;
        (seg.start, seg.end, seg.downloaded)
    };

    let start_byte = start_offset + current_downloaded;
    if start_byte > end_offset && end_offset != u64::MAX {
        return Ok(()); // Already finished
    }

    let mut req = client.get(&url);
    if let Some(headers) = custom_headers {
        req = req.headers(headers);
    }

    if end_offset != u64::MAX {
        req = req.header(RANGE, format!("bytes={}-{}", start_byte, end_offset));
    }

    let res = req.send().await.map_err(|e| e.to_string())?;
    if !res.status().is_success() && res.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        return Err(format!("Worker HTTP status: {}", res.status()));
    }

    let mut stream = res.bytes_stream();
    let mut write_pos = start_byte;

    while let Some(chunk_res) = stream.next().await {
        if cancel_flag.load(Ordering::Relaxed) {
            break;
        }

        let chunk = chunk_res.map_err(|e| e.to_string())?;
        let chunk_len = chunk.len();

        limiter.consume(chunk_len).await;

        write_chunk_at(&file, write_pos, &chunk).map_err(|e| e.to_string())?;
        write_pos += chunk_len as u64;

        downloaded_counter.fetch_add(chunk_len as u64, Ordering::Relaxed);

        // Update segment progress in meta
        let mut meta = shared_meta.lock().await;
        if let Some(seg) = meta.segments.iter_mut().find(|s| s.index == seg_index) {
            seg.downloaded += chunk_len as u64;
            if seg.downloaded >= (seg.end - seg.start + 1) && seg.end != u64::MAX {
                break; // Reached end of segment
            }
        }
    }

    Ok(())
}

/// Dynamic re-splitting helper:
/// Finds an active segment that has a large remaining portion (> MIN_SPLIT_THRESHOLD),
/// truncates its end, and appends a new segment for the remaining half.
async fn try_split_slowest_segment(shared_meta: &Arc<Mutex<DownloadMeta>>) -> Option<SegmentMeta> {
    let mut meta = shared_meta.lock().await;
    let mut candidate_idx = None;
    let mut max_remaining = 0u64;

    for (i, seg) in meta.segments.iter().enumerate() {
        let remaining = seg.remaining_bytes();
        if remaining > MIN_SPLIT_THRESHOLD && remaining > max_remaining {
            max_remaining = remaining;
            candidate_idx = Some(i);
        }
    }

    if let Some(idx) = candidate_idx {
        let seg = &mut meta.segments[idx];
        let remaining = seg.remaining_bytes();
        let split_point = seg.current_offset() + (remaining / 2);

        let old_end = seg.end;
        seg.end = split_point;

        let new_index = meta.segments.len();
        let new_seg = SegmentMeta {
            index: new_index,
            start: split_point + 1,
            end: old_end,
            downloaded: 0,
        };

        meta.segments.push(new_seg.clone());
        return Some(new_seg);
    }

    None
}
