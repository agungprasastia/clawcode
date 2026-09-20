use clawcode::tui::app::{App, Input, UiEvent};
use clawcode::tui::dialogs::SkillsDialogState;
use clawcode::workspace::skills::{SkillItem, SkillSource, SkillStore};
use std::fs;
use std::path::PathBuf;

fn temp_test_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "clawcode-skills-test-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn type_and_submit(app: &mut App, text: &str) {
    for ch in text.chars() {
        app.apply(UiEvent::Input(Input::Character(ch)));
    }
    app.apply(UiEvent::Input(Input::Submit));
}

#[test]
fn test_skill_store_load_and_frontmatter_parsing() {
    let root = temp_test_dir("frontmatter");

    // 1. Project skill with YAML frontmatter in .clawcode/skills/test-skill/SKILL.md
    let clawcode_skill_dir = root.join(".clawcode").join("skills").join("test-skill");
    fs::create_dir_all(&clawcode_skill_dir).unwrap();
    let frontmatter_content = r#"---
name: custom-test-skill
description: "Frontmatter extracted description"
---
# Test Skill Title

Follow these instructions to test the skills library.
"#;
    fs::write(clawcode_skill_dir.join("SKILL.md"), frontmatter_content).unwrap();

    // 2. Project skill without frontmatter in skills/markdown-skill/SKILL.md
    let markdown_skill_dir = root.join("skills").join("markdown-skill");
    fs::create_dir_all(&markdown_skill_dir).unwrap();
    let title_content = r#"# Markdown Skill

First paragraph extracted as description.
Continued description line.

## Instructions
1. Run step one.
2. Run step two.
"#;
    fs::write(markdown_skill_dir.join("SKILL.md"), title_content).unwrap();

    // 3. OpenCode compat skill in .opencode/skills/compat-skill/SKILL.md
    let opencode_dir = root.join(".opencode").join("skills").join("compat-skill");
    fs::create_dir_all(&opencode_dir).unwrap();
    let opencode_content = r#"---
description: Compatibility skill for opencode
---
# Compat Skill
Compat instructions body.
"#;
    fs::write(opencode_dir.join("SKILL.md"), opencode_content).unwrap();

    // 4. Direct markdown file in skills/direct-file.md
    let direct_path = root.join("skills").join("direct-file.md");
    fs::write(
        &direct_path,
        "# Direct File Skill\nDirect instructions for single-file skill.",
    )
    .unwrap();

    // Load store
    let store = SkillStore::load(&root);
    assert!(store.len() >= 4);
    assert!(!store.is_empty());

    // Verify custom-test-skill
    let skill1 = store.get("custom-test-skill").expect("custom-test-skill");
    assert_eq!(skill1.name, "custom-test-skill");
    assert_eq!(skill1.description, "Frontmatter extracted description");
    assert!(skill1.instructions.contains("Follow these instructions"));
    assert_eq!(skill1.source, SkillSource::Project);

    // Verify markdown-skill
    let skill2 = store.get("markdown-skill").expect("markdown-skill");
    assert_eq!(skill2.name, "markdown-skill");
    assert_eq!(
        skill2.description,
        "First paragraph extracted as description. Continued description line."
    );
    assert!(skill2.instructions.contains("Run step one."));
    assert_eq!(skill2.source, SkillSource::Project);

    // Verify compat-skill
    let skill3 = store.get("compat-skill").expect("compat-skill");
    assert_eq!(skill3.name, "compat-skill");
    assert_eq!(skill3.description, "Compatibility skill for opencode");
    assert_eq!(skill3.source, SkillSource::OpenCode);

    // Verify direct-file
    let skill4 = store.get("direct-file").expect("direct-file");
    assert_eq!(skill4.name, "direct-file");
    assert_eq!(skill4.source, SkillSource::Project);

    // Verify all() ordering
    // Verify all() ordering for discovered project skills
    let all = store.all();
    assert!(all.len() >= 4);
    let names: Vec<&str> = all
        .iter()
        .map(|s| s.name.as_str())
        .filter(|n| {
            matches!(
                *n,
                "compat-skill" | "custom-test-skill" | "direct-file" | "markdown-skill"
            )
        })
        .collect();
    assert_eq!(
        names,
        vec![
            "compat-skill",
            "custom-test-skill",
            "direct-file",
            "markdown-skill"
        ]
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn test_skills_dialog_state_filtering_and_navigation() {
    let items = vec![
        SkillItem {
            name: "rust-refactor".into(),
            description: "Refactor Rust code idiomatically".into(),
            location: std::path::PathBuf::from("/skills/rust/SKILL.md"),
            instructions: "Use standard traits and borrow checker".into(),
            source: SkillSource::Project,
        },
        SkillItem {
            name: "python-optimize".into(),
            description: "Optimize Python code performance".into(),
            location: std::path::PathBuf::from("/skills/python/SKILL.md"),
            instructions: "Vectorize with numpy".into(),
            source: SkillSource::Global,
        },
        SkillItem {
            name: "typescript-types".into(),
            description: "Strict TypeScript type definitions".into(),
            location: std::path::PathBuf::from("/skills/ts/SKILL.md"),
            instructions: "No any allowed".into(),
            source: SkillSource::OpenCode,
        },
    ];

    let mut state = SkillsDialogState::new(items);
    assert_eq!(state.selected, 0);
    assert_eq!(state.filtered_skills().len(), 3);
    assert_eq!(
        state.selected_skill().map(|s| s.name.as_str()),
        Some("rust-refactor")
    );

    // Navigation
    state.next();
    assert_eq!(state.selected, 1);
    assert_eq!(
        state.selected_skill().map(|s| s.name.as_str()),
        Some("python-optimize")
    );

    state.next();
    assert_eq!(state.selected, 2);
    assert_eq!(
        state.selected_skill().map(|s| s.name.as_str()),
        Some("typescript-types")
    );

    // Wrap around
    state.next();
    assert_eq!(state.selected, 0);

    state.previous();
    assert_eq!(state.selected, 2);

    // Filter
    state.push_char('p');
    state.push_char('y');
    assert_eq!(state.filter, "py");
    let filtered = state.filtered_skills();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].name, "python-optimize");
    assert_eq!(
        state.selected_skill().map(|s| s.name.as_str()),
        Some("python-optimize")
    );

    // Backspace
    state.pop_char();
    assert_eq!(state.filter, "p");
    state.pop_char();
    assert_eq!(state.filter, "");
    assert_eq!(state.filtered_skills().len(), 3);

    // Preview scrolling
    assert_eq!(state.preview_scroll, 0);
    state.scroll_preview_down(5);
    assert_eq!(state.preview_scroll, 5);
    state.scroll_preview_up(2);
    assert_eq!(state.preview_scroll, 3);
    state.scroll_preview_up(10);
    assert_eq!(state.preview_scroll, 0);
}

