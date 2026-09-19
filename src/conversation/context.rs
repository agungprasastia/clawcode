//! Context Epoch and instruction source discovery.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Maximum bytes read from a single instruction file (32 KiB).
pub const MAX_INSTRUCTION_FILE_BYTES: usize = 32 * 1024;

/// Stable identifier for the workspace instruction source.
pub const INSTRUCTION_SOURCE_KEY: &str = "agents_md";

/// Truncate UTF-8 bytes to at most `max_bytes` without slicing multi-byte characters.
pub fn bound_utf8_bytes(bytes: &[u8], max_bytes: usize) -> &[u8] {
    if bytes.len() <= max_bytes {
        return bytes;
    }
    let mut end = max_bytes;
    while end > 0 && (bytes[end] & 0xC0) == 0x80 {
        end -= 1;
    }
    &bytes[..end]
}

/// Abstract file reader for testing and isolation.
pub trait FileReader: Send + Sync {
    fn read(&self, path: &Path) -> Result<Vec<u8>, std::io::Error>;
}

/// Standard file reader using [`std::fs::read`].
#[derive(Debug, Default, Clone, Copy)]
pub struct OsFileReader;

impl FileReader for OsFileReader {
    fn read(&self, path: &Path) -> Result<Vec<u8>, std::io::Error> {
        std::fs::read(path)
    }
}

/// Snapshot entry for one instruction file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstructionFileEntry {
    pub path: String,
    pub hash: String,
    pub size: usize,
}

/// Deterministic JSON snapshot of instruction files.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstructionSnapshot {
    pub source_key: String,
    pub files: Vec<InstructionFileEntry>,
}

/// Payload emitted by [`InstructionSource`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstructionPayload {
    pub source_key: String,
    pub snapshot_json: String,
    pub aggregate_text: String,
    pub baseline_bytes: Vec<u8>,
}

/// Error loading instruction source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstructionError {
    Unavailable(String),
}

impl std::fmt::Display for InstructionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(msg) => write!(f, "instruction source unavailable: {msg}"),
        }
    }
}

impl std::error::Error for InstructionError {}

/// Discovers and reads bounded `AGENTS.md` files from workspace root and parent chain.
pub struct InstructionSource<R = OsFileReader> {
    workspace_root: PathBuf,
    max_file_bytes: usize,
    reader: R,
}

impl InstructionSource<OsFileReader> {
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            workspace_root: workspace_root.into(),
            max_file_bytes: MAX_INSTRUCTION_FILE_BYTES,
            reader: OsFileReader,
        }
    }
}

impl<R: FileReader> InstructionSource<R> {
    pub fn with_reader(workspace_root: impl Into<PathBuf>, reader: R) -> Self {
        Self {
            workspace_root: workspace_root.into(),
            max_file_bytes: MAX_INSTRUCTION_FILE_BYTES,
            reader,
        }
    }

