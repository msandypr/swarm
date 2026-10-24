#![windows_subsystem = "windows"]

mod memory;
mod single_instance;
mod startup;
mod tray;
mod store;
mod runtime;
mod security;
mod queue;
mod api;

use single_instance::SingleInstance;
use tray::{open_ui, TrayManager};

const SINGLETON_MUTEX_NAME: &str = r"Local\Swarm-singleton";
const APP_NAME: &str = "Swarm";
const TRAY_TOOLTIP: &str = "Swarm Download Manager";
const DEFAULT_UI_URL: &str = "http://127.0.0.1:52417/";

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

    // 3. Initialize Tray
    let tray = match TrayManager::new(APP_NAME, TRAY_TOOLTIP) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Failed to initialize tray: {}", e);
            return;
        }
    };

    // 4. Initial memory trimming
    memory::trim_working_set();

    // 5. Open UI if not background startup
    if !is_background {
        open_ui(DEFAULT_UI_URL);
    }

    // 6. Run Win32 message loop (blocks at 0% CPU until exit)
    tray.run_message_loop();
}
