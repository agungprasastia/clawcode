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
fn env_references_trim_whitespace_and_reject_empty() {
    const KEY: &str = "CLAWCODE_TEST_KEY_TRIM_7QF";
    unsafe { std::env::set_var(KEY, "  trimmed-secret  \n") };
    let secret = SecretRef::Env(KEY.into());
    assert_eq!(secret.resolve().unwrap(), "trimmed-secret");

    unsafe { std::env::set_var(KEY, "   \t\n  ") };
    assert!(matches!(
        secret.resolve().unwrap_err(),
        ConfigDiagnostic::MissingSecret { .. }
    ));
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

#[test]
fn parses_opencode_style_9router_provider() {
    let source = r#"{
      "model": "9router/minimax/MiniMax-Text-01",
      "provider": {
        "9router": {
          "npm": "@ai-sdk/openai-compatible",
          "name": "9router",
          "apiKey": "",
          "baseURL": "https://9router.com/v1",
          "models": {
            "minimax/MiniMax-Text-01": {
              "name": "MiniMax-Text-01",
              "limit": {
                "context": 1000000,
                "output": 8192
              },
              "modalities": {
                "input": ["text"],
                "output": ["text"]
              }
            }
          }
        }
      },
      "agent": {
        "build": {
          "model": "9router/minimax/MiniMax-Text-01"
        }
      }
    }"#;

    let config = ConfigLoader.parse_str("opencode.jsonc", source).unwrap();
    assert_eq!(
        config.model.as_deref(),
        Some("9router/minimax/MiniMax-Text-01")
    );
    assert_eq!(
        config.resolved_endpoint(Some("9router")),
        Some("https://9router.com/v1")
    );

    let provider = config.providers.get("9router").expect("9router provider");
    assert_eq!(provider.npm.as_deref(), Some("@ai-sdk/openai-compatible"));
    assert_eq!(provider.base_url.as_deref(), Some("https://9router.com/v1"));
    assert_eq!(provider.api_key, None);
    assert_eq!(provider.resolved_api_key(), None);

    let model = provider
        .models
        .get("minimax/MiniMax-Text-01")
        .expect("model config");
    assert_eq!(model.name.as_deref(), Some("MiniMax-Text-01"));
    assert_eq!(model.context_window, Some(1_000_000));
    assert_eq!(model.max_output_tokens, Some(8192));

    let modalities = model.modalities.as_ref().expect("modalities");
    assert_eq!(modalities.input, vec!["text"]);
    assert_eq!(modalities.output, vec!["text"]);

    let model_infos = provider.to_model_infos();
    assert_eq!(model_infos.len(), 1);
    assert_eq!(model_infos[0].id, "minimax/MiniMax-Text-01");
    assert_eq!(model_infos[0].context_window, 1_000_000);

    let agent = config.agents.get("build").expect("build agent");
    assert_eq!(
        agent.model.as_deref(),
        Some("9router/minimax/MiniMax-Text-01")
    );
}

#[test]
fn parses_opencode_options_nested_provider() {
    let source = r#"{
      "mcp": {},
      "plugin": ["test-plugin"],
      "provider": {
        "9router": {
          "npm": "@ai-sdk/openai-compatible",
          "options": {
            "baseURL": "http://127.0.0.1:20128/v1",
            "apiKey": "sk-local-test-key"
          },
          "models": {
            "ag/claude-opus-4-6-thinking": {
              "name": "ag/claude-opus-4-6-thinking"
            }
          }
        }
      }
    }"#;

    let config = ConfigLoader.parse_str("opencode.jsonc", source).unwrap();
    let provider = config.providers.get("9router").expect("9router");
    assert_eq!(
        provider.base_url.as_deref(),
        Some("http://127.0.0.1:20128/v1")
    );
    assert_eq!(
        provider.resolved_api_key().as_deref(),
        Some("sk-local-test-key")
    );
    assert!(provider.models.contains_key("ag/claude-opus-4-6-thinking"));
}

