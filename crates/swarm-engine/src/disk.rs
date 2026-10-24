use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::fs::FileExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::System::IO::DeviceIoControl;
use windows_sys::Win32::System::Ioctl::FSCTL_SET_SPARSE;

/// Creates a `.part` file preallocated to `total_size`.
/// Applies NTFS `FSCTL_SET_SPARSE` when possible to make allocation instant and eliminate disk thrashing.
pub fn create_preallocated_part_file(path: &Path, total_size: u64) -> io::Result<File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;

    // Attempt to enable sparse file attribute on NTFS
    let handle = file.as_raw_handle() as HANDLE;
    unsafe {
        let mut bytes_returned = 0u32;
        DeviceIoControl(
            handle,
            FSCTL_SET_SPARSE,
            null(),
            0,
            null_mut(),
            0,
            &mut bytes_returned,
            null_mut(),
        );
    }

    // Set file length (instant on sparse NTFS files)
    if total_size > 0 {
        file.set_len(total_size)?;
    }

    Ok(file)
}

/// Writes a chunk of data at an exact byte offset without modifying the file's current position pointer.
/// Safe to be called concurrently from multiple segment tasks.
pub fn write_chunk_at(file: &File, offset: u64, buf: &[u8]) -> io::Result<usize> {
    file.seek_write(buf, offset)
}

/// Writes Mark-of-the-Web (Zone.Identifier Alternate Data Stream) on NTFS.
/// ZoneId=3 identifies the file as downloaded from the Internet zone.
pub fn write_mark_of_the_web(target_file: &Path, source_url: &str) -> io::Result<()> {
    let ads_path_str = format!("{}:Zone.Identifier", target_file.to_string_lossy());
    let content = format!(
        "[ZoneTransfer]\r\nZoneId=3\r\nHostUrl={}\r\n",
        source_url
    );

    // If target filesystem is not NTFS (e.g. FAT32/exFAT), writing ADS fails.
    // In that case, we silently ignore the error so download completes normally.
    let _ = std::fs::write(&ads_path_str, content.as_bytes());
    Ok(())
}

/// Finalizes a completed download:
/// 1. Renames `<name>.part` to `<name>`.
/// 2. Writes Mark-of-the-Web ADS.
/// 3. Deletes `<name>.part.meta` if present.
pub fn finalize_download(
    part_path: &Path,
    final_path: &Path,
    meta_path: Option<&Path>,
    source_url: &str,
) -> io::Result<()> {
    // If target final path already exists or if we need replacement
    if final_path.exists() {
        let _ = std::fs::remove_file(final_path);
    }

    std::fs::rename(part_path, final_path)?;
    let _ = write_mark_of_the_web(final_path, source_url);

    if let Some(meta) = meta_path {
        if meta.exists() {
            let _ = std::fs::remove_file(meta);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sparse_file_creation_and_random_writes() {
        let temp_dir = std::env::temp_dir().join("swarm_test_disk");
        let _ = std::fs::create_dir_all(&temp_dir);
        let part_file_path = temp_dir.join("test_file.bin.part");

        let total_size = 10 * 1024 * 1024; // 10 MB
        let file = create_preallocated_part_file(&part_file_path, total_size).unwrap();

        // Write at offset 0
        let chunk1 = b"HEAD_DATA";
        write_chunk_at(&file, 0, chunk1).unwrap();

        // Write at offset 5 MB
        let chunk2 = b"MID_DATA";
        let mid_offset = 5 * 1024 * 1024;
        write_chunk_at(&file, mid_offset, chunk2).unwrap();

        drop(file);

        // Verify written data
        let read_file = File::open(&part_file_path).unwrap();
        assert_eq!(read_file.metadata().unwrap().len(), total_size);
        let mut buf1 = vec![0u8; chunk1.len()];
        read_file.seek_read(&mut buf1, 0).unwrap();
        assert_eq!(&buf1, chunk1);

        let mut buf2 = vec![0u8; chunk2.len()];
        read_file.seek_read(&mut buf2, mid_offset).unwrap();
        assert_eq!(&buf2, chunk2);
        let final_path = temp_dir.join("test_file.bin");
        finalize_download(&part_file_path, &final_path, None, "https://example.com/test").unwrap();

        assert!(final_path.exists());
        assert!(!part_file_path.exists());

        // Clean up
        let _ = std::fs::remove_file(&final_path);
        let _ = std::fs::remove_dir(&temp_dir);
    }
}
