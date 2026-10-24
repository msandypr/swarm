
use std::mem::zeroed;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::UI::Shell::{
    ShellExecuteW, Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO,
    NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, GetCursorPos, GetMessageW, LoadIconW, PostMessageW, PostQuitMessage,
    RegisterClassW, RegisterWindowMessageW, SetForegroundWindow, TrackPopupMenu, TranslateMessage,
    CS_HREDRAW, CS_VREDRAW, HICON, HMENU, IDI_APPLICATION, MF_SEPARATOR, MF_STRING, MSG,
    SW_SHOWNORMAL, TPM_BOTTOMALIGN, TPM_RIGHTBUTTON, WM_APP, WM_COMMAND, WM_DESTROY,
    WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP, WNDCLASSW,
};

use crate::single_instance::{SWARM_WINDOW_CLASS, WM_SWARM_OPEN_UI};

pub const WM_SWARM_TRAY: u32 = WM_APP + 100;
pub const WM_SWARM_EXIT: u32 = WM_APP + 102;

const IDM_OPEN: usize = 1001;
const IDM_PAUSE_RESUME_ALL: usize = 1002;
const IDM_OPEN_FOLDER: usize = 1003;
const IDM_EXIT: usize = 1004;

static ALL_PAUSED: AtomicBool = AtomicBool::new(false);
static mut TASKBAR_CREATED_MSG: u32 = 0;
static mut GLOBAL_HWND: HWND = null_mut();

fn to_wide(s: &str) -> Vec<u16> {
    let mut v: Vec<u16> = s.encode_utf16().collect();
    v.push(0);
    v
}

fn copy_slice_to_wide_buf(dest: &mut [u16], src: &str) {
    let mut i = 0;
    for c in src.encode_utf16() {
        if i >= dest.len() - 1 {
            break;
        }
        dest[i] = c;
        i += 1;
    }
    if i < dest.len() {
        dest[i] = 0;
    }
}

pub struct TrayManager {
    hwnd: HWND,
    nid: NOTIFYICONDATAW,
}

impl TrayManager {
    pub fn new(app_name: &str, tooltip: &str) -> Result<Self, &'static str> {
        unsafe {
            let taskbar_str = to_wide("TaskbarCreated");
            TASKBAR_CREATED_MSG = RegisterWindowMessageW(taskbar_str.as_ptr());

            let hinstance = null_mut();
            let icon: HICON = LoadIconW(null_mut(), IDI_APPLICATION);

            let wc = WNDCLASSW {
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(tray_window_proc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: hinstance,
                hIcon: icon,
                hCursor: null_mut(),
                hbrBackground: null_mut(),
                lpszMenuName: null(),
                lpszClassName: SWARM_WINDOW_CLASS.as_ptr(),
            };

            RegisterClassW(&wc);

            let app_name_wide = to_wide(app_name);
            let hwnd = CreateWindowExW(
                0,
                SWARM_WINDOW_CLASS.as_ptr(),
                app_name_wide.as_ptr(),
                0,
                0,
                0,
                0,
                0,
                null_mut(),
                null_mut(),
                hinstance,
                null(),
            );

            if hwnd.is_null() {
                return Err("Failed to create message window");
            }

            GLOBAL_HWND = hwnd;

            let mut nid: NOTIFYICONDATAW = zeroed();
            nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
            nid.hWnd = hwnd;
            nid.uID = 1;
            nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
            nid.uCallbackMessage = WM_SWARM_TRAY;
            nid.hIcon = icon;
            copy_slice_to_wide_buf(&mut nid.szTip, tooltip);

            if Shell_NotifyIconW(NIM_ADD, &nid) == 0 {
                DestroyWindow(hwnd);
                return Err("Failed to register tray icon");
            }

            Ok(Self { hwnd, nid })
        }
    }
    #[allow(dead_code)]

    pub fn show_balloon(&mut self, title: &str, message: &str) {
        unsafe {
            self.nid.uFlags |= NIF_INFO;
            copy_slice_to_wide_buf(&mut self.nid.szInfoTitle, title);
            copy_slice_to_wide_buf(&mut self.nid.szInfo, message);
            self.nid.dwInfoFlags = NIIF_INFO;

            Shell_NotifyIconW(NIM_MODIFY, &self.nid);
        }
    }

