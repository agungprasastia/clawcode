use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub trait FileSystem {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>>;
    fn write(&self, path: &Path, bytes: &[u8]) -> io::Result<()>;
    fn replace(&self, from: &Path, to: &Path) -> io::Result<()>;
    fn remove_file(&self, path: &Path) -> io::Result<()>;
    fn exists(&self, path: &Path) -> bool;
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf>;
}

pub(crate) fn temporary_sibling(path: &Path, suffix: &str) -> io::Result<PathBuf> {
    static NEXT_TEMPORARY_ID: AtomicU64 = AtomicU64::new(0);
    let name = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "snapshot target has no file name",
        )
    })?;
    let operation_id = NEXT_TEMPORARY_ID.fetch_add(1, Ordering::Relaxed);
    Ok(path.with_file_name(format!(
        ".{}.{suffix}.{operation_id}.tmp",
        name.to_string_lossy()
    )))
}

pub struct RealFileSystem;

impl FileSystem for RealFileSystem {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        fs::read(path)
    }

    fn write(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        fs::write(path, bytes)
    }

    fn replace(&self, from: &Path, to: &Path) -> io::Result<()> {
        replace_file(from, to)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        fs::canonicalize(path)
    }
}

#[cfg(not(windows))]
fn replace_file(from: &Path, to: &Path) -> io::Result<()> {
    fs::rename(from, to)
}

#[cfg(windows)]
fn replace_file(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;

    unsafe extern "system" {
        fn MoveFileExW(
            existing_file_name: *const u16,
            new_file_name: *const u16,
            flags: u32,
        ) -> i32;
    }

    let from: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: Both paths are NUL-terminated UTF-16 buffers kept alive for call duration.
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_REPLACE_EXISTING) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
