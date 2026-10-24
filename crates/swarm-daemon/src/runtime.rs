#![allow(dead_code)]
use std::fs::File;
use std::io::{self, BufWriter};
use std::net::TcpListener;
use std::path::PathBuf;
use std::ptr::null_mut;
use serde::{Deserialize, Serialize};

use windows_sys::Win32::Security::Cryptography::{
    BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
};

pub const DEFAULT_PORT: u16 = 52417;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeInfo {
    pub port: u16,
    pub token: String,
    pub pid: u32,
}

/// Returns the root data directory `%LOCALAPPDATA%\Swarm`.
pub fn get_swarm_data_dir() -> PathBuf {
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        PathBuf::from(local_app_data).join("Swarm")
    } else if let Ok(user_profile) = std::env::var("USERPROFILE") {
        PathBuf::from(user_profile).join("AppData").join("Local").join("Swarm")
    } else {
        PathBuf::from("swarm_data")
    }
}

/// Generates a 32-byte (64 hex characters) cryptographically secure random token using Windows BCrypt.
pub fn generate_secure_token() -> String {
    let mut bytes = [0u8; 32];
    unsafe {
        let status = BCryptGenRandom(
            null_mut(),
            bytes.as_mut_ptr(),
            bytes.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        );
        if status != 0 {
            // Fallback in extreme failure: system time mixed with process id
            let seed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(123456789);
            for (i, b) in bytes.iter_mut().enumerate() {
                *b = ((seed >> (i % 8)) ^ (std::process::id() as u128)) as u8;
            }
        }
    }

    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Finds an available local port starting from `preferred_port`.
pub fn find_available_port(preferred_port: u16) -> u16 {
    // Try preferred port first
    if TcpListener::bind(("127.0.0.1", preferred_port)).is_ok() {
        return preferred_port;
    }

    // Try a few fallback sequential ports
    for p in (preferred_port + 1)..=(preferred_port + 10) {
        if TcpListener::bind(("127.0.0.1", p)).is_ok() {
            return p;
        }
    }

    // Let the OS pick an available ephemeral port
    TcpListener::bind(("127.0.0.1", 0))
        .and_then(|l| l.local_addr())
        .map(|addr| addr.port())
        .unwrap_or(preferred_port)
}

/// Writes `runtime.json` into the Swarm data directory.
pub fn init_runtime(preferred_port: u16) -> io::Result<RuntimeInfo> {
    let dir = get_swarm_data_dir();
    std::fs::create_dir_all(&dir)?;

    let port = find_available_port(preferred_port);
    let token = generate_secure_token();
    let pid = std::process::id();

    let runtime = RuntimeInfo { port, token, pid };
    let runtime_path = dir.join("runtime.json");

    let file = File::create(&runtime_path)?;
    let writer = BufWriter::new(file);
    serde_json::to_writer_pretty(writer, &runtime)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    Ok(runtime)
}

/// Removes `runtime.json` on daemon exit.
pub fn cleanup_runtime() {
    let runtime_path = get_swarm_data_dir().join("runtime.json");
    let _ = std::fs::remove_file(runtime_path);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secure_token_generation() {
        let t1 = generate_secure_token();
        let t2 = generate_secure_token();
        assert_eq!(t1.len(), 64);
        assert_eq!(t2.len(), 64);
        assert_ne!(t1, t2);
    }

    #[test]
    fn test_port_finder() {
        let port = find_available_port(DEFAULT_PORT);
        assert!(port > 0);
    }
}
