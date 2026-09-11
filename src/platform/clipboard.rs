use super::{Clipboard, MAX_CLIPBOARD_BYTES, PlatformError};
use std::process::Command;

pub trait ClipboardBackend: Send + Sync {
    fn read(&self) -> Result<String, PlatformError>;
    fn write(&self, text: &str) -> Result<(), PlatformError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClipboard;

impl SystemClipboard {
    pub fn with_backend(backend: Box<dyn ClipboardBackend>) -> BackendClipboard {
        BackendClipboard { backend }
    }
}

impl Clipboard for SystemClipboard {
    fn read(&self) -> Result<String, PlatformError> {
        SystemBackend.read()
    }

    fn write(&self, text: &str) -> Result<(), PlatformError> {
        if text.len() > MAX_CLIPBOARD_BYTES {
            return Err(PlatformError::InvalidInput(
                "clipboard payload exceeds limit".into(),
            ));
        }
        SystemBackend.write(text)
    }
}

pub struct BackendClipboard {
    backend: Box<dyn ClipboardBackend>,
}

impl Clipboard for BackendClipboard {
    fn read(&self) -> Result<String, PlatformError> {
        self.backend.read()
    }

    fn write(&self, text: &str) -> Result<(), PlatformError> {
        if text.len() > MAX_CLIPBOARD_BYTES {
            return Err(PlatformError::InvalidInput(
                "clipboard payload exceeds limit".into(),
            ));
        }
        self.backend.write(text)
    }
}

struct SystemBackend;

impl ClipboardBackend for SystemBackend {
    fn read(&self) -> Result<String, PlatformError> {
        command_read()
    }

    fn write(&self, text: &str) -> Result<(), PlatformError> {
        command_write(text)
    }
}

#[cfg(windows)]
fn command_read() -> Result<String, PlatformError> {
    command_output(Command::new("powershell").args(["-NoProfile", "-Command", "Get-Clipboard"]))
}

#[cfg(windows)]
fn command_write(text: &str) -> Result<(), PlatformError> {
    let mut command = Command::new("powershell");
    command.args(["-NoProfile", "-Command", "Set-Clipboard"]);
    command.stdin(std::process::Stdio::piped());
    let mut child = command.spawn().map_err(io_error)?;
    use std::io::Write;
    child
        .stdin
        .take()
        .ok_or_else(|| unavailable("clipboard input unavailable"))?
        .write_all(text.as_bytes())
        .map_err(io_error)?;
    child
        .wait()
        .map_err(io_error)?
        .success()
        .then_some(())
        .ok_or_else(|| unavailable("clipboard command failed"))
}

#[cfg(not(windows))]
fn command_read() -> Result<String, PlatformError> {
    command_output(Command::new("xclip").args(["-selection", "clipboard", "-o"]))
}

#[cfg(not(windows))]
fn command_write(text: &str) -> Result<(), PlatformError> {
    let mut command = Command::new("xclip");
    command.args(["-selection", "clipboard", "-i"]);
    command.stdin(std::process::Stdio::piped());
    let mut child = command.spawn().map_err(io_error)?;
    use std::io::Write;
    child
        .stdin
        .take()
        .ok_or_else(|| unavailable("clipboard input unavailable"))?
        .write_all(text.as_bytes())
        .map_err(io_error)?;
    child
        .wait()
        .map_err(io_error)?
        .success()
        .then_some(())
        .ok_or_else(|| unavailable("clipboard command failed"))
}

fn command_output(command: &mut Command) -> Result<String, PlatformError> {
    let output = command.output().map_err(io_error)?;
    if !output.status.success() {
        return Err(unavailable("clipboard command failed"));
    }
    String::from_utf8(output.stdout).map_err(|_| unavailable("clipboard output is not UTF-8"))
}

fn io_error(error: std::io::Error) -> PlatformError {
    PlatformError::Io(error.to_string())
}

fn unavailable(message: &str) -> PlatformError {
    PlatformError::Unavailable(message.into())
}
