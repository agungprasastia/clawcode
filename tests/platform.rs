use clawcode::platform::{
    Clipboard, CredentialStore, MAX_CLIPBOARD_BYTES, PlatformError, ShellDiscovery,
    UnsupportedCredentialStore, get_branch_for_path, get_current_branch, is_git_repo,
};
use clawcode::platform::{ClipboardBackend, PathShellDiscovery, SystemClipboard};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct RecordingBackend {
    writes: Arc<Mutex<Vec<String>>>,
}

impl ClipboardBackend for RecordingBackend {
    fn read(&self) -> Result<String, PlatformError> {
        Ok("backend value".into())
    }

    fn write(&self, text: &str) -> Result<(), PlatformError> {
        self.writes.lock().unwrap().push(text.into());
        Ok(())
    }
}

struct FakeClipboard {
    value: String,
    fail_read: bool,
    written: std::cell::RefCell<Vec<String>>,
}

impl Clipboard for FakeClipboard {
    fn read(&self) -> Result<String, PlatformError> {
        if self.fail_read {
            return Err(PlatformError::Unavailable("clipboard unavailable".into()));
        }
        Ok(self.value.clone())
    }

    fn write(&self, text: &str) -> Result<(), PlatformError> {
        if text.len() > MAX_CLIPBOARD_BYTES {
            return Err(PlatformError::InvalidInput(
                "clipboard payload exceeds limit".into(),
            ));
        }
        self.written.borrow_mut().push(text.to_owned());
        Ok(())
    }
}

struct FakeShell;

impl ShellDiscovery for FakeShell {
    fn find(&self, name: &str) -> Result<PathBuf, PlatformError> {
        if name.is_empty() {
            return Err(PlatformError::InvalidInput(
                "shell name cannot be empty".into(),
            ));
        }
        Ok(PathBuf::from(name))
    }
}

#[test]
fn fake_clipboard_reads_and_writes_successfully() {
    let clipboard = FakeClipboard {
        value: "hello".into(),
        fail_read: false,
        written: std::cell::RefCell::new(Vec::new()),
    };

    assert_eq!(clipboard.read().unwrap(), "hello");
    clipboard.write("world").unwrap();
    assert_eq!(clipboard.written.borrow().as_slice(), ["world"]);
}

#[test]
fn fake_clipboard_reports_read_failure_and_rejects_oversized_input() {
    let clipboard = FakeClipboard {
        value: String::new(),
        fail_read: true,
        written: std::cell::RefCell::new(Vec::new()),
    };

    assert!(matches!(
        clipboard.read(),
        Err(PlatformError::Unavailable(_))
    ));
    assert!(matches!(
        clipboard.write(&"x".repeat(MAX_CLIPBOARD_BYTES + 1)),
        Err(PlatformError::InvalidInput(_))
    ));
    assert!(clipboard.written.borrow().is_empty());
}

#[test]
fn system_clipboard_rejects_oversized_write_before_backend_invocation() {
    let backend = RecordingBackend::default();
    let writes = backend.writes.clone();
    let clipboard = SystemClipboard::with_backend(Box::new(backend));

    assert!(matches!(
        clipboard.write(&"x".repeat(MAX_CLIPBOARD_BYTES + 1)),
        Err(PlatformError::InvalidInput(_))
    ));
    assert!(writes.lock().unwrap().is_empty());
}

#[test]
fn system_clipboard_delegates_valid_operations() {
    let backend = RecordingBackend::default();
    let writes = backend.writes.clone();
    let clipboard = SystemClipboard::with_backend(Box::new(backend));

    assert_eq!(clipboard.read().unwrap(), "backend value");
    clipboard.write("safe").unwrap();
    assert_eq!(writes.lock().unwrap().as_slice(), ["safe"]);
}

#[test]
fn empty_shell_name_is_invalid() {
    assert!(matches!(
        FakeShell.find(""),
        Err(PlatformError::InvalidInput(_))
    ));
}

#[test]
fn credential_lookup_is_explicitly_unsupported() {
    let store = UnsupportedCredentialStore;
    let error = store.get("token").unwrap_err();
    assert!(matches!(error, PlatformError::Unsupported(_)));
    assert!(!error.to_string().contains("token"));
}

#[test]
fn shell_discovery_finds_regular_file_without_executing_it() {
    let directory = temp_directory("find");
    let marker = directory.join("marker");
    fs::write(&marker, b"marker content").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&marker, fs::Permissions::from_mode(0o755)).unwrap();
    }

    let discovery = PathShellDiscovery::new([directory.clone()]);
    assert_eq!(discovery.find("marker").unwrap(), marker);

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn shell_discovery_reports_missing_name() {
    let directory = temp_directory("missing");
    let discovery = PathShellDiscovery::new([directory.clone()]);

    assert!(matches!(
        discovery.find("missing"),
        Err(PlatformError::NotFound(_))
    ));

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn shell_discovery_rejects_empty_and_path_separator_names() {
    let discovery = PathShellDiscovery::new(std::iter::empty::<PathBuf>());

    assert!(matches!(
        discovery.find(""),
        Err(PlatformError::InvalidInput(_))
    ));
    assert!(matches!(
        discovery.find("nested/name"),
        Err(PlatformError::InvalidInput(_))
    ));
}

#[test]
fn git_branch_and_repo_detection_in_workspace() {
    assert!(is_git_repo(std::path::Path::new(".")));
    let branch = get_current_branch();
    assert!(branch.is_some());
    let branch = branch.unwrap();
    assert!(!branch.is_empty());
    assert_ne!(branch, "HEAD");
}

#[test]
fn git_branch_for_non_git_directory_returns_none() {
    let directory = temp_directory("non-git");
    assert!(!is_git_repo(&directory));
    assert_eq!(get_branch_for_path(&directory), None);
    fs::remove_dir_all(directory).unwrap();
}

fn temp_directory(label: &str) -> PathBuf {
    let directory =
        std::env::temp_dir().join(format!("clawcode-platform-{label}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).unwrap();
    directory
}
