use clawcode::config::compat::{CompatibilityLoader, Permission, PermissionRule};
use clawcode::workspace::{Mode, PolicyDecision};
use std::fs;

#[test]
fn parses_jsonc_agent_and_command() {
    let loader = CompatibilityLoader;
    let agent = loader
        .parse_agent(
            "agent.jsonc",
            r#"{
                // comments and trailing comma are supported
                "name": "reviewer",
                "description": "Reviews changes",
                "prompt": "Inspect diff",
                "model": "local/test",
            }"#,
        )
        .expect("agent parses");
    assert_eq!(agent.name, "reviewer");
    assert_eq!(agent.model.as_deref(), Some("local/test"));

    let command = loader
        .parse_command(
            "command.jsonc",
            r#"{"name":"review","description":"Review","prompt":"Check diff"}"#,
        )
        .expect("command parses");
    assert_eq!(command.prompt, "Check diff");
}

#[test]
fn parses_permissions_and_theme() {
    let loader = CompatibilityLoader;
    let permissions = loader
        .parse_permissions(
            "permissions.jsonc",
            r#"[
                {"operation":"read","decision":"allow"},
                {"operation":"build","decision":"ask"},
                {"operation":"shell","decision":"deny"},
            ]"#,
        )
        .expect("permissions parse");
    assert_eq!(permissions[0].decision, Permission::Allow);
    assert_eq!(permissions[1].decision, Permission::Ask);

    let theme = loader
        .parse_theme(
            "theme.jsonc",
            r##"{"name":"dark","tokens":{"background":"#111","text":"#eee"}}"##,
        )
        .expect("theme parses");
    assert_eq!(theme.tokens.len(), 2);
}

#[test]
fn rejects_unknown_fields_and_bad_permissions() {
    let loader = CompatibilityLoader;
    let error = loader
        .parse_command(
            "command.jsonc",
            r#"{"name":"x","description":"x","prompt":"x","extra":true}"#,
        )
        .expect_err("unknown field rejected");
    assert!(error.to_string().contains("unknown field `extra`"));

    let error = loader
        .parse_permissions(
            "permissions.jsonc",
            r#"[{"operation":"build","decision":"maybe"}]"#,
        )
        .expect_err("bad decision rejected");
    assert!(
        error
            .to_string()
            .contains("decision must be allow, ask, or deny")
    );
}

#[test]
fn rejects_oversized_text() {
    let loader = CompatibilityLoader;
    let description = "x".repeat(16 * 1024 + 1);
    let source = format!(r#"{{"name":"x","description":"{description}","prompt":"x"}}"#);
    let error = loader
        .parse_command("command.jsonc", &source)
        .expect_err("bound enforced");
    assert!(error.to_string().contains("text exceeds configured limit"));
}

#[test]
fn maps_permissions_through_existing_policy() {
    let allow = PermissionRule {
        operation: "write".into(),
        decision: Permission::Allow,
    };
    assert_eq!(
        allow.policy_decision(Mode::Build),
        Some(PolicyDecision::Allowed)
    );

    let deny = PermissionRule {
        operation: "write".into(),
        decision: Permission::Deny,
    };
    assert_eq!(
        deny.policy_decision(Mode::Build),
        Some(PolicyDecision::Denied)
    );

    let unknown = PermissionRule {
        operation: "unknown".into(),
        decision: Permission::Allow,
    };
    assert_eq!(unknown.policy_decision(Mode::Build), None);
}

#[test]
fn project_overrides_global_compatibility_fixture() {
    let base = std::env::temp_dir().join(format!("clawcode-compat-{}", std::process::id()));
    let global = base.join("global");
    let project = base.join("project");
    fs::create_dir_all(global.join(".opencode/agents")).expect("global fixture directory");
    fs::create_dir_all(project.join(".opencode/agents")).expect("project fixture directory");
    fs::write(
        global.join(".opencode/agents/reviewer.jsonc"),
        r#"{"name":"reviewer","description":"global","prompt":"global"}"#,
    )
    .expect("global fixture");
    fs::write(
        project.join(".opencode/agents/reviewer.jsonc"),
        r#"{"name":"reviewer","description":"project","prompt":"project"}"#,
    )
    .expect("project fixture");

    let loaded = CompatibilityLoader
        .load_merged(&global, &project)
        .expect("merged fixture");
    assert_eq!(loaded.agents[0].description, "project");
    let _ = fs::remove_dir_all(base);
}