#[test]
fn merges_providers_and_agents_across_configs() {
    let global_src = r#"{
      "schema_version": 1,
      "provider": {
        "p1": { "baseURL": "https://p1.global" }
      },
      "agent": {
        "a1": { "model": "p1/m1" }
      }
    }"#;
    let project_src = r#"{
      "schema_version": 1,
      "provider": {
        "p2": { "baseURL": "https://p2.project" }
      },
      "agent": {
        "a2": { "model": "p2/m2" }
      }
    }"#;

    let global = ConfigLoader.parse_str("global.jsonc", global_src).unwrap();
    let project = ConfigLoader
        .parse_str("project.jsonc", project_src)
        .unwrap();
    let merged = Config::merge(global, project).unwrap();

    assert!(merged.providers.contains_key("p1"));
    assert!(merged.providers.contains_key("p2"));
    assert!(merged.agents.contains_key("a1"));
    assert!(merged.agents.contains_key("a2"));
}

#[test]
fn loads_from_opencode_directory_fallback() {
    let temp = std::env::temp_dir().join(format!("clawcode-test-opencode-{}", std::process::id()));
    let xdg = temp.join(".config");
    let opencode_dir = xdg.join("opencode");
    std::fs::create_dir_all(&opencode_dir).unwrap();

    let opencode_jsonc = r#"{
      "model": "fallback-model",
      "provider": {
        "fallback": {
          "baseURL": "https://fallback.local"
        }
      }
    }"#;
    std::fs::write(opencode_dir.join("opencode.jsonc"), opencode_jsonc).unwrap();

    let project = temp.join("project");
    std::fs::create_dir_all(&project).unwrap();

    let config = ConfigLoader.load_with_paths(&xdg, &project).unwrap();
    assert_eq!(config.model.as_deref(), Some("fallback-model"));
    assert!(config.providers.contains_key("fallback"));

    let _ = std::fs::remove_dir_all(temp);
}

#[test]
fn parses_mcp_local_and_remote_servers() {
    let config = ConfigLoader
        .parse_str(
            "project.jsonc",
            r#"{
              "schema_version": 1,
              "mcp": {
                "local": {
                  "type": "local",
                  "command": ["npx", "-y", "server"],
                  "environment": {"TOKEN": "abc"}
                },
                "remote": {
                  "type": "remote",
                  "url": "https://example.test/mcp",
                  "headers": {"Authorization": "Bearer test"},
                  "enabled": false
                }
              }
            }"#,
        )
        .unwrap();

    let local = config.mcp.get("local").unwrap();
    assert!(local.enabled);
    assert!(!local.is_remote());
    assert_eq!(local.command, ["npx", "-y", "server"]);
    assert_eq!(
        local.environment.get("TOKEN").map(String::as_str),
        Some("abc")
    );

    let remote = config.mcp.get("remote").unwrap();
    assert!(!remote.enabled);
    assert!(remote.is_remote());
    assert_eq!(remote.url.as_deref(), Some("https://example.test/mcp"));
    assert_eq!(
        remote.headers.get("Authorization").map(String::as_str),
        Some("Bearer test")
    );
}

#[test]
fn parses_mcp_servers_shape_and_merges_by_name() {
    let global = ConfigLoader
        .parse_str(
            "global.jsonc",
            r#"{
              "schema_version": 1,
              "mcp": {
                "servers": {
                  "one": {"url": "https://one.test/mcp"},
                  "two": {"command": "two-server", "disabled": true}
                }
              }
            }"#,
        )
        .unwrap();
    let project = ConfigLoader
        .parse_str(
            "project.jsonc",
            r#"{
              "schema_version": 1,
              "mcp": {"two": {"command": ["two", "--verbose"]}}
            }"#,
        )
        .unwrap();
    let merged = Config::merge(global, project).unwrap();

    assert!(merged.mcp.contains_key("one"));
    assert!(merged.mcp.get("two").unwrap().enabled);
    assert_eq!(merged.mcp["two"].command, ["two", "--verbose"]);
}

#[test]
fn rejects_mcp_server_without_transport() {
    let error = ConfigLoader
        .parse_str(
            "project.jsonc",
            r#"{"schema_version": 1, "mcp": {"broken": {}}}"#,
        )
        .unwrap_err();
    let ConfigDiagnostic::Parse { message, .. } = error else {
        panic!("expected Parse");
    };
    assert!(message.contains("needs a command or url"), "{message}");
}

