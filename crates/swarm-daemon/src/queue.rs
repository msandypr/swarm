#![allow(dead_code)]
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::header::HeaderMap;
use serde::Deserialize;
use tokio::sync::{broadcast, Mutex};

use swarm_engine::engine::{DownloadProgress, DownloadStatus, DownloadTask};
use swarm_engine::limiter::SpeedLimiter;

use crate::store::{DownloadRecord, DownloadRecordStatus, StateMutation, StateStore};

pub struct ActiveDownload {
    pub id: String,
    pub task_handle: tokio::task::JoinHandle<()>,
    pub cancel_tx: tokio::sync::oneshot::Sender<()>,
}

#[derive(Clone)]
pub struct QueueManager {
    inner: Arc<Mutex<QueueInner>>,
    progress_tx: broadcast::Sender<DownloadProgress>,
}

struct QueueInner {
    store: StateStore,
    output_dir: PathBuf,
    active_downloads: HashMap<String, tokio::sync::oneshot::Sender<()>>,
    limiter: SpeedLimiter,
}

#[derive(Debug, Deserialize)]
pub struct AddDownloadRequest {
    pub url: String,
    pub connections: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct ExtAddDownloadRequest {
    pub url: String,
    pub referrer: Option<String>,
    pub user_agent: Option<String>,
    pub cookie: Option<String>,
    pub suggested_filename: Option<String>,
}

impl QueueManager {
    pub fn new(store: StateStore, output_dir: PathBuf) -> Self {
        let (progress_tx, _) = broadcast::channel(128);
        let inner = Arc::new(Mutex::new(QueueInner {
            store,
            output_dir,
            active_downloads: HashMap::new(),
            limiter: SpeedLimiter::new(None),
        }));

        Self { inner, progress_tx }
    }

    pub fn subscribe_progress(&self) -> broadcast::Receiver<DownloadProgress> {
        self.progress_tx.subscribe()
    }

    pub async fn list_downloads(&self) -> Vec<DownloadRecord> {
        let inner = self.inner.lock().await;
        inner.store.get_all()
    }

    pub async fn add_download(
        &self,
        url: String,
        headers: Option<HeaderMap>,
        connections: Option<usize>,
    ) -> Result<String, String> {
        let id = format!(
            "d_{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        );

        let output_dir = {
            let inner = self.inner.lock().await;
            inner.output_dir.clone()
        };

        let conn_count = connections.unwrap_or(8);
        let limiter = {
            let inner = self.inner.lock().await;
            inner.limiter.clone()
        };

        let (mut task, mut progress_rx) = DownloadTask::new(
            id.clone(),
            url.clone(),
            output_dir.clone(),
            conn_count,
            limiter,
            headers,
        );

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let initial_record = DownloadRecord {
            id: id.clone(),
            url: url.clone(),
            filename: String::new(),
            output_path: String::new(),
            total_size: None,
            downloaded_bytes: 0,
            status: DownloadRecordStatus::Downloading,
            created_at: now,
            completed_at: None,
        };

        {
            let mut inner = self.inner.lock().await;
            let _ = inner.store.append(StateMutation::Add {
                record: initial_record,
            });
        }

        let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel::<()>();

        {
            let mut inner = self.inner.lock().await;
            inner.active_downloads.insert(id.clone(), cancel_tx);
        }

        // Spawn download task in background
        let manager_clone = self.clone();
        let download_id = id.clone();

        tokio::spawn(async move {
            // Monitor progress in parallel
            let progress_id = download_id.clone();
            let p_tx = manager_clone.progress_tx.clone();
            let m_clone_for_rx = manager_clone.clone();

            let rx_handle = tokio::spawn(async move {
                while progress_rx.changed().await.is_ok() {
                    let p = progress_rx.borrow().clone();
                    let _ = p_tx.send(p.clone());

                    // Update store if status changed to completed or failed
                    if p.status == DownloadStatus::Completed || p.status == DownloadStatus::Failed {
                        let status = if p.status == DownloadStatus::Completed {
                            DownloadRecordStatus::Completed
                        } else {
                            DownloadRecordStatus::Failed
                        };
                        let now = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .ok();

                        let mut inner = m_clone_for_rx.inner.lock().await;
                        let _ = inner.store.append(StateMutation::UpdateStatus {
                            id: progress_id.clone(),
                            status,
                            downloaded_bytes: p.downloaded_bytes,
                            completed_at: now,
                        });
                        break;
                    }
                }
            });

            tokio::select! {
                _ = &mut cancel_rx => {
                    task.cancel();
                }
                res = task.run() => {
                    match res {
                        Ok(final_path) => {
                            let mut inner = manager_clone.inner.lock().await;
                            if let Some(rec) = inner.store.get(&download_id).cloned() {
                                let mut updated = rec;
                                updated.output_path = final_path.to_string_lossy().to_string();
                                updated.filename = final_path.file_name().unwrap_or_default().to_string_lossy().to_string();
                                updated.status = DownloadRecordStatus::Completed;
                                let _ = inner.store.append(StateMutation::Add { record: updated });
                            }
                        }
                        Err(_) => {}
                    }
                }
            }

            let _ = rx_handle.await;

            let mut inner = manager_clone.inner.lock().await;
            inner.active_downloads.remove(&download_id);
        });

        Ok(id)
    }

    pub async fn cancel_download(&self, id: &str) -> bool {
        let mut inner = self.inner.lock().await;
        if let Some(cancel_tx) = inner.active_downloads.remove(id) {
            let _ = cancel_tx.send(());
            let _ = inner.store.append(StateMutation::UpdateStatus {
                id: id.to_string(),
                status: DownloadRecordStatus::Paused,
                downloaded_bytes: 0,
                completed_at: None,
            });
            true
        } else {
            false
        }
    }

    pub async fn delete_download(&self, id: &str, delete_file: bool) -> bool {
        self.cancel_download(id).await;

        let mut inner = self.inner.lock().await;
        if let Some(rec) = inner.store.get(id).cloned() {
            if delete_file && !rec.output_path.is_empty() {
                let _ = std::fs::remove_file(&rec.output_path);
            }
            let _ = inner.store.append(StateMutation::Delete { id: id.to_string() });
            true
        } else {
            false
        }
    }
}
