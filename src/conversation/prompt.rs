use crate::workspace::Mode;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_MAX_RULE_BYTES: usize = 32 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderKind {
    OpenAI,
    Anthropic,
    Gemini,
    Generic,
}

impl ProviderKind {
    pub fn from_model_or_provider(model: &str, provider: &str) -> Self {
        let lower = format!("{model} {provider}").to_lowercase();
        if lower.contains("gemini") {
            ProviderKind::Gemini
        } else if lower.contains("claude") || lower.contains("anthropic") {
            ProviderKind::Anthropic
        } else if lower.contains("gpt-") || lower.contains("o1") || lower.contains("o3") || lower.contains("openai") {
            ProviderKind::OpenAI
        } else {
            ProviderKind::Generic
        }
    }
}

pub struct SystemPromptComposer {
    pub provider_kind: ProviderKind,
    pub working_directory: PathBuf,
    pub is_git_repo: bool,
    pub mode: Mode,
    pub custom_instructions: Option<String>,
}

impl SystemPromptComposer {
    pub fn new(model: &str, provider: &str, working_directory: impl Into<PathBuf>, mode: Mode) -> Self {
        let wd = working_directory.into();
        let is_git = wd.join(".git").exists();
        Self {
            provider_kind: ProviderKind::from_model_or_provider(model, provider),
            working_directory: wd,
            is_git_repo: is_git,
            mode,
            custom_instructions: None,
        }
    }

    pub fn with_custom_instructions(mut self, instructions: String) -> Self {
        self.custom_instructions = Some(instructions);
        self
    }

    pub fn compose(&self) -> String {
        let mut sections = Vec::new();

        // 1. Core provider instructions
        sections.push(self.get_core_prompt());

        // 2. Environment context
        sections.push(self.get_environment_context());

        // 3. Mode instructions (Plan vs Build)
        sections.push(self.get_mode_instructions());

        // 4. Tools usage instructions
        sections.push(self.get_tools_guidance());

        // 5. Local project rules (AGENTS.md, CLAUDE.md)
        if let Some((path, content)) = self.resolve_local_rules() {
            sections.push(format!("# Project Instructions ({})\n{}", path.display(), content));
        }

        // 6. Custom extra instructions if any
        if let Some(ref extra) = self.custom_instructions {
            if !extra.trim().is_empty() {
                sections.push(format!("# Additional Instructions\n{}", extra.trim()));
            }
        }

        sections.join("\n\n---\n\n")
    }

    fn get_core_prompt(&self) -> String {
        match self.provider_kind {
            ProviderKind::OpenAI => self.get_openai_prompt(),
            ProviderKind::Anthropic => self.get_anthropic_prompt(),
            ProviderKind::Gemini => self.get_gemini_prompt(),
            ProviderKind::Generic => self.get_generic_prompt(),
        }
    }

    fn get_openai_prompt(&self) -> String {
        r#"You are an expert autonomous software engineer working directly in the user's codebase.
You iterate persistently until the task is completely finished and verified.

Core Directives:
- Investigate thoroughly before making changes. Search codebase to understand structure and style.
- Plan multi-step tasks clearly before executing.
- Make incremental, surgical changes. Touch only the lines that need modification.
- Test frequently after each change.
- Never output speculative file edits as raw text when actions are requested; invoke tools directly.
- Keep terminal responses short, direct, and concise (< 4 lines typically, excluding tool calls).
- Avoid preambles, fillers, and restatements."#.to_string()
    }

    fn get_anthropic_prompt(&self) -> String {
        r#"You are Claude, an expert software engineering assistant running as an autonomous agent in Clawcode.

Core Directives:
- Plan tasks carefully with clear actionable steps.
- Investigate before executing: search symbols and read file contexts before changing code.
- Prefer dedicated tools (`read_file`, `edit_file`, `write_file`) over raw shell commands.
- Make minimal, surgical edits adhering strictly to local project style.
- Keep terminal responses short and focused (< 4 lines typically, excluding tool calls).
- When referencing specific code locations, use `file_path:line_number` format."#.to_string()
    }

    fn get_gemini_prompt(&self) -> String {
        r#"You are an expert autonomous software engineer. Rigorously adhere to existing project conventions.

Core Directives:
- Understand context via codebase search (`grep_search`, `glob_search`, `read_file`).
- Formulate a grounded plan based on actual file contents.
- Implement surgical changes adhering to conventions.
- Verify with tests or build commands where applicable.
- Adopt a professional, direct, concise tone.
- No conversational filler or preambles. Format output with clean Markdown."#.to_string()
    }

    fn get_generic_prompt(&self) -> String {
        r#"You are an expert autonomous coding assistant.
Analyze codebase structure, execute necessary tools, and produce minimal, high-quality code changes.
Keep responses concise, clear, and direct."#.to_string()
    }

    fn get_environment_context(&self) -> String {
        let git_status = if self.is_git_repo { "yes" } else { "no" };
        let date = format_epoch_date(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        );
        let os = std::env::consts::OS;

        format!(
            r#"<env>
  Working directory: {}
  Is directory a git repo: {}
  Platform: {}
  Today's date: {}
</env>"#,
            self.working_directory.display(),
            git_status,
            os,
            date
        )
    }