#[test]
fn test_tui_app_skills_dialog_lifecycle() {
    let mut app = App::new();
    assert!(app.skills_dialog().is_none());

    // Open dialog manually
    app.open_skills_dialog();
    assert!(app.skills_dialog().is_some());

    // Close dialog with Esc / Cancel
    app.apply(UiEvent::Input(Input::Cancel));
    assert!(app.skills_dialog().is_none());

    // Reopen and populate with a dummy item for Enter selection test
    app.open_skills_dialog();
    if let Some(dialog) = app.skills_dialog_mut() {
        dialog.skills = vec![SkillItem {
            name: "test-dialog-skill".into(),
            description: "A skill for testing dialog selection".into(),
            location: std::path::PathBuf::from("/test/SKILL.md"),
            instructions: "Do things".into(),
            source: SkillSource::Project,
        }];
        dialog.selected = 0;
    }

    // Submit selection with Enter
    app.apply(UiEvent::Input(Input::Submit));
    assert!(app.skills_dialog().is_none());
    assert_eq!(app.prompt(), "/skill test-dialog-skill");
}

#[test]
fn test_app_input_skills_slash_commands() {
    let root = temp_test_dir("commands");
    let skill_dir = root.join(".clawcode").join("skills").join("calc-skill");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\ndescription: Calculate stuff\n---\n# Calc\nCalculate 1+1=2.",
    )
    .unwrap();

    let orig_dir = std::env::current_dir().unwrap();
    std::env::set_current_dir(&root).unwrap();

    let mut app = App::new();

    // 1. Submit /skills opens dialog
    type_and_submit(&mut app, "/skills");
    assert!(app.skills_dialog().is_some());
    app.close_skills_dialog();

    // 2. Submit /skill unknown sets diagnostic error
    type_and_submit(&mut app, "/skill non-existent-skill");
    assert!(app.diagnostic().contains("skill not found"));

    // 3. Submit /skill calc-skill loads skill instructions
    type_and_submit(&mut app, "/skill calc-skill");
    assert!(app.diagnostic().contains("skill loaded: calc-skill"));

    // 4. Submit /skill calc-skill with extra argument
    type_and_submit(&mut app, "/skill calc-skill please compute");
    assert!(app.diagnostic().contains("skill loaded: calc-skill"));

    std::env::set_current_dir(orig_dir).unwrap();
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn test_agents_skills_discovered_and_parsed() {
    let root = temp_test_dir("agents_discovery");
    let skill_dir = root.join(".agents").join("skills").join("antislop");
    fs::create_dir_all(&skill_dir).unwrap();
    let content = r#"---
name: antislop
description: "Anti-slop design rules"
---
# Antislop

Avoid generic AI slop.
"#;
    fs::write(skill_dir.join("SKILL.md"), content).unwrap();

    // Also test lowercase skill.md
    let lowercase_dir = root.join(".agents").join("skills").join("reviewer");
    fs::create_dir_all(&lowercase_dir).unwrap();
    fs::write(
        lowercase_dir.join("skill.md"),
        "# Reviewer\nReview code carefully.",
    )
    .unwrap();

    // Also test single file skill: <dir>/<skill-name>.md
    let file_skill = root.join(".agents").join("skills").join("helper.md");
    fs::write(file_skill, "# Helper\nHelper instructions.").unwrap();

    let store = SkillStore::load(&root);

    let skill = store.get("antislop").expect("antislop skill");
    assert_eq!(skill.name, "antislop");
    assert_eq!(skill.description, "Anti-slop design rules");
    assert!(skill.instructions.contains("Avoid generic AI slop."));
    assert_eq!(skill.source, SkillSource::Agents);

    let reviewer = store.get("reviewer").expect("reviewer skill");
    assert_eq!(reviewer.name, "reviewer");
    assert_eq!(reviewer.source, SkillSource::Agents);

    let helper = store.get("helper").expect("helper skill");
    assert_eq!(helper.name, "helper");
    assert_eq!(helper.source, SkillSource::Agents);

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn test_project_skills_override_global_skills() {
    let global_dir = temp_test_dir("override_global");
    let global_skill_dir = global_dir.join("shared-skill");
    fs::create_dir_all(&global_skill_dir).unwrap();
    fs::write(
        global_skill_dir.join("SKILL.md"),
        "---\ndescription: Global description\n---\n# Global\nGlobal instructions",
    )
    .unwrap();

    let project_root = temp_test_dir("override_project");
    let project_skill_dir = project_root.join("skills").join("shared-skill");
    fs::create_dir_all(&project_skill_dir).unwrap();
    fs::write(
        project_skill_dir.join("SKILL.md"),
        "---\ndescription: Project description\n---\n# Project\nProject instructions",
    )
    .unwrap();

    let store = SkillStore::load_with_custom_globals(
        &project_root,
        &[(global_dir.clone(), SkillSource::Global)],
    );

    let skill = store.get("shared-skill").expect("shared-skill");
    assert_eq!(skill.description, "Project description");
    assert!(skill.instructions.contains("Project instructions"));
    assert_eq!(skill.source, SkillSource::Project);

    let _ = fs::remove_dir_all(&global_dir);
    let _ = fs::remove_dir_all(&project_root);
}
