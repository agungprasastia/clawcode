use clawcode::config::{Config, ConfigDiagnostic, ConfigLoader, SecretRef};

const GLOBAL: &str = r#"{
  // global settings
  "schema_version": 1,
  "model": "small",
  "endpoint": "https://global",
  "api_key": "env:OPENAI_API_KEY",
}
"#;

const PROJECT: &str = r#"{
  "schema_version": 1,
  /* project override */
  "model": "large",
}
"#;

#[test]
fn parses_jsonc_and_project_overrides_global() {
    let loader = ConfigLoader;
    let global = loader.parse_str("global.jsonc", GLOBAL).unwrap();
    let project = loader.parse_str("project.jsonc", PROJECT).unwrap();

    let merged = Config::merge(global, project).unwrap();

    assert_eq!(merged.model.as_deref(), Some("large"));
    assert_eq!(merged.endpoint.as_deref(), Some("https://global"));
    assert_eq!(
        merged.api_key,
        Some(SecretRef::Env("OPENAI_API_KEY".into()))
    );
}

#[test]
fn malformed_jsonc_reports_exact_line_and_column() {
    let error = ConfigLoader
        .parse_str(
            "project.jsonc",
            "{\n  \"schema_version\": 1,\n  \"model\": ,\n}\n",
        )
        .unwrap_err();

    let ConfigDiagnostic::Parse {
        path, line, column, ..
    } = error
    else {
        panic!("expected Parse, got {error:?}");
    };
    assert_eq!(path, "project.jsonc");
    assert_eq!(line, 3);
    assert_eq!(column, 12);
}

#[test]
fn unknown_fields_report_location_without_crashing() {
    let error = ConfigLoader
        .parse_str(
            "project.jsonc",
            "{\n  \"schema_version\": 1,\n  \"unknown\": true,\n}\n",
        )
        .unwrap_err();

    let ConfigDiagnostic::UnknownField { path, field, line } = error else {
        panic!("expected UnknownField, got {error:?}");
    };
    assert_eq!(path, "project.jsonc");
    assert_eq!(field, "unknown");
    assert_eq!(line, 3);
}

#[test]
fn plaintext_secrets_are_rejected() {
    let error = ConfigLoader
        .parse_str(
            "project.jsonc",
            r#"{ "schema_version": 1, "api_key": "sk-plaintext" }"#,
        )
        .unwrap_err();

    assert!(matches!(error, ConfigDiagnostic::PlaintextSecret { .. }));
}

#[test]
fn credential_store_references_are_accepted() {
    let config = ConfigLoader
        .parse_str(
            "project.jsonc",
            r#"{ "schema_version": 1, "api_key": "credential:openai/prod" }"#,
        )
        .unwrap();

    assert_eq!(
        config.api_key,
        Some(SecretRef::Credential("openai/prod".into()))
    );
}

#[test]
fn env_references_resolve_from_environment() {
    const KEY: &str = "CLAWCODE_TEST_KEY_7QF";
    // SAFETY: test binary runs tests on one thread for this suite.
    unsafe { std::env::set_var(KEY, "secret-value") };
    let config = ConfigLoader
        .parse_str(
            "project.jsonc",
            r#"{ "schema_version": 1, "api_key": "env:CLAWCODE_TEST_KEY_7QF" }"#,
        )
        .unwrap();

    let resolved = config.api_key.as_ref().unwrap().resolve().unwrap();
    assert_eq!(resolved, "secret-value");
}

#[test]
fn missing_env_reference_is_diagnostic() {
    let config = ConfigLoader
        .parse_str(
            "project.jsonc",
            r#"{ "schema_version": 1, "api_key": "env:CLAWCODE_MISSING_7QF" }"#,
        )
        .unwrap();

    let error = config.api_key.as_ref().unwrap().resolve().unwrap_err();
    assert!(matches!(error, ConfigDiagnostic::MissingSecret { .. }));
}

#[test]
fn legacy_schema_migrates_to_current() {
    let config = ConfigLoader
        .parse_str(
            "legacy.jsonc",
            r#"{ "schema_version": 1, "model": "small", "api_key": "env:K" }"#,
        )
        .unwrap();
    assert_eq!(
        config.schema_version,
        clawcode::config::CURRENT_SCHEMA_VERSION
    );
}

#[test]
fn unsupported_future_schema_is_diagnostic() {
    let error = ConfigLoader
        .parse_str("project.jsonc", r#"{ "schema_version": 99 }"#)
        .unwrap_err();

    assert!(matches!(error, ConfigDiagnostic::Parse { .. }));
}

#[test]
fn json5_only_syntax_is_rejected() {
    // JSON5-only: unquoted key is invalid JSONC.
    let error = ConfigLoader
        .parse_str("project.jsonc", "{ schema_version: 1 }")
        .unwrap_err();
    assert!(matches!(error, ConfigDiagnostic::Parse { .. }));

    // JSON5-only: single-quoted string is invalid JSONC.
    let error = ConfigLoader
        .parse_str("project.jsonc", r#"{ "schema_version": 1, 'x': 1 }"#)
        .unwrap_err();
    assert!(matches!(error, ConfigDiagnostic::Parse { .. }));
}

#[test]
fn debug_output_never_contains_secret_values() {
    let config = ConfigLoader
        .parse_str(
            "project.jsonc",
            r#"{ "schema_version": 1, "api_key": "env:CLAWCODE_TEST_KEY_7QF" }"#,
        )
        .unwrap();

    let debug = format!("{config:?}");
    assert!(!debug.contains("secret-value"));
}

#[test]
fn bundled_schema_exists_and_is_local() {
    let schema = include_str!("../config/schema.json");
    assert!(schema.contains("schema_version"));
    assert!(!schema.contains("http://") && !schema.contains("https://"));
}
