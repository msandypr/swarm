#![allow(dead_code)]
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_SZ,
};

const RUN_KEY_PATH: &[u16] = &[
    'S' as u16, 'o' as u16, 'f' as u16, 't' as u16, 'w' as u16, 'a' as u16, 'r' as u16, 'e' as u16,
    '\\' as u16, 'M' as u16, 'i' as u16, 'c' as u16, 'r' as u16, 'o' as u16, 's' as u16, 'o' as u16,
    'f' as u16, 't' as u16, '\\' as u16, 'W' as u16, 'i' as u16, 'n' as u16, 'd' as u16, 'o' as u16,
    'w' as u16, 's' as u16, '\\' as u16, 'C' as u16, 'u' as u16, 'r' as u16, 'r' as u16, 'e' as u16,
    'n' as u16, 't' as u16, 'V' as u16, 'e' as u16, 'r' as u16, 's' as u16, 'i' as u16, 'o' as u16,
    'n' as u16, '\\' as u16, 'R' as u16, 'u' as u16, 'n' as u16, 0,
];

const STARTUP_APPROVED_PATH: &[u16] = &[
    'S' as u16, 'o' as u16, 'f' as u16, 't' as u16, 'w' as u16, 'a' as u16, 'r' as u16, 'e' as u16,
    '\\' as u16, 'M' as u16, 'i' as u16, 'c' as u16, 'r' as u16, 'o' as u16, 's' as u16, 'o' as u16,
    'f' as u16, 't' as u16, '\\' as u16, 'W' as u16, 'i' as u16, 'n' as u16, 'd' as u16, 'o' as u16,
    'w' as u16, 's' as u16, '\\' as u16, 'C' as u16, 'u' as u16, 'r' as u16, 'r' as u16, 'e' as u16,
    'n' as u16, 't' as u16, 'V' as u16, 'e' as u16, 'r' as u16, 's' as u16, 'i' as u16, 'o' as u16,
    'n' as u16, '\\' as u16, 'E' as u16, 'x' as u16, 'p' as u16, 'l' as u16, 'o' as u16, 'r' as u16,
    'e' as u16, 'r' as u16, '\\' as u16, 'S' as u16, 't' as u16, 'a' as u16, 'r' as u16, 't' as u16,
    'u' as u16, 'p' as u16, 'A' as u16, 'p' as u16, 'p' as u16, 'r' as u16, 'o' as u16, 'v' as u16,
    'e' as u16, 'd' as u16, '\\' as u16, 'R' as u16, 'u' as u16, 'n' as u16, 0,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupStatus {
    Enabled,
    DisabledInTaskManager,
    NotRegistered,
}

fn to_wide(s: &str) -> Vec<u16> {
    let mut v: Vec<u16> = s.encode_utf16().collect();
    v.push(0);
    v
}

/// Checks startup status for `app_name`.
pub fn get_startup_status(app_name: &str) -> StartupStatus {
    let name_wide = to_wide(app_name);
    unsafe {
        let mut hkey: HKEY = null_mut();
        // 1. Check HKCU\...\Run
        if RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY_PATH.as_ptr(), 0, KEY_READ, &mut hkey) != ERROR_SUCCESS {
            return StartupStatus::NotRegistered;
        }

        let mut data_len = 0u32;
        let query_res = RegQueryValueExW(
            hkey,
            name_wide.as_ptr(),
            null_mut(),
            null_mut(),
            null_mut(),
            &mut data_len,
        );
        RegCloseKey(hkey);

        if query_res != ERROR_SUCCESS {
            return StartupStatus::NotRegistered;
        }

        // 2. Check StartupApproved\Run
        let mut approved_key: HKEY = null_mut();
        if RegOpenKeyExW(HKEY_CURRENT_USER, STARTUP_APPROVED_PATH.as_ptr(), 0, KEY_READ, &mut approved_key) == ERROR_SUCCESS {
            let mut binary_data = [0u8; 12];
            let mut len = binary_data.len() as u32;
            let mut val_type = 0u32;
            let res = RegQueryValueExW(
                approved_key,
                name_wide.as_ptr(),
                null_mut(),
                &mut val_type,
                binary_data.as_mut_ptr(),
                &mut len,
            );
            RegCloseKey(approved_key);

            if res == ERROR_SUCCESS && len > 0 {
                // Byte 0: 0x02 or 0x06 = enabled, 0x03 = disabled by user in Task Manager
                if binary_data[0] == 0x03 {
                    return StartupStatus::DisabledInTaskManager;
                }
            }
        }

        StartupStatus::Enabled
    }
}

/// Registers or unregisters the app in HKCU Run.
pub fn set_startup_registration(app_name: &str, exe_path: &str, enable: bool) -> Result<(), u32> {
    let name_wide = to_wide(app_name);
    unsafe {
        let mut hkey: HKEY = null_mut();
        let res = RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY_PATH.as_ptr(), 0, KEY_WRITE, &mut hkey);
        if res != ERROR_SUCCESS {
            return Err(res);
        }

        let result = if enable {
            let cmd = format!("\"{}\" --background", exe_path);
            let cmd_wide = to_wide(&cmd);
            RegSetValueExW(
                hkey,
                name_wide.as_ptr(),
                0,
                REG_SZ,
                cmd_wide.as_ptr() as *const u8,
                (cmd_wide.len() * 2) as u32,
            )
        } else {
            let del_res = RegDeleteValueW(hkey, name_wide.as_ptr());
            if del_res == ERROR_FILE_NOT_FOUND {
                ERROR_SUCCESS
            } else {
                del_res
            }
        };

        RegCloseKey(hkey);

        if result == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(result)
        }
    }
}