    fn get_mode_instructions(&self) -> String {
        match self.mode {
            Mode::Plan => r#"# MODE: PLAN (Read-Only Exploration & Planning)
You are currently operating in PLAN mode.
- You CANNOT modify or write files, and you CANNOT execute destructive commands.
- You have read-only tools: `read_file`, `list_dir`, `glob_search`, `grep_search`, `webfetch`, `websearch`, `skill`, `question`, `update_plan`.
- Investigate the codebase thoroughly: search for symbols, read implementations, trace dependencies.
- Synthesize your findings into a clear, actionable plan.
- If changes to files or destructive shell commands are needed, instruct the user to press Tab to switch to BUILD mode."#.to_string(),
            Mode::Build => r#"# MODE: BUILD (Execution & Implementation)
You are currently operating in BUILD mode.
- You have full permission to make code changes and run commands to complete the task.
- Available tools include: `read_file`, `write_file`, `edit_file`, `list_dir`, `glob_search`, `grep_search`, `bash`, `webfetch`, `websearch`, `skill`, `question`, `update_plan`.
- ALWAYS read files before editing them to understand context and match existing formatting.
- Make minimal, surgical edits. Do not refactor unrelated code.
- CRITICAL: Always use `edit_file` with precise string replacement to update or modify existing files. Use `write_file` ONLY when creating brand-new files or completely replacing a whole file from scratch.
- Verify your changes with tests or build commands when possible.
- Once finished, provide a concise summary of what changed and validation results."#.to_string(),
        }
    }

    fn get_tools_guidance(&self) -> String {
        r#"# Tool Calling Guidelines
- Use the model's native function/tool calling mechanism.
- Never output speculative diffs or hypothetical file edits in raw text when an action is requested — call the appropriate tool directly.
- CRITICAL: Always use `edit_file` with precise string replacement to update or modify existing files. Use `write_file` ONLY when creating brand-new files or completely replacing a whole file from scratch.
- Inspect files thoroughly using `read_file` or search tools before applying edits.
- When referencing code in responses, use `file_path:line_number` format.
- Call `update_plan` before starting multi-step tasks and as milestones are reached to update the visual checklist.
- After receiving tool call results, you MUST ALWAYS provide a comprehensive text response explaining your findings, code changes, or next steps. NEVER end a turn with empty content or silence after executing tools."#.to_string()
    }

    fn resolve_local_rules(&self) -> Option<(PathBuf, String)> {
        let mut cur = self.working_directory.clone();
        loop {
            let candidates = ["AGENTS.md", "CLAUDE.md", ".clawcode/rules.md", ".clawcode/rules"];
            for name in &candidates {
                let p = cur.join(name);
                if p.is_file() {
                    if let Ok(bytes) = fs::read(&p) {
                        let len = bytes.len().min(DEFAULT_MAX_RULE_BYTES);
                        let s = String::from_utf8_lossy(&bytes[..len]).to_string();
                        return Some((p, s));
                    }
                }
            }
            if !cur.pop() {
                break;
            }
        }

        // Global fallback
        let home = std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from);
        if let Some(home) = home {
            let global_candidates = [
                home.join(".config").join("clawcode").join("AGENTS.md"),
                home.join(".claude").join("CLAUDE.md"),
            ];
            for p in &global_candidates {
                if p.is_file() {
                    if let Ok(bytes) = fs::read(p) {
                        let len = bytes.len().min(DEFAULT_MAX_RULE_BYTES);
                        let s = String::from_utf8_lossy(&bytes[..len]).to_string();
                        return Some((p.clone(), s));
                    }
                }
            }
        }

        None
    }
}

fn format_epoch_date(secs: u64) -> String {
    let mut days = secs / 86400;
    let mut year = 1970;
    loop {
        let leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
        let days_in_year = if leap { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }
    let leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
    let month_days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1;
    for &d in &month_days {
        if days < d {
            break;
        }
        days -= d;
        month += 1;
    }
    let day = days + 1;
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_epoch_date() {
        // 2026-09-16 approx 1789516800
        assert_eq!(format_epoch_date(0), "1970-01-01");
        assert_eq!(format_epoch_date(86400 * 365), "1971-01-01");
    }

    #[test]
    fn test_compose_contains_env_and_mode() {
        let composer = SystemPromptComposer::new(
            "ag/gemini-3.8-flash-high",
            "9router",
            std::env::current_dir().unwrap(),
            Mode::Plan,
        );
        let prompt = composer.compose();
        assert!(prompt.contains("<env>"));
        assert!(prompt.contains("MODE: PLAN"));
        assert!(prompt.contains("read_file"));
    }

    #[test]
    fn test_compose_build_mode() {
        let composer = SystemPromptComposer::new(
            "gpt-4o",
            "openai",
            std::env::current_dir().unwrap(),
            Mode::Build,
        );
        let prompt = composer.compose();
        assert!(prompt.contains("MODE: BUILD"));
        assert!(prompt.contains("edit_file"));
    }
}
