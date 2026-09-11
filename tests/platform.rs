use clawcode::platform::{
    Clipboard, CredentialStore, MAX_CLIPBOARD_BYTES, PlatformError, ShellDiscovery,
    UnsupportedCredentialStore,
};
use std::path::PathBuf;

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
fn empty_shell_name_is_invalid() {
    assert!(matches!(
        FakeShell.find(""),
        Err(PlatformError::InvalidInput(_))
    ));
}

#[test]
fn credential_lookup_is_explicitly_unsupported() {
    let store = UnsupportedCredentialStore;
    assert!(matches!(
        store.get("token"),
        Err(PlatformError::Unsupported(_))
    ));
}
