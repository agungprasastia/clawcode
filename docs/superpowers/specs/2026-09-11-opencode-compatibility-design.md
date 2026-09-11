# OpenCode Compatibility Loader Design

## Goal

Load OpenCode-style agents, commands, permissions, and themes from bounded project configuration without coupling them to provider configuration or executing untrusted behavior.

## Architecture

Add `config::compat`, a typed strict loader over the existing JSONC parser. `CompatibilityLoader` accepts a project root and reads only known compatibility files under `.opencode/` plus the supported project config file. It returns a `CompatibilityConfig` containing bounded agents, commands, permission rules, and themes.

Each artifact has a typed schema, duplicate names are rejected, unknown fields produce file/line diagnostics, and malformed files report path/line/column. Loaded commands and agents are data only; execution remains owned by existing conversation/tool services. Permission rules map to existing workspace policy decisions and do not introduce a second policy engine.

## Safety and limits

- No path traversal or absolute paths for compatibility file references.
- Bounded file size, collection count, names, descriptions, prompts, and theme values.
- No plaintext secrets or provider credentials in compatibility artifacts.
- Missing optional directories produce empty collections.
- Invalid present files fail with actionable diagnostics.

## Supported artifacts

- `agents/*.jsonc`: name, description, prompt, optional model.
- `commands/*.jsonc`: name, description, prompt.
- `permissions.jsonc`: ordered rules with operation and decision (`allow`, `ask`, `deny`).
- `themes/*.jsonc`: name and bounded color/style tokens.

## Testing

Fixtures cover valid loading, JSONC comments/trailing commas, malformed syntax with location, unknown fields, duplicate names, missing optional directories, path boundary rejection, and permission mapping. Existing provider config behavior remains unchanged.