    pub fn run_message_loop(&self) {
        unsafe {
            let mut msg: MSG = zeroed();
            while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}

impl Drop for TrayManager {
    fn drop(&mut self) {
        unsafe {
            Shell_NotifyIconW(NIM_DELETE, &self.nid);
            if !self.hwnd.is_null() {
                DestroyWindow(self.hwnd);
            }
        }
    }
}

/// Launches the UI in Microsoft Edge (--app mode) or default browser.
pub fn open_ui(url: &str) {
    let edge_paths = [
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
    ];

    let mut launched = false;
    for path in &edge_paths {
        if std::path::Path::new(path).exists() {
            let app_arg = format!("--app={}", url);
            let _ = std::process::Command::new(path).arg(app_arg).spawn();
            launched = true;
            break;
        }
    }

    if !launched {
        // Fallback to ShellExecuteW with default browser
        unsafe {
            let url_wide = to_wide(url);
            let open_str = to_wide("open");
            ShellExecuteW(
                null_mut(),
                open_str.as_ptr(),
                url_wide.as_ptr(),
                null(),
                null(),
                SW_SHOWNORMAL,
            );
        }
    }
}

/// Opens the user's Downloads folder in Windows Explorer.
pub fn open_downloads_folder() {
    let download_dir = dirs_downloads_folder();
    unsafe {
        let path_wide = to_wide(&download_dir);
        let open_str = to_wide("open");
        ShellExecuteW(
            null_mut(),
            open_str.as_ptr(),
            path_wide.as_ptr(),
            null(),
            null(),
            SW_SHOWNORMAL,
        );
    }
}

fn dirs_downloads_folder() -> String {
    if let Ok(profile) = std::env::var("USERPROFILE") {
        let p = format!(r"{}\Downloads", profile);
        if std::path::Path::new(&p).exists() {
            return p;
        }
    }
    r"C:\Downloads".to_string()
}

unsafe extern "system" fn tray_window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_SWARM_TRAY => {
            let lparam_msg = (lparam & 0xFFFF) as u32;
            match lparam_msg {
                WM_LBUTTONUP | WM_LBUTTONDBLCLK => {
                    open_ui("http://127.0.0.1:52417/");
                }
                WM_RBUTTONUP => {
                    show_tray_menu(hwnd);
                }
                _ => {}
            }
            0
        }
        WM_COMMAND => {
            let id = (wparam & 0xFFFF) as usize;
            match id {
                IDM_OPEN => {
                    open_ui("http://127.0.0.1:52417/");
                }
                IDM_PAUSE_RESUME_ALL => {
                    let paused = ALL_PAUSED.fetch_xor(true, Ordering::SeqCst);
                    // Toggle pause/resume signal
                    println!("Toggle Pause/Resume: {}", !paused);
                }
                IDM_OPEN_FOLDER => {
                    open_downloads_folder();
                }
                IDM_EXIT => {
                    PostQuitMessage(0);
                }
                _ => {}
            }
            0
        }
        WM_SWARM_OPEN_UI => {
            open_ui("http://127.0.0.1:52417/");
            0
        }
        WM_SWARM_EXIT => {
            PostQuitMessage(0);
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ if msg == TASKBAR_CREATED_MSG && TASKBAR_CREATED_MSG != 0 => {
            // Taskbar restarted (e.g. explorer.exe restart), re-register icon
            let icon: HICON = LoadIconW(null_mut(), IDI_APPLICATION);
            let mut nid: NOTIFYICONDATAW = zeroed();
            nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
            nid.hWnd = hwnd;
            nid.uID = 1;
            nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
            nid.uCallbackMessage = WM_SWARM_TRAY;
            nid.hIcon = icon;
            copy_slice_to_wide_buf(&mut nid.szTip, "Swarm Download Manager");
            Shell_NotifyIconW(NIM_ADD, &nid);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn show_tray_menu(hwnd: HWND) {
    let hmenu: HMENU = CreatePopupMenu();
    if hmenu.is_null() {
        return;
    }

    let open_str = to_wide("Open Swarm");
    AppendMenuW(hmenu, MF_STRING, IDM_OPEN, open_str.as_ptr());

    let paused = ALL_PAUSED.load(Ordering::SeqCst);
    let pause_resume_label = if paused {
        to_wide("Resume All")
    } else {
        to_wide("Pause All")
    };
    AppendMenuW(hmenu, MF_STRING, IDM_PAUSE_RESUME_ALL, pause_resume_label.as_ptr());

    let folder_str = to_wide("Open Downloads Folder");
    AppendMenuW(hmenu, MF_STRING, IDM_OPEN_FOLDER, folder_str.as_ptr());

    AppendMenuW(hmenu, MF_SEPARATOR, 0, null());

    let exit_str = to_wide("Exit");
    AppendMenuW(hmenu, MF_STRING, IDM_EXIT, exit_str.as_ptr());

    let mut pt: POINT = zeroed();
    GetCursorPos(&mut pt);

    SetForegroundWindow(hwnd);
    TrackPopupMenu(
        hmenu,
        TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
        pt.x,
        pt.y,
        0,
        hwnd,
        null(),
    );
    PostMessageW(hwnd, WM_NULL, 0, 0);
    DestroyMenu(hmenu);
}