#[test]
fn parses_opencode_and_tui_settings_fields() {
    use clawcode::config::{
        MarkdownMode, ThinkingDisplay, ToolGrouping, TranscriptVerbosity, TurnTokensMode,
    };

    let source = r#"{
      "schema_version": 1,
      "animations": false,
      "mouse": false,
      "session": {
        "thinking": "show",
        "grouping": "none",
        "verbosity": "high",
        "markdown": "source",
        "tps": false,
        "scrollbar": false
      },
      "scroll": {
        "speed": 5,
        "acceleration": true
      },
      "leader": {
        "timeout": 3500
      },
      "debug": {
        "turn_tokens": "verbose"
      },
      "provider": {
        "openai": {
          "models": {
            "gpt-5": {
              "variants": ["low", "medium", "high"]
            }
          }
        }
      }
    }"#;

    let config = ConfigLoader.parse_str("clawcode.jsonc", source).unwrap();
    assert!(!config.tui.animations);
    assert!(!config.tui.mouse);
    assert_eq!(config.tui.thinking, ThinkingDisplay::Show);
    assert_eq!(config.tui.grouping, ToolGrouping::None);
    assert_eq!(config.tui.verbosity, TranscriptVerbosity::High);
    assert_eq!(config.tui.markdown, MarkdownMode::Source);
    assert!(!config.tui.tps);
    assert!(!config.tui.scrollbar);
    assert_eq!(config.tui.scroll_speed, 5);
    assert!(config.tui.scroll_acceleration);
    assert_eq!(config.tui.leader_timeout_ms, 3500);
    assert_eq!(config.tui.turn_tokens, TurnTokensMode::Verbose);

    let model = &config.providers["openai"].models["gpt-5"];
    assert_eq!(model.variants, vec!["low", "medium", "high"]);
}

#[test]
fn save_tui_settings_preserves_existing_fields_comments_and_roundtrips() {
    use clawcode::config::{
        MarkdownMode, ThinkingDisplay, ToolGrouping, TranscriptVerbosity, TuiSettingsConfig,
        TurnTokensMode,
    };

    let temp_dir = std::env::temp_dir().join(format!(
        "clawcode_cfg_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let cfg_path = temp_dir.join("clawcode.jsonc");
    std::fs::write(
        &cfg_path,
        r#"{
  // Keep user comment intact
  "schema_version": 1,
  "model": "openai/o3-mini",
  "api_key": "env:OPENAI_API_KEY",
}
"#,
    )
    .unwrap();

    let updated = TuiSettingsConfig {
        animations: false,
        thinking: ThinkingDisplay::Show,
        grouping: ToolGrouping::None,
        verbosity: TranscriptVerbosity::Low,
        markdown: MarkdownMode::Source,
        tps: false,
        scrollbar: false,
        scroll_speed: 6,
        scroll_acceleration: true,
        mouse: false,
        leader_timeout_ms: 1500,
        turn_tokens: TurnTokensMode::Compact,
    };

    Config::save_tui_settings(&cfg_path, &updated).unwrap();

    let raw = std::fs::read_to_string(&cfg_path).unwrap();
    assert!(raw.contains("// Keep user comment intact"), "{raw}");

    let reloaded = ConfigLoader
        .parse_str(cfg_path.display().to_string(), &raw)
        .unwrap();
    assert_eq!(reloaded.model.as_deref(), Some("openai/o3-mini"));
    assert_eq!(
        reloaded.api_key,
        Some(SecretRef::Env("OPENAI_API_KEY".into()))
    );
    assert_eq!(reloaded.tui, updated);

    // Update a second time to verify in-place replacement of existing `tui` block.
    let mut updated_again = updated;
    updated_again.verbosity = TranscriptVerbosity::High;
    updated_again.animations = true;
    Config::save_tui_settings(&cfg_path, &updated_again).unwrap();

    let raw2 = std::fs::read_to_string(&cfg_path).unwrap();
    assert!(raw2.contains("// Keep user comment intact"), "{raw2}");
    let reloaded2 = ConfigLoader
        .parse_str(cfg_path.display().to_string(), &raw2)
        .unwrap();
    assert_eq!(reloaded2.tui, updated_again);

    let _ = std::fs::remove_dir_all(&temp_dir);
}
