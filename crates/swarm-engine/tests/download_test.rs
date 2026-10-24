use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use swarm_engine::engine::{DownloadStatus, DownloadTask};
use swarm_engine::limiter::SpeedLimiter;

/// Starts a mock HTTP server on 127.0.0.1 that supports HTTP Range requests.
async fn start_mock_range_server(payload: Arc<Vec<u8>>) -> (String, tokio::sync::oneshot::Sender<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let url = format!("http://127.0.0.1:{}/test_file.bin", port);

    let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut shutdown_rx => break,
                res = listener.accept() => {
                    let (mut socket, _) = match res {
                        Ok(s) => s,
                        Err(_) => break,
                    };
                    let data = payload.clone();

                    tokio::spawn(async move {
                        let mut buf = vec![0u8; 4096];
                        let n = match socket.read(&mut buf).await {
                            Ok(n) if n > 0 => n,
                            _ => return,
                        };

                        let req_str = String::from_utf8_lossy(&buf[..n]);
                        let total_len = data.len();

                        // Parse Range header if present: Range: bytes=X-Y or bytes=X-
                        let mut range_header = None;
                        for line in req_str.lines() {
                            if line.to_lowercase().starts_with("range: bytes=") {
                                range_header = Some(line["range: bytes=".len()..].trim().to_string());
                                break;
                            }
                        }

                        if let Some(r) = range_header {
                            let parts: Vec<&str> = r.split('-').collect();
                            let start = parts[0].parse::<usize>().unwrap_or(0);
                            let end = if parts.len() > 1 && !parts[1].is_empty() {
                                parts[1].parse::<usize>().unwrap_or(total_len - 1).min(total_len - 1)
                            } else {
                                total_len - 1
                            };

                            let slice = if start <= end && start < total_len {
                                &data[start..=end]
                            } else {
                                &[]
                            };

                            let header = format!(
                                "HTTP/1.1 206 Partial Content\r\n\
                                Content-Type: application/octet-stream\r\n\
                                Content-Disposition: attachment; filename=\"smoke_artifact.bin\"\r\n\
                                Content-Range: bytes {}-{}/{}\r\n\
                                Content-Length: {}\r\n\
                                ETag: \"smoke-etag-999\"\r\n\
                                Connection: close\r\n\r\n",
                                start, end, total_len, slice.len()
                            );

                            let _ = socket.write_all(header.as_bytes()).await;
                            let _ = socket.write_all(slice).await;
                        } else {
                            // Full response
                            let header = format!(
                                "HTTP/1.1 200 OK\r\n\
                                Accept-Ranges: bytes\r\n\
                                Content-Type: application/octet-stream\r\n\
                                Content-Disposition: attachment; filename=\"smoke_artifact.bin\"\r\n\
                                Content-Length: {}\r\n\
                                ETag: \"smoke-etag-999\"\r\n\
                                Connection: close\r\n\r\n",
                                total_len
                            );

                            let _ = socket.write_all(header.as_bytes()).await;
                            let _ = socket.write_all(&data).await;
                        }
                    });
                }
            }
        }
    });

    (url, shutdown_tx)
}

#[tokio::test]
async fn test_full_segmented_download_and_motw() {
    let size = 256 * 1024; // 256 KB
    let mut payload = Vec::with_capacity(size);
    for i in 0..size {
        payload.push((i % 251) as u8);
    }
    let payload = Arc::new(payload);

    let (server_url, shutdown_tx) = start_mock_range_server(payload.clone()).await;

    let test_dir = std::env::temp_dir().join("swarm_test_smoke");
    let _ = std::fs::create_dir_all(&test_dir);

    let limiter = SpeedLimiter::new(None);
    let (mut task, mut rx) = DownloadTask::new(
        "test_task_1".to_string(),
        server_url.clone(),
        test_dir.clone(),
        4, // 4 segments
        limiter,
        None,
    );

    // Monitor progress task
    let monitor_handle = tokio::spawn(async move {
        while rx.changed().await.is_ok() {
            let progress = rx.borrow().clone();
            if progress.status == DownloadStatus::Completed {
                break;
            }
        }
    });

    let result = task.run().await;
    assert!(result.is_ok(), "Download failed: {:?}", result.err());

    let final_path = result.unwrap();
    assert!(final_path.exists());
    assert_eq!(final_path.file_name().unwrap(), "smoke_artifact.bin");

    // Verify downloaded content matches byte-for-byte
    let downloaded_data = std::fs::read(&final_path).unwrap();
    assert_eq!(downloaded_data.len(), payload.len());
    assert_eq!(&downloaded_data, payload.as_ref());

    // Verify .part and .part.meta were cleaned up
    let part_path = test_dir.join("smoke_artifact.bin.part");
    let meta_path = test_dir.join("smoke_artifact.bin.part.meta");
    assert!(!part_path.exists());
    assert!(!meta_path.exists());

    // Verify Zone.Identifier (MOTW) exists and has ZoneId=3
    let ads_path = format!("{}:Zone.Identifier", final_path.to_string_lossy());
    if let Ok(ads_content) = std::fs::read_to_string(&ads_path) {
        assert!(ads_content.contains("ZoneId=3"));
        assert!(ads_content.contains(&server_url));
    }

    let _ = monitor_handle.await;
    let _ = shutdown_tx.send(());

    // Cleanup
    let _ = std::fs::remove_file(&final_path);
    let _ = std::fs::remove_dir(&test_dir);
}

#[tokio::test]
async fn test_pause_and_resume_download() {
    let size = 512 * 1024; // 512 KB
    let mut payload = Vec::with_capacity(size);
    for i in 0..size {
        payload.push(((i * 7) % 255) as u8);
    }
    let payload = Arc::new(payload);

    let (server_url, shutdown_tx) = start_mock_range_server(payload.clone()).await;

    let test_dir = std::env::temp_dir().join("swarm_test_resume");
    let _ = std::fs::create_dir_all(&test_dir);

    // 1. Start download with slow limiter so we can pause mid-flight
    let limiter = SpeedLimiter::new(Some(64 * 1024)); // 64 KB/s
    let (mut task1, mut rx1) = DownloadTask::new(
        "test_task_resume".to_string(),
        server_url.clone(),
        test_dir.clone(),
        4,
        limiter,
        None,
    );

    // Spawn task1 in background
    let task1_handle = tokio::spawn(async move {
        task1.run().await
    });

    // Wait until at least some bytes are downloaded
    while rx1.changed().await.is_ok() {
        let progress = rx1.borrow().clone();
        if progress.downloaded_bytes > 32 * 1024 {
            break;
        }
    }

    // Trigger cancel/pause on task1
    task1_handle.abort();
    tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

    // 2. Resume download with full speed
    let fast_limiter = SpeedLimiter::new(None);
    let (mut task2, _rx2) = DownloadTask::new(
        "test_task_resume".to_string(),
        server_url.clone(),
        test_dir.clone(),
        4,
        fast_limiter,
        None,
    );

    let resume_res = task2.run().await;
    assert!(resume_res.is_ok(), "Resume failed: {:?}", resume_res.err());

    let final_path = resume_res.unwrap();
    assert!(final_path.exists());

    let downloaded_data = std::fs::read(&final_path).unwrap();
    assert_eq!(downloaded_data.len(), payload.len());
    assert_eq!(&downloaded_data, payload.as_ref());

    let _ = shutdown_tx.send(());
    let _ = std::fs::remove_file(&final_path);
    let _ = std::fs::remove_dir(&test_dir);
}
