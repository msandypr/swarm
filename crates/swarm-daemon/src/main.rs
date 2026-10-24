#![windows_subsystem = "windows"]

mod api;
mod memory;
mod queue;
mod runtime;
mod security;
mod single_instance;
mod startup;
mod store;
mod tray;

use std::path::PathBuf;
use api::{create_router, AppState};
use queue::QueueManager;
use runtime::{cleanup_runtime, get_swarm_data_dir, init_runtime, DEFAULT_PORT};
use single_instance::SingleInstance;
use store::StateStore;
use tray::{get_ui_url, open_ui, set_ui_url, TrayManager};

const SINGLETON_MUTEX_NAME: &str = r"Local\Swarm-singleton";
const APP_NAME: &str = "Swarm";
const TRAY_TOOLTIP: &str = "Swarm Download Manager";

fn default_downloads_dir() -> PathBuf {
    if let Ok(profile) = std::env::var("USERPROFILE") {
        let p = PathBuf::from(profile).join("Downloads");
        if p.exists() {
            return p;
        }
    }
    PathBuf::from(r"C:\Downloads")
}

fn main() {
    // 1. Single instance check
    let _instance = match SingleInstance::acquire(SINGLETON_MUTEX_NAME) {
        Ok(instance) => instance,
        Err(_) => {
            // Already running; message sent to existing instance to open UI
            return;
        }
    };

    // 2. Parse arguments
    let args: Vec<String> = std::env::args().collect();
    let is_background = args.iter().any(|arg| arg == "--background");

    // 3. Initialize Runtime Credentials (port, token, pid)
    let runtime_info = match init_runtime(DEFAULT_PORT) {
        Ok(info) => info,
        Err(e) => {
            eprintln!("Failed to initialize runtime: {}", e);
            return;
        }
    };

    let ui_url = format!("http://127.0.0.1:{}/?t={}", runtime_info.port, runtime_info.token);
    set_ui_url(ui_url);

    // 4. Initialize State Store & Queue Manager
    let data_dir = get_swarm_data_dir();
    let store = match StateStore::open_or_create(&data_dir) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to open state store: {}", e);
            cleanup_runtime();
            return;
        }
    };

    let download_dir = default_downloads_dir();
    let queue = QueueManager::new(store, download_dir);

    // 5. Build Axum Router
    let app_state = AppState {
        runtime: runtime_info.clone(),
        queue,
    };
    let router = create_router(app_state);

    // 6. Spawn Tokio Runtime on a background thread with minimal stack size
    let port = runtime_info.port;
    let _server_thread = std::thread::Builder::new()
        .name("swarm-async-runtime".to_string())
        .stack_size(512 * 1024) // 512 KB stack
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to build current_thread tokio runtime");

            rt.block_on(async move {
                let addr = format!("127.0.0.1:{}", port);
                let listener = match tokio::net::TcpListener::bind(&addr).await {
                    Ok(l) => l,
                    Err(e) => {
                        eprintln!("Failed to bind HTTP listener to {}: {}", addr, e);
                        return;
                    }
                };

                // Periodic memory trimmer every 30 seconds
                tokio::spawn(async {
                    let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
                    loop {
                        interval.tick().await;
                        memory::trim_working_set();
                    }
                });

                let _ = axum::serve(listener, router).await;
            });
        })
        .expect("Failed to spawn runtime thread");

    // 7. Initialize Tray
    let tray = match TrayManager::new(APP_NAME, TRAY_TOOLTIP) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Failed to initialize tray: {}", e);
            cleanup_runtime();
            return;
        }
    };

    // 8. Initial memory trimming
    memory::trim_working_set();

    // 9. Open UI if not background startup
    if !is_background {
        open_ui(get_ui_url());
    }

    // 10. Run Win32 message loop (blocks at 0% CPU until exit)
    tray.run_message_loop();

    // 11. Cleanup runtime credentials on exit
    cleanup_runtime();
}
