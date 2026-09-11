# OpenCode Compatibility Loader Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Load bounded typed OpenCode-style agents, commands, permissions, and themes with fixture-backed diagnostics.

**Architecture:** Add `config::compat` over existing JSONC `Value`; strict typed parsing and bounded collections. Keep loaded data separate from provider `Config`; map permission rules to existing workspace policy only at integration boundary.

**Tech Stack:** Rust 2024, existing JSONC parser, existing `Diagnostic` and workspace policy types.

## Global Constraints

- Existing provider `ConfigLoader` API remains unchanged.
- Compatibility files are data only; no command or agent execution in loader.
- Optional missing directories return empty collections.
- Present malformed/unknown/duplicate data returns actionable diagnostics.
- All names, text, collections, and files remain bounded.
- No `#[allow(...)]`.

---

### Task 1: Typed compatibility schema

**Files:**
- Create: `src/config/compat.rs`
- Modify: `src/config/mod.rs`
- Test: `tests/compat_config.rs`

- [ ] Add tests for typed agents, commands, permissions, themes, JSONC comments, and missing optional artifacts.
- [ ] Implement bounded public structs and strict object-field parsing over `jsonc::Value`.
- [ ] Reject unknown fields, wrong types, duplicate names, and oversized values with `ConfigDiagnostic`.
- [ ] Run focused tests and commit `feat: add typed compatibility config`.

### Task 2: Filesystem loader and safety

**Files:**
- Modify: `src/config/compat.rs`
- Test: `tests/compat_config.rs`
- Create: `tests/fixtures/compat/.opencode/` fixture files as needed.

- [ ] Add root-bounded loader for `.opencode/agents`, `.opencode/commands`, `.opencode/themes`, and `.opencode/permissions.jsonc`.
- [ ] Enforce file-size, collection-count, and path-boundary limits.
- [ ] Test malformed location diagnostics, traversal/absolute path rejection, duplicate files, and missing optional directories.
- [ ] Run gates and commit `feat: load bounded compatibility files`.

### Task 3: Permission policy mapping and checklist

**Files:**
- Modify: `src/config/compat.rs`
- Modify: `src/workspace/policy.rs` only for explicit mapping helper if needed.
- Test: `tests/compat_config.rs`
- Modify: `docs/TODO.md`

- [ ] Add deterministic mapping from compatibility permission rules to existing `PolicyDecision` without a second policy engine.
- [ ] Test allow/ask/deny mapping and rule ordering.
- [ ] Mark `P0-M7-03` complete only after compatibility fixtures and diagnostics pass.
- [ ] Run all gates and commit `feat: complete OpenCode compatibility loader`.
