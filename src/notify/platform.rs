use super::Notification;
use std::io::{self, Write};

pub fn terminal_bell() -> Result<(), String> {
    let mut stdout = io::stdout();
    stdout
        .write_all(b"\x07")
        .map_err(|error| error.to_string())?;
    stdout.flush().map_err(|error| error.to_string())
}

pub fn desktop(notification: &Notification) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let script = format!(
            "$ErrorActionPreference='Stop'; [Windows.Data.Xml.Dom.XmlDocument,Windows.Data.Xml.Dom.XmlDocument,ContentType=WindowsRuntime]$xml=New-Object Windows.Data.Xml.Dom.XmlDocument; $xml.LoadXml(\"<toast><visual><binding template='ToastGeneric'><text>{}</text><text>{}</text></binding></visual></toast>\"); $toast=[Windows.UI.Notifications.ToastNotification,Windows,ContentType=WindowsRuntime]::new($xml); [Windows.UI.Notifications.ToastNotificationManager,Windows,ContentType=WindowsRuntime]::CreateToastNotifier('Clawcode').Show($toast)",
            xml_escape(&notification.title),
            xml_escape(&notification.body)
        );
        std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("desktop notification unavailable: {error}"))
    }
    #[cfg(not(target_os = "windows"))]
    {
        #[cfg(target_os = "linux")]
        {
            std::process::Command::new("notify-send")
                .args([&notification.title, &notification.body])
                .spawn()
                .map(|_| ())
                .map_err(|error| format!("desktop notification unavailable: {error}"))
        }
        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("osascript")
                .args([
                    "-e",
                    &format!(
                        "display notification {} with title {}",
                        applescript_quote(&notification.body),
                        applescript_quote(&notification.title)
                    ),
                ])
                .spawn()
                .map(|_| ())
                .map_err(|error| format!("desktop notification unavailable: {error}"))
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = notification;
            Err("desktop notification unavailable on this platform".into())
        }
    }
}

#[cfg(target_os = "windows")]
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(target_os = "macos")]
fn applescript_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

pub fn sound() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "[console]::beep(660,100)",
            ])
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("sound notification unavailable: {error}"))
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("sound notification unavailable on this platform".into())
    }
}