    pub fn with_max_bytes(mut self, max_bytes: usize) -> Self {
        self.max_file_bytes = max_bytes;
        self
    }

    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    /// Walk from workspace root up through the parent directory chain.
    pub fn candidate_directories(&self) -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        let mut cur = Some(self.workspace_root.clone());
        let mut depth = 0;
        while let Some(dir) = cur {
            dirs.push(dir.clone());
            depth += 1;
            if depth >= 64 {
                break;
            }
            cur = dir.parent().map(|p| p.to_path_buf());
        }
        dirs
    }

    /// Load and aggregate all discovered `AGENTS.md` files in deterministic order.
    pub fn load(&self) -> Result<InstructionPayload, InstructionError> {
        let mut entries = Vec::new();
        let mut sections = Vec::new();

        for dir in self.candidate_directories() {
            let path = dir.join("AGENTS.md");
            match self.reader.read(&path) {
                Ok(bytes) => {
                    let bounded = bound_utf8_bytes(&bytes, self.max_file_bytes);
                    let text = String::from_utf8_lossy(bounded).to_string();
                    let hash = format!("{:x}", Sha256::digest(bounded));
                    let size = bounded.len();
                    let normalized_path = path.to_string_lossy().to_string();

                    sections.push(format!(
                        "# Project Instructions ({})\n{}",
                        normalized_path, text
                    ));

                    entries.push(InstructionFileEntry {
                        path: normalized_path,
                        hash,
                        size,
                    });
                }
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    // Valid absence: not an error.
                    continue;
                }
                Err(err) => {
                    return Err(InstructionError::Unavailable(format!(
                        "failed to read {}: {err}",
                        path.display()
                    )));
                }
            }
        }

        let snapshot = InstructionSnapshot {
            source_key: INSTRUCTION_SOURCE_KEY.to_string(),
            files: entries,
        };
        let snapshot_json = serde_json::to_string(&snapshot).unwrap_or_default();
        let aggregate_text = sections.join("\n\n");
        let baseline_bytes = aggregate_text.as_bytes().to_vec();

        Ok(InstructionPayload {
            source_key: INSTRUCTION_SOURCE_KEY.to_string(),
            snapshot_json,
            aggregate_text,
            baseline_bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    struct MockReader {
        files: Mutex<HashMap<PathBuf, Result<Vec<u8>, std::io::ErrorKind>>>,
    }

    impl MockReader {
        fn new() -> Self {
            Self {
                files: Mutex::new(HashMap::new()),
            }
        }

        fn set_file(&self, path: impl Into<PathBuf>, content: impl AsRef<[u8]>) {
            self.files
                .lock()
                .unwrap()
                .insert(path.into(), Ok(content.as_ref().to_vec()));
        }

        fn set_error(&self, path: impl Into<PathBuf>, error: std::io::ErrorKind) {
            self.files.lock().unwrap().insert(path.into(), Err(error));
        }
    }

    impl FileReader for MockReader {
        fn read(&self, path: &Path) -> Result<Vec<u8>, std::io::Error> {
            let files = self.files.lock().unwrap();
            match files.get(path) {
                Some(Ok(bytes)) => Ok(bytes.clone()),
                Some(Err(kind)) => Err(std::io::Error::new(*kind, "mock error")),
                None => Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "not found",
                )),
            }
        }
    }

    #[test]
    fn missing_file_is_valid_absence() {
        let reader = MockReader::new();
        let source = InstructionSource::with_reader(PathBuf::from("/workspace/sub"), reader);
        let payload = source.load().expect("missing file is valid absence");
        assert_eq!(payload.source_key, "agents_md");
        assert!(payload.aggregate_text.is_empty());
        assert!(payload.baseline_bytes.is_empty());
        let snapshot: InstructionSnapshot = serde_json::from_str(&payload.snapshot_json).unwrap();
        assert!(snapshot.files.is_empty());
    }

    #[test]
    fn deterministic_ordering_workspace_then_parents() {
        let reader = MockReader::new();
        let ws = PathBuf::from("/repo/crates/app");
        let root = PathBuf::from("/repo");

        reader.set_file(ws.join("AGENTS.md"), "app instructions");
        reader.set_file(root.join("AGENTS.md"), "root instructions");

        let source = InstructionSource::with_reader(&ws, reader);
        let payload = source.load().unwrap();

        let snapshot: InstructionSnapshot = serde_json::from_str(&payload.snapshot_json).unwrap();
        assert_eq!(snapshot.files.len(), 2);
        assert_eq!(
            snapshot.files[0].path,
            ws.join("AGENTS.md").to_string_lossy()
        );
        assert_eq!(
            snapshot.files[1].path,
            root.join("AGENTS.md").to_string_lossy()
        );

        assert!(payload.aggregate_text.contains("app instructions"));
        assert!(payload.aggregate_text.contains("root instructions"));
        let app_pos = payload.aggregate_text.find("app instructions").unwrap();
        let root_pos = payload.aggregate_text.find("root instructions").unwrap();
        assert!(
            app_pos < root_pos,
            "workspace instructions must precede parent instructions"
        );
    }

    #[test]
    fn file_size_limit_bounded_and_utf8() {
        let reader = MockReader::new();
        let ws = PathBuf::from("/workspace");
        // Create 100 bytes of ASCII plus multi-byte character
        let mut content = vec![b'a'; 100];
        // Append 3-byte UTF-8 character: € (0xE2, 0x82, 0xAC)
        content.extend_from_slice("€".as_bytes());

        reader.set_file(ws.join("AGENTS.md"), &content);
        // Set max limit to 101 bytes (which falls in the middle of €)
        let source = InstructionSource::with_reader(&ws, reader).with_max_bytes(101);
        let payload = source.load().unwrap();

        let snapshot: InstructionSnapshot = serde_json::from_str(&payload.snapshot_json).unwrap();
        assert_eq!(
            snapshot.files[0].size, 100,
            "must truncate on utf-8 boundary before partial multi-byte char"
        );
    }

    #[test]
    fn read_failure_is_unavailable() {
        let reader = MockReader::new();
        let ws = PathBuf::from("/workspace");
        reader.set_error(ws.join("AGENTS.md"), std::io::ErrorKind::PermissionDenied);

        let source = InstructionSource::with_reader(&ws, reader);
        let error = source
            .load()
            .expect_err("permission denied must be unavailable");
        assert!(matches!(error, InstructionError::Unavailable(_)));
    }

    #[test]
    fn stable_source_key_snapshot_and_baseline() {
        let reader = MockReader::new();
        let ws = PathBuf::from("/workspace");
        reader.set_file(ws.join("AGENTS.md"), "fixed instructions");

        let source = InstructionSource::with_reader(&ws, reader);
        let p1 = source.load().unwrap();
        let p2 = source.load().unwrap();

        assert_eq!(p1.source_key, p2.source_key);
        assert_eq!(p1.snapshot_json, p2.snapshot_json);
        assert_eq!(p1.baseline_bytes, p2.baseline_bytes);
        assert_eq!(p1.aggregate_text, p2.aggregate_text);
    }
}
