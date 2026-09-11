use clawcode::notify::{Notification, NotificationKind, NotificationReport, Notifier};

struct FakeNotifier {
    fail: bool,
}

impl Notifier for FakeNotifier {
    fn notify(&self, _notification: &Notification) -> NotificationReport {
        if self.fail {
            NotificationReport::failure("fake failure")
        } else {
            NotificationReport::success()
        }
    }
}

#[test]
fn notification_payload_is_bounded() {
    let notification = Notification::new(NotificationKind::Success, "x".repeat(4097), "body");
    assert!(notification.is_err());
}

#[test]
fn notifier_failure_is_reported_without_panicking() {
    let notification = Notification::new(NotificationKind::Error, "title", "body").unwrap();
    let report = FakeNotifier { fail: true }.notify(&notification);
    assert_eq!(report.failures, vec!["fake failure"]);
}

#[test]
fn successful_notifier_reports_success() {
    let notification = Notification::new(NotificationKind::Success, "title", "body").unwrap();
    assert!(
        FakeNotifier { fail: false }
            .notify(&notification)
            .is_success()
    );
}
