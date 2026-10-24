use std::ptr::null_mut;
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE, ERROR_ALREADY_EXISTS};
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW};

pub const SWARM_WINDOW_CLASS: &[u16] = &[
    'S' as u16, 'w' as u16, 'a' as u16, 'r' as u16, 'm' as u16, 'T' as u16, 'r' as u16, 'a' as u16,
    'y' as u16, 'W' as u16, 'n' as u16, 'd' as u16, 0,
];

pub const WM_SWARM_OPEN_UI: u32 = 0x8000 + 101; // WM_APP + 101

pub struct SingleInstance {
    handle: HANDLE,
}

impl SingleInstance {
    /// Attempts to acquire the single-instance mutex.
    /// Returns Ok(SingleInstance) if this is the first instance.
    /// If another instance is running, sends an open UI message to it and returns Err(()).
    pub fn acquire(name: &str) -> Result<Self, ()> {
        let mut name_wide: Vec<u16> = name.encode_utf16().collect();
        name_wide.push(0);

        unsafe {
            let handle = CreateMutexW(null_mut(), 1, name_wide.as_ptr());
            if handle.is_null() {
                return Err(());
            }

            if GetLastError() == ERROR_ALREADY_EXISTS {
                // Another instance is already running
                CloseHandle(handle);

                // Notify running instance to open UI
                let hwnd = FindWindowW(SWARM_WINDOW_CLASS.as_ptr(), null_mut());
                if !hwnd.is_null() {
                    PostMessageW(hwnd, WM_SWARM_OPEN_UI, 0, 0);
                }

                return Err(());
            }

            Ok(Self { handle })
        }
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        unsafe {
            if !self.handle.is_null() {
                CloseHandle(self.handle);
            }
        }
    }
}
