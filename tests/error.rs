use clawcode::core::error::{Diagnostic, ErrorCategory, SourceLocation};

#[test]
fn error_categories_have_stable_machine_names() {
    assert_eq!(ErrorCategory::Config.as_str(), "config");
    assert_eq!(ErrorCategory::Provider.as_str(), "provider");
    assert_eq!(ErrorCategory::Workspace.as_str(), "workspace");
    assert_eq!(ErrorCategory::Persistence.as_str(), "persistence");
    assert_eq!(ErrorCategory::Internal.as_str(), "internal");
}

#[test]
fn diagnostic_preserves_source_location() {
    let location = SourceLocation::new("clawcode.jsonc", 12, 7);
    let diagnostic =
        Diagnostic::new(ErrorCategory::Config, "unknown provider").with_source(location.clone());

    assert_eq!(diagnostic.category(), ErrorCategory::Config);
    assert_eq!(diagnostic.message(), "unknown provider");
    assert_eq!(diagnostic.source(), Some(&location));
    assert_eq!(
        diagnostic.to_string(),
        "config: unknown provider (clawcode.jsonc:12:7)"
    );
}
