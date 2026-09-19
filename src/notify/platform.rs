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
        let mut cmd = std::process::Command::new("powershell");
        cmd.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "$ErrorActionPreference='Stop'; [Windows.Data.Xml.Dom.XmlDocument,Windows.Data.Xml.Dom.XmlDocument,ContentType=WindowsRuntime]$xml=New-Object Windows.Data.Xml.Dom.XmlDocument; $xml.LoadXml(\"<toast><visual><binding template='ToastGeneric'><text>$env:CLAWCODE_TOAST_TITLE</text><text>$env:CLAWCODE_TOAST_BODY</text></binding></visual></toast>\"); $toast=[Windows.UI.Notifications.ToastNotification,Windows,ContentType=WindowsRuntime]::new($xml); [Windows.UI.Notifications.ToastNotificationManager,Windows,ContentType=WindowsRuntime]::CreateToastNotifier('Clawcode').Show($toast)",
        ]);
        cmd.env("CLAWCODE_TOAST_TITLE", xml_escape(&notification.title));
        cmd.env("CLAWCODE_TOAST_BODY", xml_escape(&notification.body));
        cmd.spawn()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "windows")]
    #[test]
    fn xml_escape_escapes_xml_special_characters() {
        assert_eq!(
            xml_escape("test <tag> & 'quote' \"double\""),
            "test &lt;tag&gt; &amp; &apos;quote&apos; &quot;double&quot;"
        );
    }
}
