mod platform;

const MAX_TITLE: usize = 256;
const MAX_BODY: usize = 4096;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotificationKind {
    Success,
    Error,
    Cancelled,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Notification {
    pub kind: NotificationKind,
    pub title: String,
    pub body: String,
}

impl Notification {
    pub fn new(
        kind: NotificationKind,
        title: impl Into<String>,
        body: impl Into<String>,
    ) -> Result<Self, &'static str> {
        let title = title.into();
        let body = body.into();
        if title.len() > MAX_TITLE || body.len() > MAX_BODY {
            return Err("notification payload exceeds configured limit");
        }
        Ok(Self { kind, title, body })
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NotificationReport {
    pub failures: Vec<String>,
}

impl NotificationReport {
    pub const fn success() -> Self {
        Self {
            failures: Vec::new(),
        }
    }
    pub fn failure(message: impl Into<String>) -> Self {
        Self {
            failures: vec![message.into()],
        }
    }
    pub const fn is_success(&self) -> bool {
        self.failures.is_empty()
    }
}

pub trait Notifier {
    fn notify(&self, notification: &Notification) -> NotificationReport;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NoopNotifier;

impl Notifier for NoopNotifier {
    fn notify(&self, _notification: &Notification) -> NotificationReport {
        NotificationReport::success()
    }
}

pub struct BestEffortNotifier;

impl Notifier for BestEffortNotifier {
    fn notify(&self, notification: &Notification) -> NotificationReport {
        let mut report = NotificationReport::success();
        if let Err(error) = platform::terminal_bell() {
            report.failures.push(error);
        }
        if let Err(error) = platform::desktop(notification) {
            report.failures.push(error);
        }
        if let Err(error) = platform::sound() {
            report.failures.push(error);
        }
        report
    }
}
