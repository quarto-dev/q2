# `cargo xtask create-worktree` — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `cargo xtask create-worktree` so every new worktree comes with `.beads/redirect` and a marker-delimited CLAUDE.local.md context section — driven from one of three modes (beads ID / GitHub issue / upgrade-date), idempotent, filesystem-only.

**Architecture:** All code lives in one new module `crates/xtask/src/create_worktree.rs`. The module exposes a `clap::Args` struct (`ArgGroup`-validated tri-state mode) and `run(args)` orchestrator. `run()` fans out to three pure helpers (`derive_slug`, `build_section`, `update_claude_local_md`) and three subprocess wrappers (`fetch_beads_metadata`, `fetch_gh_issue`, `git_worktree_add`). The xtask never mutates beads state — skills retain that responsibility.

**Tech Stack:** Rust, `clap` (workspace), `anyhow` (workspace), `time = "0.3"` (new direct dep, features `macros` + `formatting`), `serde_json` via existing workspace deps for parsing `br --json` / `gh --json`.

**Design reference:** Full design rationale lives in `claude-notes/plans/2026-05-07-create-worktree-xtask.md`. This document is the execution sequence — it does not re-derive design decisions.

---

## File map

| Path | Action |
|---|---|
| `crates/xtask/src/create_worktree.rs` | **Create** — full implementation + `#[cfg(test)] mod tests` |
| `crates/xtask/src/main.rs` | **Modify** — add `mod`, `Command` variant, match arm, doc comment |
| `crates/xtask/Cargo.toml` | **Modify** — add `time` direct dependency, add `serde_json` |
| `.gitignore` | **Modify** — append `CLAUDE.local.md` line |
| `.claude/rules/xtask.md` | **Modify** — add `create-worktree` row to commands table |
| `.claude/rules/worktrees.md` | **Modify** — replace § Fresh worktree bootstrap, add § CLAUDE.local.md, add § Manual bootstrap |
| `.claude/skills/investigate-beads/SKILL.md` | **Modify** — replace inline git commands with `cargo xtask create-worktree <id>` |
| `.claude/skills/triage/SKILL.md` | **Modify** — replace inline git commands with `cargo xtask create-worktree --issue <N>`, note step ordering |
| `.claude/skills/upgrade-cargo-deps/SKILL.md` | **Modify** — replace inline git commands with `cargo xtask create-worktree --upgrade` |

---

## Phase A — Scaffolding

### Task A1: Add direct dependencies to xtask Cargo.toml

**Files:**
- Modify: `crates/xtask/Cargo.toml:13-20`

- [ ] **Step 1: Edit dependencies block**

Add the two new direct deps. After this edit the `[dependencies]` block reads:

```toml
[dependencies]
anyhow = { workspace = true }
clap = { workspace = true }
proc-macro2 = { workspace = true }
serde_json = { workspace = true }
syn = { workspace = true }
tempfile = "3"
time = { version = "0.3", features = ["macros", "formatting"] }
walkdir = { workspace = true }
```

Both crates are already in the workspace `Cargo.lock` transitively (verify with `cargo tree -p xtask` after the edit), so adding them as direct deps incurs no extra compile cost.

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p xtask`
Expected: clean build, no warnings.

- [ ] **Step 3: Commit**

```bash
git add crates/xtask/Cargo.toml
git commit -m "xtask: add time + serde_json direct deps for create-worktree"
```

---

### Task A2: Create empty create_worktree module + stub Args + run()

**Files:**
- Create: `crates/xtask/src/create_worktree.rs`

- [ ] **Step 1: Write module stub**

Create `crates/xtask/src/create_worktree.rs` with this content:

```rust
//! `cargo xtask create-worktree` — set up a git worktree with beads redirect
//! and a marker-delimited CLAUDE.local.md context section.
//!
//! Three modes (exactly one required):
//!   - positional `<bd-id>` — beads issue (reads `br show`)
//!   - `--issue <N>` — GitHub issue triage (reads `gh issue view`)
//!   - `--upgrade` — cargo dependency upgrade (date-based branch)
//!
//! Filesystem-only: never touches beads state. Skills own beads lifecycle.

use anyhow::Result;

const BEGIN_MARKER: &str = "<!-- BEGIN WORKTREE CONTEXT — managed by cargo xtask create-worktree -->";
const END_MARKER: &str = "<!-- END WORKTREE CONTEXT -->";

const STOP_WORDS: &[&str] = &[
    "a", "an", "the", "and", "or", "in", "on", "of", "to",
    "for", "with", "from", "at", "by", "is", "as",
];

// Lock the em-dash in BEGIN_MARKER against accidental editor substitution.
const _: () = {
    let bytes = BEGIN_MARKER.as_bytes();
    // U+2014 EM DASH encodes as 0xE2 0x80 0x94 in UTF-8.
    let mut i = 0;
    let mut found = false;
    while i + 2 < bytes.len() {
        if bytes[i] == 0xE2 && bytes[i + 1] == 0x80 && bytes[i + 2] == 0x94 {
            found = true;
        }
        i += 1;
    }
    assert!(found, "BEGIN_MARKER must contain U+2014 em dash");
};

#[derive(clap::Args)]
#[command(group(clap::ArgGroup::new("mode").required(true).multiple(false)))]
pub struct Args {
    /// Beads issue ID, e.g. `bd-1d3e`. Reads `br show <id>` for title and external_ref.
    #[arg(group = "mode")]
    pub beads_id: Option<String>,

    /// GitHub issue number, e.g. `157`. Reads `gh issue view`.
    #[arg(long, group = "mode")]
    pub issue: Option<u32>,

    /// Cargo dependency upgrade — uses today's date for branch name.
    #[arg(long, group = "mode")]
    pub upgrade: bool,

    /// Override auto-derived slug. In beads mode replaces the derived slug;
    /// in issue/upgrade modes appended as a suffix (for parallel-worktree workflows).
    #[arg(long)]
    pub slug: Option<String>,

    /// Base branch.
    #[arg(long, default_value = "main")]
    pub base: String,
}

pub fn run(_args: Args) -> Result<()> {
    anyhow::bail!("create-worktree not yet implemented");
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p xtask`
Expected: clean build (warnings about unused fields are OK at this stage — they will be consumed in later tasks).

- [ ] **Step 3: Commit**

```bash
git add crates/xtask/src/create_worktree.rs
git commit -m "xtask: scaffold create_worktree module with Args + stub run()"
```

---

### Task A3: Wire CreateWorktree into main.rs

**Files:**
- Modify: `crates/xtask/src/main.rs:8-22` (doc comment + module decls)
- Modify: `crates/xtask/src/main.rs:36-166` (Command enum)
- Modify: `crates/xtask/src/main.rs:168-232` (match arms)

- [ ] **Step 1: Update top-level doc comment**

In the file header (lines 8-14), add a `create-worktree` bullet between `lint` and `test`:

```rust
//! Available commands:
//! - `dev-setup`: Install required development tools (cargo-nextest, wasm-bindgen-cli)
//! - `lint`: Run custom lint checks on the codebase
//! - `create-worktree`: Create git worktree with beads redirect and CLAUDE.local.md
//! - `test`: Run workspace tests with platform-appropriate crate exclusions
//! - `verify`: Run full project verification (build + tests for Rust and hub-client)
//! - `build-all`: Fresh-clone build orchestration (npm install + hub-client + Rust workspace)
//! - `build-trace-viewer`: Build just the trace-viewer SPA
```

- [ ] **Step 2: Add module declaration**

Insert `mod create_worktree;` into the alphabetically-sorted mod list at lines 16-22. After the edit the block reads:

```rust
mod build_all;
mod build_trace_viewer;
mod create_worktree;
mod dev_setup;
mod lint;
mod test;
mod treesitter_crlf;
mod verify;
```

- [ ] **Step 3: Add Command variant**

Inside the `enum Command { ... }` block (currently 36-166), insert a new variant after `Lint { ... }` (before `Test { ... }`):

```rust
    /// Create a new git worktree with beads redirect and CLAUDE.local.md context stub.
    ///
    /// Modes (exactly one required):
    ///   <bd-id>      — beads issue (positional)
    ///   --issue N    — GitHub issue triage
    ///   --upgrade    — cargo dependency upgrade (date-based branch)
    CreateWorktree {
        #[command(flatten)]
        args: create_worktree::Args,
    },
```

- [ ] **Step 4: Add match arm**

Inside `main()` (lines 168-232), insert a match arm in the same position (after `Command::Lint { .. }`, before `Command::Test { .. }`):

```rust
        Command::CreateWorktree { args } => create_worktree::run(args),
```

- [ ] **Step 5: Verify clap parses each mode correctly**

Run each of the following and confirm clap produces the expected behavior:

```bash
cargo run -q -p xtask -- create-worktree --help
# Expected: help text lists positional [BEADS_ID], --issue <ISSUE>, --upgrade, --slug, --base.

cargo run -q -p xtask -- create-worktree
# Expected: error mentioning "the following required arguments" / one of mode.

cargo run -q -p xtask -- create-worktree bd-1d3e --issue 1
# Expected: error from ArgGroup: "argument cannot be used with one or more of the other specified arguments".

cargo run -q -p xtask -- create-worktree bd-1d3e
# Expected: bail message "create-worktree not yet implemented".
```

If `--upgrade` (a bool flag) does not participate in `ArgGroup` validation in the installed clap version, fall back to defining `upgrade` as `Option<bool>` with `action = clap::ArgAction::SetTrue` and revisit; see the design doc § Command interface for rationale.

- [ ] **Step 6: Commit**

```bash
git add crates/xtask/src/main.rs
git commit -m "xtask: wire create-worktree subcommand into Command enum"
```

---

## Phase B — Pure helpers (TDD)

Each task in this phase adds one pure function plus its test cases, following red-green-commit. All tests live in the `#[cfg(test)] mod tests { ... }` block at the bottom of `create_worktree.rs`.

### Task B1: `derive_slug` + `validate_slug`

`derive_slug` produces an auto-slug from titles (already ASCII-only by filter); `validate_slug` enforces a grammar contract on `--slug` overrides (rejects `/`, `..`, whitespace, etc.) so the override can't produce invalid branch names or path-escape.

**Files:**
- Modify: `crates/xtask/src/create_worktree.rs` (add fns + tests)

- [ ] **Step 1: Write failing tests**

Append to `crates/xtask/src/create_worktree.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_drops_stop_words_and_kebab_splits() {
        let s = derive_slug("Fix CRLF test failures in quarto-doctemplate on Windows").unwrap();
        assert_eq!(s, "fix-crlf-test-failures");
    }

    #[test]
    fn slug_caps_at_four_tokens() {
        let s = derive_slug("alpha beta gamma delta epsilon zeta").unwrap();
        assert_eq!(s, "alpha-beta-gamma-delta");
    }

    #[test]
    fn slug_strips_punctuation_and_unicode() {
        let s = derive_slug("Don't panic — handle naïve input (v2)!").unwrap();
        // apostrophe / em dash / accent / parens / digits-with-letters all collapse
        assert_eq!(s, "dont-panic-handle-nave");
    }

    #[test]
    fn slug_empty_result_errors() {
        let err = derive_slug("the and of on in").unwrap_err().to_string();
        assert!(err.contains("unable to derive slug"));
        assert!(err.contains("--slug"));
    }

    #[test]
    fn slug_only_punctuation_errors() {
        let err = derive_slug("!!! ??? ---").unwrap_err().to_string();
        assert!(err.contains("unable to derive slug"));
    }

    #[test]
    fn validate_slug_accepts_safe_input() {
        assert!(validate_slug("e2e-beads").is_ok());
        assert!(validate_slug("issue42").is_ok());
        assert!(validate_slug("a_b-c").is_ok());
    }

    #[test]
    fn validate_slug_rejects_empty() {
        let err = validate_slug("").unwrap_err().to_string();
        assert!(err.contains("must not be empty"));
    }

    #[test]
    fn validate_slug_rejects_path_separators_and_traversal() {
        assert!(validate_slug("foo/bar").is_err());
        assert!(validate_slug("foo\\bar").is_err());
        assert!(validate_slug("..").is_err());
        assert!(validate_slug(".").is_err());
    }

    #[test]
    fn validate_slug_rejects_whitespace_and_other_punct() {
        assert!(validate_slug("foo bar").is_err());
        assert!(validate_slug("foo.bar").is_err());
        assert!(validate_slug("foo:bar").is_err());
    }

    #[test]
    fn validate_slug_rejects_leading_or_trailing_dash() {
        assert!(validate_slug("-leading").is_err());
        assert!(validate_slug("trailing-").is_err());
    }

    #[test]
    fn validate_slug_rejects_too_long() {
        let too_long = "a".repeat(65);
        assert!(validate_slug(&too_long).is_err());
    }
}
```

- [ ] **Step 2: Run tests — expect compile failure**

Run: `cargo nextest run -p xtask create_worktree::tests::slug_ create_worktree::tests::validate_slug_`
Expected: compile error — `derive_slug` / `validate_slug` not defined.

- [ ] **Step 3: Implement `derive_slug` and `validate_slug`**

Insert into `create_worktree.rs` between the constants block and the `Args` struct:

```rust
pub fn derive_slug(title: &str) -> Result<String> {
    let tokens: Vec<String> = title
        .to_lowercase()
        .split(|c: char| c.is_whitespace() || c == '-')
        .map(|tok| {
            tok.chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .collect::<String>()
        })
        .filter(|tok| !tok.is_empty())
        .filter(|tok| !STOP_WORDS.contains(&tok.as_str()))
        .take(4)
        .collect();

    if tokens.is_empty() {
        anyhow::bail!(
            "unable to derive slug from title \"{title}\" — pass --slug <name> to override"
        );
    }
    Ok(tokens.join("-"))
}

/// Validate a user-provided `--slug` override. Auto-derived slugs already
/// satisfy these rules by construction; this only applies to overrides.
pub fn validate_slug(slug: &str) -> Result<()> {
    if slug.is_empty() {
        anyhow::bail!("--slug must not be empty");
    }
    if slug.len() > 64 {
        anyhow::bail!("--slug too long ({} chars, max 64): {slug:?}", slug.len());
    }
    if slug == "." || slug == ".." {
        anyhow::bail!("--slug must not be {slug:?}");
    }
    if slug.starts_with('-') || slug.ends_with('-') {
        anyhow::bail!("--slug must not start or end with '-': {slug:?}");
    }
    if let Some(bad) = slug
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || *c == '-' || *c == '_'))
    {
        anyhow::bail!(
            "--slug contains invalid character {bad:?} — only ASCII alphanumeric, '-', '_' allowed: {slug:?}"
        );
    }
    Ok(())
}
```

- [ ] **Step 4: Run tests — expect green**

Run: `cargo nextest run -p xtask create_worktree::tests::slug_ create_worktree::tests::validate_slug_`
Expected: 5 slug_ + 6 validate_slug_ = 11 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/xtask/src/create_worktree.rs
git commit -m "xtask(create-worktree): derive_slug + validate_slug grammar"
```

---

### Task B2: `parse_external_ref_to_github_url`

**Files:**
- Modify: `crates/xtask/src/create_worktree.rs`

- [ ] **Step 1: Write failing tests**

Append inside `mod tests`:

```rust
    #[test]
    fn external_ref_gh_prefix_to_url() {
        assert_eq!(
            parse_external_ref_to_github_url(Some("gh-157")),
            Some("https://github.com/quarto-dev/q2/issues/157".to_string())
        );
    }

    #[test]
    fn external_ref_none_returns_none() {
        assert_eq!(parse_external_ref_to_github_url(None), None);
    }

    #[test]
    fn external_ref_empty_string_returns_none() {
        assert_eq!(parse_external_ref_to_github_url(Some("")), None);
    }

    #[test]
    fn external_ref_non_gh_prefix_returns_none() {
        assert_eq!(parse_external_ref_to_github_url(Some("linear-ABC-12")), None);
    }

    #[test]
    fn external_ref_malformed_gh_returns_none() {
        // Non-numeric suffix
        assert_eq!(parse_external_ref_to_github_url(Some("gh-foo")), None);
        // Empty suffix
        assert_eq!(parse_external_ref_to_github_url(Some("gh-")), None);
    }
```

- [ ] **Step 2: Run tests — expect compile failure**

Run: `cargo nextest run -p xtask create_worktree::tests::external_ref_`
Expected: compile error — function not defined.

- [ ] **Step 3: Implement**

Add to `create_worktree.rs`, near `derive_slug`:

```rust
pub fn parse_external_ref_to_github_url(ext: Option<&str>) -> Option<String> {
    let ext = ext?;
    let n = ext.strip_prefix("gh-")?;
    if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) {
        Some(format!("https://github.com/quarto-dev/q2/issues/{n}"))
    } else {
        None
    }
}
```

- [ ] **Step 4: Run tests — expect green**

Run: `cargo nextest run -p xtask create_worktree::tests::external_ref_`
Expected: 5 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/xtask/src/create_worktree.rs
git commit -m "xtask(create-worktree): parse gh-N external_ref to GitHub URL"
```

---

### Task B3: `detect_line_ending`

**Files:**
- Modify: `crates/xtask/src/create_worktree.rs`

- [ ] **Step 1: Write failing tests**

Append inside `mod tests`:

```rust
    #[test]
    fn detect_le_empty_defaults_to_lf() {
        assert_eq!(detect_line_ending(""), "\n");
    }

    #[test]
    fn detect_le_no_newlines_defaults_to_lf() {
        assert_eq!(detect_line_ending("hello world"), "\n");
    }

    #[test]
    fn detect_le_lf_only() {
        assert_eq!(detect_line_ending("a\nb\nc\n"), "\n");
    }

    #[test]
    fn detect_le_crlf_pure() {
        assert_eq!(detect_line_ending("a\r\nb\r\nc\r\n"), "\r\n");
    }

    #[test]
    fn detect_le_mixed_falls_back_to_lf() {
        // CRLF + bare LF -> LF (do not propagate inconsistency)
        assert_eq!(detect_line_ending("a\r\nb\nc\r\n"), "\n");
    }

    #[test]
    fn detect_le_sniffs_only_first_1kb() {
        // Pad the head with LF, place a CRLF beyond the sniff window
        let mut s = "x\n".repeat(600); // 1200 bytes of LF-terminated lines
        s.push_str("z\r\n");
        assert_eq!(detect_line_ending(&s), "\n");
    }
```

- [ ] **Step 2: Run tests — expect compile failure**

Run: `cargo nextest run -p xtask create_worktree::tests::detect_le_`
Expected: compile error — function not defined.

- [ ] **Step 3: Implement**

Add to `create_worktree.rs`:

```rust
pub fn detect_line_ending(content: &str) -> &'static str {
    // Sniff up to first 1 KiB, snapped to a char boundary so slicing is valid.
    let mut sniff_end = content.len().min(1024);
    while sniff_end > 0 && !content.is_char_boundary(sniff_end) {
        sniff_end -= 1;
    }
    let sniff = &content[..sniff_end];

    let crlf_count = sniff.matches("\r\n").count();
    let lf_total = sniff.matches('\n').count();
    let bare_lf = lf_total - crlf_count;

    if crlf_count > 0 && bare_lf == 0 {
        "\r\n"
    } else {
        "\n"
    }
}
```

- [ ] **Step 4: Run tests — expect green**

Run: `cargo nextest run -p xtask create_worktree::tests::detect_le_`
Expected: 6 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/xtask/src/create_worktree.rs
git commit -m "xtask(create-worktree): detect line ending with 1 KiB sniff"
```

---

### Task B4: `build_section` (template generation)

**Files:**
- Modify: `crates/xtask/src/create_worktree.rs`

- [ ] **Step 1: Define the SectionKind enum and helper**

Add to `create_worktree.rs`, near the top of the implementation block:

```rust
pub enum SectionKind {
    Beads {
        id: String,
        title: String,
        github_url: Option<String>,
    },
    Issue {
        number: u32,
        title: String,
        url: String,
    },
    Upgrade {
        date: String,
    },
}
```

- [ ] **Step 2: Write failing tests**

Append inside `mod tests`:

```rust
    #[test]
    fn section_beads_with_github() {
        let s = build_section(&SectionKind::Beads {
            id: "bd-1d3e".into(),
            title: "Fix X".into(),
            github_url: Some("https://github.com/quarto-dev/q2/issues/42".into()),
        });
        assert!(s.starts_with(BEGIN_MARKER));
        assert!(s.trim_end().ends_with(END_MARKER));
        assert!(s.contains("**Beads:** bd-1d3e — Fix X"));
        assert!(s.contains("**GitHub:** https://github.com/quarto-dev/q2/issues/42"));
        assert!(s.contains("Run `br show bd-1d3e`"));
        assert!(s.contains("Main repo: `../..`"));
    }

    #[test]
    fn section_beads_without_github_omits_line() {
        let s = build_section(&SectionKind::Beads {
            id: "bd-zzzz".into(),
            title: "T".into(),
            github_url: None,
        });
        assert!(!s.contains("**GitHub:**"));
        assert!(s.contains("**Beads:** bd-zzzz — T"));
    }

    #[test]
    fn section_issue() {
        let s = build_section(&SectionKind::Issue {
            number: 157,
            title: "An issue".into(),
            url: "https://github.com/quarto-dev/q2/issues/157".into(),
        });
        assert!(s.contains("**GitHub issue:** #157 — An issue"));
        assert!(s.contains("**URL:** https://github.com/quarto-dev/q2/issues/157"));
        assert!(s.contains("**Beads:** (run `br search 157`"));
        assert!(!s.contains("**Beads:** bd-")); // no resolved beads id
    }

    #[test]
    fn section_upgrade() {
        let s = build_section(&SectionKind::Upgrade {
            date: "2026-05-11".into(),
        });
        assert!(s.contains("**Task:** Cargo dependency upgrade — 2026-05-11"));
        assert!(!s.contains("**Beads:**"));
        assert!(!s.contains("**GitHub:**"));
    }

    #[test]
    fn section_strips_marker_from_title() {
        // A title that literally contains the END marker must not be interpolated
        // verbatim — `strip_managed_section` would otherwise pick it up as the
        // section terminator on the next run.
        let evil = format!("real title {END_MARKER} oops");
        let s = build_section(&SectionKind::Beads {
            id: "bd-x".into(),
            title: evil,
            github_url: None,
        });
        // END_MARKER must appear exactly once — at the section's actual close.
        assert_eq!(s.matches(END_MARKER).count(), 1);
        // BEGIN_MARKER ditto.
        assert_eq!(s.matches(BEGIN_MARKER).count(), 1);
    }
```

- [ ] **Step 3: Run tests — expect compile failure**

Run: `cargo nextest run -p xtask create_worktree::tests::section_`
Expected: compile error — `build_section` not defined.

- [ ] **Step 4: Implement `build_section` (with marker-safe title interpolation)**

Add to `create_worktree.rs`:

```rust
/// Neutralize any occurrences of BEGIN/END marker substrings inside
/// externally-sourced text (titles from `br`/`gh`). Without this, a title
/// containing `<!-- END WORKTREE CONTEXT -->` would terminate the section
/// prematurely on the next idempotent strip pass.
fn marker_safe(s: &str) -> String {
    s.replace(BEGIN_MARKER, "[BEGIN marker scrubbed]")
        .replace(END_MARKER, "[END marker scrubbed]")
}

pub fn build_section(kind: &SectionKind) -> String {
    let body = match kind {
        SectionKind::Beads {
            id,
            title,
            github_url,
        } => {
            let title = marker_safe(title);
            let mut s = String::new();
            s.push_str("# Worktree Context\n\n");
            s.push_str("This is a **worktree** of the q2 repository. Main repo: `../..`\n\n");
            s.push_str(&format!("**Beads:** {id} — {title}\n"));
            if let Some(url) = github_url {
                s.push_str(&format!("**GitHub:** {url}\n"));
            }
            s.push_str("**Plan:** <!-- fill in after creating: claude-notes/plans/YYYY-MM-DD-name.md -->\n");
            s.push('\n');
            s.push_str(&format!("Run `br show {id}` for current status and notes.\n"));
            s
        }
        SectionKind::Issue { number, title, url } => {
            let title = marker_safe(title);
            let mut s = String::new();
            s.push_str("# Worktree Context\n\n");
            s.push_str("This is a **worktree** of the q2 repository. Main repo: `../..`\n\n");
            s.push_str(&format!("**GitHub issue:** #{number} — {title}\n"));
            s.push_str(&format!("**URL:** {url}\n"));
            s.push_str(&format!(
                "**Beads:** (run `br search {number}` to find or create a beads issue)\n"
            ));
            s.push_str("**Plan:** <!-- fill in after creating: claude-notes/plans/YYYY-MM-DD-name.md -->\n");
            s
        }
        SectionKind::Upgrade { date } => {
            let mut s = String::new();
            s.push_str("# Worktree Context\n\n");
            s.push_str("This is a **worktree** of the q2 repository. Main repo: `../..`\n\n");
            s.push_str(&format!("**Task:** Cargo dependency upgrade — {date}\n"));
            s.push_str("**Plan:** <!-- fill in if needed -->\n");
            s
        }
    };

    format!("{BEGIN_MARKER}\n{body}{END_MARKER}\n")
}
```

- [ ] **Step 5: Run tests — expect green**

Run: `cargo nextest run -p xtask create_worktree::tests::section_`
Expected: 5 passed.

- [ ] **Step 6: Commit**

```bash
git add crates/xtask/src/create_worktree.rs
git commit -m "xtask(create-worktree): build_section templates for 3 modes"
```

---

### Task B5: `strip_managed_section` (the surgical CLAUDE.local.md slice)

**Files:**
- Modify: `crates/xtask/src/create_worktree.rs`

- [ ] **Step 1: Write failing tests**

Append inside `mod tests`:

```rust
    #[test]
    fn strip_no_marker_returns_input_unchanged() {
        let input = "# My notes\nfoo bar\n";
        assert_eq!(strip_managed_section(input).unwrap(), input);
    }

    #[test]
    fn strip_full_managed_section() {
        let input = format!(
            "{BEGIN_MARKER}\n# Worktree Context\nstuff\n{END_MARKER}\n# My notes\nfoo\n"
        );
        assert_eq!(strip_managed_section(&input).unwrap(), "# My notes\nfoo\n");
    }

    #[test]
    fn strip_section_in_middle_of_file() {
        let input = format!(
            "# Header\n\n{BEGIN_MARKER}\nbody\n{END_MARKER}\n\n# Footer\n"
        );
        assert_eq!(
            strip_managed_section(&input).unwrap(),
            "# Header\n\n\n# Footer\n"
        );
    }

    #[test]
    fn strip_begin_without_end_errors() {
        let input = format!("{BEGIN_MARKER}\nbody never closed\n");
        let err = strip_managed_section(&input).unwrap_err().to_string();
        assert!(err.contains("BEGIN marker without END marker"));
    }

    #[test]
    fn strip_uses_first_of_multiple_begins() {
        let input = format!(
            "{BEGIN_MARKER}\nfirst\n{END_MARKER}\nmiddle\n{BEGIN_MARKER}\nsecond\n{END_MARKER}\n"
        );
        // First section + trailing newline stripped; everything from "middle" onward preserved.
        let out = strip_managed_section(&input).unwrap();
        assert!(out.starts_with("middle\n"));
        assert!(out.contains(BEGIN_MARKER)); // second still present
    }

    #[test]
    fn strip_handles_crlf_marker_lines() {
        let input = format!("{BEGIN_MARKER}\r\nbody\r\n{END_MARKER}\r\nrest\r\n");
        assert_eq!(strip_managed_section(&input).unwrap(), "rest\r\n");
    }
```

- [ ] **Step 2: Run tests — expect compile failure**

Run: `cargo nextest run -p xtask create_worktree::tests::strip_`
Expected: compile error — `strip_managed_section` not defined.

- [ ] **Step 3: Implement**

Add to `create_worktree.rs`:

```rust
pub fn strip_managed_section(content: &str) -> Result<String> {
    let Some(begin_pos) = content.find(BEGIN_MARKER) else {
        return Ok(content.to_string());
    };

    // Warn (but proceed) if a second BEGIN appears between the first BEGIN and EOF.
    let after_begin = &content[begin_pos + BEGIN_MARKER.len()..];
    if after_begin.contains(BEGIN_MARKER) {
        eprintln!(
            "warning: CLAUDE.local.md contains multiple BEGIN markers — using the first; \
             recommend manual review of {}",
            "CLAUDE.local.md"
        );
    }

    let end_search_start = begin_pos + BEGIN_MARKER.len();
    let end_rel = content[end_search_start..]
        .find(END_MARKER)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "CLAUDE.local.md has BEGIN marker without END marker — refusing to modify; \
                 resolve manually"
            )
        })?;
    let end_marker_end = end_search_start + end_rel + END_MARKER.len();

    // Strip from the start of the BEGIN line through one trailing newline after END.
    let begin_line_start = content[..begin_pos]
        .rfind('\n')
        .map(|i| i + 1)
        .unwrap_or(0);

    let mut after_end = end_marker_end;
    let rest = &content[after_end..];
    if let Some(stripped) = rest.strip_prefix("\r\n") {
        after_end += rest.len() - stripped.len();
    } else if let Some(stripped) = rest.strip_prefix('\n') {
        after_end += rest.len() - stripped.len();
    }

    let mut out = String::with_capacity(content.len());
    out.push_str(&content[..begin_line_start]);
    out.push_str(&content[after_end..]);
    Ok(out)
}
```

- [ ] **Step 4: Run tests — expect green**

Run: `cargo nextest run -p xtask create_worktree::tests::strip_`
Expected: 6 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/xtask/src/create_worktree.rs
git commit -m "xtask(create-worktree): strip managed section by markers (idempotent)"
```

---

### Task B6: `update_claude_local_md` (full file rewrite, atomic)

**Files:**
- Modify: `crates/xtask/src/create_worktree.rs`

- [ ] **Step 1: Write failing tests**

Append inside `mod tests` (these touch the filesystem via `tempfile`):

```rust
    use std::fs;
    use tempfile::TempDir;

    fn make_dummy_section() -> String {
        build_section(&SectionKind::Beads {
            id: "bd-xxxx".into(),
            title: "Demo".into(),
            github_url: None,
        })
    }

    #[test]
    fn update_creates_file_when_missing() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("CLAUDE.local.md");
        update_claude_local_md(&p, &make_dummy_section()).unwrap();
        let out = fs::read_to_string(&p).unwrap();
        assert!(out.starts_with(BEGIN_MARKER));
        assert!(out.trim_end().ends_with(END_MARKER));
        assert!(out.ends_with('\n'));
    }

    #[test]
    fn update_prepends_when_no_marker_present() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("CLAUDE.local.md");
        fs::write(&p, "# My notes\nfoo\n").unwrap();
        update_claude_local_md(&p, &make_dummy_section()).unwrap();
        let out = fs::read_to_string(&p).unwrap();
        assert!(out.starts_with(BEGIN_MARKER));
        assert!(out.contains("# My notes"));
    }

    #[test]
    fn update_is_idempotent() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("CLAUDE.local.md");
        update_claude_local_md(&p, &make_dummy_section()).unwrap();
        update_claude_local_md(&p, &make_dummy_section()).unwrap();
        let out = fs::read_to_string(&p).unwrap();
        assert_eq!(out.matches(BEGIN_MARKER).count(), 1);
        assert_eq!(out.matches(END_MARKER).count(), 1);
    }

    #[test]
    fn update_preserves_user_content_below_section() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("CLAUDE.local.md");
        update_claude_local_md(&p, &make_dummy_section()).unwrap();
        // User edits below the managed section.
        let mut content = fs::read_to_string(&p).unwrap();
        content.push_str("\n# My notes\nfoo bar\n");
        fs::write(&p, &content).unwrap();
        // Re-run — managed section refreshed, user content stays.
        update_claude_local_md(&p, &make_dummy_section()).unwrap();
        let out = fs::read_to_string(&p).unwrap();
        assert!(out.contains("# My notes"));
        assert!(out.contains("foo bar"));
        assert_eq!(out.matches(BEGIN_MARKER).count(), 1);
    }

    #[test]
    fn update_preserves_crlf_when_existing_is_crlf() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("CLAUDE.local.md");
        fs::write(&p, "# Header\r\n\r\nnotes\r\n").unwrap();
        update_claude_local_md(&p, &make_dummy_section()).unwrap();
        let out = fs::read(&p).unwrap();
        // Output should contain CRLF; no bare LFs.
        let lf_total = out.iter().filter(|&&b| b == b'\n').count();
        let crlf_pairs = out.windows(2).filter(|w| w == b"\r\n").count();
        assert_eq!(lf_total, crlf_pairs, "bare LFs found in CRLF output: {:?}", out);
    }

    #[test]
    fn update_errors_when_path_is_directory() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("CLAUDE.local.md");
        fs::create_dir(&p).unwrap();
        let err = update_claude_local_md(&p, &make_dummy_section())
            .unwrap_err()
            .to_string();
        assert!(err.contains("not a regular file"));
    }

    #[test]
    fn update_errors_on_begin_without_end() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("CLAUDE.local.md");
        fs::write(&p, format!("{BEGIN_MARKER}\nbroken\n")).unwrap();
        let err = update_claude_local_md(&p, &make_dummy_section())
            .unwrap_err()
            .to_string();
        assert!(err.contains("BEGIN marker without END marker"));
    }
```

- [ ] **Step 2: Run tests — expect compile failure**

Run: `cargo nextest run -p xtask create_worktree::tests::update_`
Expected: compile error — `update_claude_local_md` not defined.

- [ ] **Step 3: Implement**

Add to `create_worktree.rs`:

```rust
use std::fs;
use std::path::{Path, PathBuf};

pub fn update_claude_local_md(path: &Path, new_section: &str) -> Result<()> {
    // 1. Read existing content (or empty if file missing).
    let existing = if path.exists() {
        let meta = path.symlink_metadata().with_context(|| {
            format!("reading metadata of {}", path.display())
        })?;
        if !meta.is_file() {
            anyhow::bail!(
                "CLAUDE.local.md exists but is not a regular file: {}",
                path.display()
            );
        }
        fs::read_to_string(path)
            .with_context(|| format!("reading {}", path.display()))?
    } else {
        String::new()
    };

    // 2. Detect line ending from existing content.
    let nl = detect_line_ending(&existing);

    // 3. Strip any existing managed section.
    let body = strip_managed_section(&existing)?;

    // 4. Normalize new_section to detected line ending.
    let new_section_nl = if nl == "\r\n" {
        new_section.replace('\n', "\r\n")
    } else {
        new_section.to_string()
    };

    // 5. Compose: new section + blank line + remaining body (if any).
    let mut out = new_section_nl;
    if !body.is_empty() {
        if !out.ends_with(nl) {
            out.push_str(nl);
        }
        out.push_str(nl); // blank line
        out.push_str(&body);
    }
    if !out.ends_with(nl) {
        out.push_str(nl);
    }

    // 6. Atomic write: tmp + rename.
    // Build the temp path by appending ".tmp" to the *full* OsStr — avoids the
    // `Path::with_extension("md.tmp")` ambiguity around dots in extensions.
    let mut tmp_os = path.as_os_str().to_owned();
    tmp_os.push(".tmp");
    let tmp = PathBuf::from(tmp_os);
    fs::write(&tmp, out.as_bytes())
        .with_context(|| format!("writing {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| {
        format!("renaming {} to {}", tmp.display(), path.display())
    })?;

    Ok(())
}
```

Note the `use anyhow::Context;` import will be needed at the top of the module — add it next to the existing `use anyhow::Result;`.

- [ ] **Step 4: Run tests — expect green**

Run: `cargo nextest run -p xtask create_worktree::tests::update_`
Expected: 7 passed.

- [ ] **Step 5: Verify cross-module unit tests still pass**

Run: `cargo nextest run -p xtask`
Expected: all xtask tests pass; no regressions in other modules.

- [ ] **Step 6: Commit**

```bash
git add crates/xtask/src/create_worktree.rs
git commit -m "xtask(create-worktree): update_claude_local_md with atomic rename"
```

---

## Phase C — Subprocess wrappers

These functions call out to `br`, `gh`, and `git` and cannot be unit-tested cleanly. Each is verified at compile time and exercised end-to-end in Phase E.

### Task C1: `fetch_beads_metadata`

**Files:**
- Modify: `crates/xtask/src/create_worktree.rs`

- [ ] **Step 1: Define the result type**

Add to `create_worktree.rs`:

```rust
pub struct BeadsMetadata {
    pub title: String,
    pub external_ref: Option<String>,
}
```

- [ ] **Step 2: Implement `fetch_beads_metadata`**

Add to `create_worktree.rs`:

```rust
use std::process::Command;

pub fn fetch_beads_metadata(id: &str) -> Result<BeadsMetadata> {
    let output = Command::new("br")
        .args(["show", id, "--json"])
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                anyhow::anyhow!(
                    "br is required — install via `cargo install beads-rust` or see project README"
                )
            } else {
                anyhow::Error::new(e).context("spawning `br show`")
            }
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("br show {id} failed:\n{stderr}");
    }

    let stdout = std::str::from_utf8(&output.stdout)
        .with_context(|| format!("`br show {id} --json` produced non-UTF-8 output"))?;

    // `br show --json` returns an array; take the first element.
    let arr: Vec<serde_json::Value> = serde_json::from_str(stdout)
        .with_context(|| format!("parsing JSON from `br show {id} --json`"))?;
    let first = arr
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("`br show {id} --json` returned an empty array"))?;

    let title = first
        .get("title")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("`br show {id}` JSON missing `title` field"))?
        .to_string();

    let external_ref = first
        .get("external_ref")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    Ok(BeadsMetadata {
        title,
        external_ref,
    })
}
```

- [ ] **Step 3: Verify compile**

Run: `cargo check -p xtask`
Expected: clean.

(No standalone smoke test here — `run()` is still the stub, so any CLI invocation
bails before reaching `fetch_beads_metadata`. End-to-end coverage lives in Phase E
after `run()` is wired in Task D1.)

- [ ] **Step 4: Commit**

```bash
git add crates/xtask/src/create_worktree.rs
git commit -m "xtask(create-worktree): fetch_beads_metadata via br show --json"
```

---

### Task C2: `fetch_gh_issue`

**Files:**
- Modify: `crates/xtask/src/create_worktree.rs`

- [ ] **Step 1: Define type + implement**

Add to `create_worktree.rs`:

```rust
pub struct GhIssue {
    pub title: String,
    pub url: String,
}

pub fn fetch_gh_issue(number: u32) -> Result<GhIssue> {
    let n = number.to_string();
    let output = Command::new("gh")
        .args([
            "issue",
            "view",
            &n,
            "--repo",
            "quarto-dev/q2",
            "--json",
            "title,url",
        ])
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                anyhow::anyhow!("gh is required — see https://cli.github.com/")
            } else {
                anyhow::Error::new(e).context("spawning `gh issue view`")
            }
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh issue view {n} failed:\n{stderr}");
    }

    let stdout = std::str::from_utf8(&output.stdout)
        .with_context(|| format!("`gh issue view {n}` produced non-UTF-8 output"))?;

    let v: serde_json::Value = serde_json::from_str(stdout)
        .with_context(|| format!("parsing JSON from `gh issue view {n}`"))?;
    let title = v
        .get("title")
        .and_then(|x| x.as_str())
        .ok_or_else(|| anyhow::anyhow!("`gh issue view {n}` JSON missing `title`"))?
        .to_string();
    let url = v
        .get("url")
        .and_then(|x| x.as_str())
        .ok_or_else(|| anyhow::anyhow!("`gh issue view {n}` JSON missing `url`"))?
        .to_string();

    Ok(GhIssue { title, url })
}
```

- [ ] **Step 2: Verify compile**

Run: `cargo check -p xtask`
Expected: clean.

- [ ] **Step 3: Commit**

```bash
git add crates/xtask/src/create_worktree.rs
git commit -m "xtask(create-worktree): fetch_gh_issue via gh issue view --json"
```

---

### Task C3: Filesystem ops — `git_worktree_add` + `write_beads_redirect`

**Files:**
- Modify: `crates/xtask/src/create_worktree.rs`

- [ ] **Step 1: Implement filesystem ops**

Add to `create_worktree.rs`:

```rust
pub fn git_worktree_add(branch: &str, dir: &Path, base: &str) -> Result<()> {
    if dir.exists() {
        anyhow::bail!("worktree directory already exists: {}", dir.display());
    }

    // Pre-check: does the branch already exist locally?
    let check = Command::new("git")
        .args(["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")])
        .output()
        .context("spawning `git rev-parse`")?;
    if check.status.success() {
        anyhow::bail!(
            "branch already exists: {branch} — remove it or pass --slug to disambiguate"
        );
    }

    // Pass the directory as OsStr so paths with non-UTF-8 bytes (Windows UTF-16
    // halves, weird POSIX names) still round-trip correctly.
    // `.output()` captures stderr so we can include git's actual error message
    // in our anyhow context — `.status()` would just give us an exit code.
    let output = Command::new("git")
        .arg("worktree")
        .arg("add")
        .arg("-b")
        .arg(branch)
        .arg(dir.as_os_str())
        .arg(base)
        .output()
        .context("spawning `git worktree add`")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "git worktree add failed (exit {:?}):\n{stderr}",
            output.status.code()
        );
    }

    Ok(())
}

pub fn write_beads_redirect(dir: &Path) -> Result<()> {
    let redirect = dir.join(".beads").join("redirect");
    // `.beads/` is tracked in the new worktree — directory should exist.
    if !redirect.parent().map(Path::is_dir).unwrap_or(false) {
        anyhow::bail!(
            ".beads/ directory missing in new worktree: {} — was the base branch correct?",
            redirect.parent().unwrap().display()
        );
    }
    // LF line ending intentionally, even on Windows.
    fs::write(&redirect, "../../../.beads\n")
        .with_context(|| format!("writing {}", redirect.display()))?;
    Ok(())
}
```

- [ ] **Step 2: Verify compile**

Run: `cargo check -p xtask`
Expected: clean.

- [ ] **Step 3: Commit**

```bash
git add crates/xtask/src/create_worktree.rs
git commit -m "xtask(create-worktree): git_worktree_add + write_beads_redirect"
```

---

## Phase D — Orchestration

### Task D1: Wire `run()` to dispatch by mode

**Files:**
- Modify: `crates/xtask/src/create_worktree.rs`

- [ ] **Step 1: Replace the stub `run()`**

Replace the existing stub body with a full implementation:

```rust
pub fn run(args: Args) -> Result<()> {
    // Mode is enforced by clap::ArgGroup(required, single).
    let plan = if let Some(id) = args.beads_id.as_deref() {
        plan_beads(id, args.slug.as_deref(), &args.base)?
    } else if let Some(n) = args.issue {
        plan_issue(n, args.slug.as_deref(), &args.base)?
    } else if args.upgrade {
        plan_upgrade(args.slug.as_deref(), &args.base)?
    } else {
        unreachable!("clap ArgGroup guarantees one mode is set");
    };

    git_worktree_add(&plan.branch, &plan.dir, &plan.base)?;

    // From here on, on any error we roll back the worktree+branch we just
    // created so a retry is not blocked by directory/branch collision.
    let post = (|| -> Result<()> {
        write_beads_redirect(&plan.dir)?;
        let section = build_section(&plan.kind);
        let claude_local = plan.dir.join("CLAUDE.local.md");
        update_claude_local_md(&claude_local, &section)?;
        Ok(())
    })();

    if let Err(e) = post {
        eprintln!("error after worktree creation: {e:#}");
        eprintln!("rolling back worktree {} and branch {}", plan.dir.display(), plan.branch);
        let _ = Command::new("git")
            .arg("worktree")
            .arg("remove")
            .arg("--force")
            .arg(plan.dir.as_os_str())
            .status();
        let _ = Command::new("git")
            .args(["branch", "-D", &plan.branch])
            .status();
        return Err(e);
    }

    print_summary(&plan);
    Ok(())
}

struct Plan {
    branch: String,
    dir: PathBuf,
    base: String,
    kind: SectionKind,
}

fn plan_beads(id: &str, slug_override: Option<&str>, base: &str) -> Result<Plan> {
    let meta = fetch_beads_metadata(id)?;
    let slug = match slug_override {
        Some(s) => {
            validate_slug(s)?;
            s.to_string()
        }
        None => derive_slug(&meta.title)?,
    };
    let leaf = format!("{id}-{slug}");
    let github_url = parse_external_ref_to_github_url(meta.external_ref.as_deref());
    if github_url.is_none() {
        if let Some(other) = meta
            .external_ref
            .as_deref()
            .filter(|s| !s.is_empty() && !s.starts_with("gh-"))
        {
            eprintln!(
                "note: external_ref {other:?} is not a `gh-` reference; omitting GitHub line"
            );
        }
    }
    Ok(Plan {
        branch: format!("beads/{leaf}"),
        dir: PathBuf::from(".worktrees").join(&leaf),
        base: base.to_string(),
        kind: SectionKind::Beads {
            id: id.to_string(),
            title: meta.title,
            github_url,
        },
    })
}

fn plan_issue(number: u32, slug_suffix: Option<&str>, base: &str) -> Result<Plan> {
    if let Some(s) = slug_suffix {
        validate_slug(s)?;
    }
    let gh = fetch_gh_issue(number)?;
    let leaf = match slug_suffix {
        Some(s) => format!("issue-{number}-{s}"),
        None => format!("issue-{number}"),
    };
    Ok(Plan {
        branch: leaf.clone(),
        dir: PathBuf::from(".worktrees").join(&leaf),
        base: base.to_string(),
        kind: SectionKind::Issue {
            number,
            title: gh.title,
            url: gh.url,
        },
    })
}

fn plan_upgrade(slug_suffix: Option<&str>, base: &str) -> Result<Plan> {
    if let Some(s) = slug_suffix {
        validate_slug(s)?;
    }
    let date = time::OffsetDateTime::now_utc()
        .format(&time::macros::format_description!("[year]-[month]-[day]"))
        .context("formatting today's date")?;
    let leaf = match slug_suffix {
        Some(s) => format!("cargo-upgrade-{date}-{s}"),
        None => format!("cargo-upgrade-{date}"),
    };
    Ok(Plan {
        branch: leaf.clone(),
        dir: PathBuf::from(".worktrees").join(&leaf),
        base: base.to_string(),
        kind: SectionKind::Upgrade { date },
    })
}

fn print_summary(plan: &Plan) {
    println!("Created worktree: {}/", plan.dir.display());
    println!("  Branch:  {}", plan.branch);
    match &plan.kind {
        SectionKind::Beads { id, title, github_url } => {
            println!("  Beads:   {id} — {title}");
            if let Some(url) = github_url {
                println!("  GitHub:  {url}");
            }
        }
        SectionKind::Issue { number, title, url } => {
            println!("  Issue:   #{number} — {title}");
            println!("  URL:     {url}");
        }
        SectionKind::Upgrade { date } => {
            println!("  Task:    Cargo dependency upgrade — {date}");
        }
    }
    println!();
    println!("Next steps:");
    println!("  1. Fill in plan file path in CLAUDE.local.md (once plan is created)");
    println!(
        "  2. cd {} && npm install  (if hub-client work is in scope)",
        plan.dir.display()
    );
    println!("  3. Start Claude Code session in {}/", plan.dir.display());
    if let SectionKind::Beads { id, .. } = &plan.kind {
        println!("  4. Run: br update {id} --status in_progress");
    }
}
```

- [ ] **Step 2: Verify compile + tests still pass**

Run: `cargo check -p xtask && cargo nextest run -p xtask`
Expected: clean build, all unit tests pass.

- [ ] **Step 3: `cargo xtask verify --skip-hub-build`**

Ask Chris to run this — per `feedback_verification_not_background`, don't run heavy verification in background.

Run command Chris should execute (in main worktree, NOT this one):

```bash
cargo xtask verify --skip-hub-build
```

Expected: passes. If failures, treat them as regressions to fix before continuing.

- [ ] **Step 4: Commit**

```bash
git add crates/xtask/src/create_worktree.rs
git commit -m "xtask(create-worktree): wire run() to dispatch + summary"
```

---

## Phase E — End-to-end smoke test (Chris-driven)

Important: this worktree (`bd-spsv-create-worktree-xtask`) cannot be the smoke-test target — it was set up with the manual commands the xtask replaces. Smoke-test by creating a throwaway worktree per mode, then cleaning up.

**Idempotency scope:** the command is **not** rerun-idempotent end-to-end — `git worktree add` errors on existing directories, by design. File-level idempotency of `update_claude_local_md` (re-running on an existing CLAUDE.local.md updates the section in place) is covered by unit tests in Task B6 (`update_is_idempotent`, `update_preserves_user_content_below_section`). Phase E does not re-verify what those tests already cover.

**Shell:** these commands assume Git Bash on Windows (or any POSIX shell on Linux/macOS). On Windows: open Git Bash, not PowerShell — `cat`, `grep`, `printf`, `xargs`, `mkdir -p`, and `$(...)` substitution all rely on it.

Chris runs each block; any failure is a defect to fix before proceeding to Phase F.

- [ ] **Step 1: Build the binary once**

```bash
cargo build -p xtask
cargo xtask create-worktree --help
# Expected: help text with [BEADS_ID], --issue, --upgrade, --slug, --base.
```

- [ ] **Step 2: Beads mode**

```bash
cargo xtask create-worktree bd-spsv --slug e2e-beads
cat .worktrees/bd-spsv-e2e-beads/.beads/redirect      # → ../../../.beads
cat .worktrees/bd-spsv-e2e-beads/CLAUDE.local.md      # → managed section + Beads + (GitHub if external_ref)
(cd .worktrees/bd-spsv-e2e-beads && br where)         # → main .beads via redirect
```

- [ ] **Step 3: Issue mode (pick an open issue dynamically)**

```bash
ISSUE=$(gh issue list --repo quarto-dev/q2 --state open --limit 1 --json number --jq '.[0].number')
cargo xtask create-worktree --issue "$ISSUE" --slug e2e-issue
cat ".worktrees/issue-${ISSUE}-e2e-issue/CLAUDE.local.md"   # → has GitHub line, no resolved Beads line
```

- [ ] **Step 4: Upgrade mode**

```bash
cargo xtask create-worktree --upgrade --slug e2e-upgrade
ls .worktrees/cargo-upgrade-*-e2e-upgrade/CLAUDE.local.md   # → upgrade variant
```

- [ ] **Step 5: Failure cases**

```bash
# 5a. Existing directory collision
mkdir -p .worktrees/collision-test
cargo xtask create-worktree bd-spsv --slug collision-test
# Expected: clear error before any git operation. Then:
rmdir .worktrees/collision-test

# 5b. Invalid --slug grammar (path-separator, traversal, whitespace)
cargo xtask create-worktree bd-spsv --slug "foo/bar"
# Expected: error from validate_slug — no worktree created.
cargo xtask create-worktree bd-spsv --slug ".."
# Expected: error from validate_slug — no worktree created.

# 5c. Re-running on existing worktree (idempotency is NOT a goal here)
cargo xtask create-worktree bd-spsv --slug e2e-beads
# Expected: fails with "worktree directory already exists" — by design.
# If you need to refresh the CLAUDE.local.md section, remove the worktree
# and recreate, or hand-edit the file (the BEGIN/END markers make this safe).
```

- [ ] **Step 6: Cleanup**

```bash
git worktree remove .worktrees/bd-spsv-e2e-beads
git worktree remove ".worktrees/issue-${ISSUE}-e2e-issue"
git worktree remove .worktrees/cargo-upgrade-*-e2e-upgrade

# Delete the branches the e2e run created (no commits should have been added)
git branch -d beads/bd-spsv-e2e-beads "issue-${ISSUE}-e2e-issue"
# Upgrade branch name embeds today's date — list and delete:
git branch | grep 'cargo-upgrade-.*-e2e-upgrade' | xargs -r git branch -d
```

- [ ] **Step 7: Record the smoke-test transcript**

Capture exact output from steps 2-4 and paste into the eventual PR body under § End-to-end verification. This satisfies q2 CLAUDE.md "End-to-end verification before declaring success".

---

## Phase F — Documentation and skills

These edits happen after the code is green so the docs reference behavior that demonstrably works.

### Task F1: `.gitignore`

**Files:**
- Modify: `.gitignore`

- [ ] **Step 1: Append CLAUDE.local.md ignore**

Add after the last existing entry (currently `.claude/scheduled_tasks.lock` on line 36):

```gitignore

# Per-session local context (managed by `cargo xtask create-worktree` for worktrees)
CLAUDE.local.md
```

The bare filename matches CLAUDE.local.md in any directory, not just the root.

- [ ] **Step 2: Verify no tracked CLAUDE.local.md exists**

Run: `git ls-files | grep -i claude.local.md`
Expected: empty output.

- [ ] **Step 3: Commit**

```bash
git add .gitignore
git commit -m "gitignore: ignore CLAUDE.local.md everywhere"
```

---

### Task F2: `.claude/rules/xtask.md`

**Files:**
- Modify: `.claude/rules/xtask.md` (commands table)

- [ ] **Step 1: Add the row**

Replace the commands table so it reads:

```markdown
| Command | Alias | Purpose |
|---------|-------|---------|
| `cargo xtask dev-setup` | `cargo dev-setup` | Install required dev tools (cargo-nextest, wasm-bindgen-cli) |
| `cargo xtask lint` | — | Run custom lint checks |
| `cargo xtask create-worktree` | — | Create git worktree + `.beads/redirect` + CLAUDE.local.md context stub |
| `cargo xtask verify` | — | Full project verification (build + tests for Rust and hub-client) |
```

- [ ] **Step 2: Commit**

```bash
git add .claude/rules/xtask.md
git commit -m "rules/xtask: document create-worktree command"
```

---

### Task F3: `.claude/rules/worktrees.md`

**Files:**
- Modify: `.claude/rules/worktrees.md`

- [ ] **Step 1: Replace § Fresh worktree bootstrap (lines 14-24)**

Replace that section with:

```markdown
## Fresh worktree bootstrap

Use `cargo xtask create-worktree <bd-id>` (or `--issue N` / `--upgrade`) for new worktrees —
it handles `git worktree add`, `.beads/redirect`, and the CLAUDE.local.md context stub in
one shot. After it finishes, run `npm install` from the new worktree if hub-client is in scope:

```bash
cargo xtask create-worktree bd-XXXX
cd .worktrees/<id>-<slug>
npm install                              # only if hub-client work is in scope
cargo xtask verify --skip-hub-build      # confirm green at branch HEAD
```

If the xtask is not yet built (fresh clone, or a branch where `cargo build -p xtask` has
not run), see § Manual bootstrap below.

`cargo xtask dev-setup` exists for Rust dev tools (cargo-nextest, wasm-bindgen-cli) but
does not currently run `npm install`. bd-7giz tracks extending it.
```

- [ ] **Step 2: Add § CLAUDE.local.md after § Beads Redirect**

Insert a new section after the existing § Beads Redirect (which ends around line 36 with `Verify with \`br where\` from inside the worktree.`):

```markdown

## CLAUDE.local.md

`cargo xtask create-worktree` prepends a worktree context section to `CLAUDE.local.md`.
Claude Code loads it automatically — no need to run `br show` to orient at session start.

The section contains: worktree declaration, main repo path (`../..`), beads ID,
GitHub URL, and a placeholder for the plan file path (fill in manually after creating
the plan).

Status lives in beads, not in this file. Run `br show <id>` for current status + notes.

The section is delimited by `<!-- BEGIN/END WORKTREE CONTEXT -->` markers so it can be
refreshed in place (e.g. when a worktree is recreated, or by hand-editing the file).
The `update_claude_local_md` rewrite is idempotent at the file level: re-running it on
a file that already has a managed section replaces that section without duplicating it
and preserves any user content below.

`cargo xtask create-worktree` itself is **not** idempotent end-to-end — `git worktree add`
fails fast if the directory already exists. To refresh a worktree's CLAUDE.local.md,
either edit it by hand (the markers make this safe) or remove the worktree and recreate.
```

- [ ] **Step 3: Add § Manual bootstrap at the end**

Append at the end of the file (after § Pushing for PR):

```markdown

## Manual bootstrap

If `cargo xtask create-worktree` is unavailable (fresh clone before first build, or
the xtask binary is broken on the current branch), fall back to manual setup:

```bash
git worktree add -b beads/<id>-<slug> .worktrees/<id>-<slug> main
echo "../../../.beads" > .worktrees/<id>-<slug>/.beads/redirect
# Optional but recommended: write a CLAUDE.local.md context stub manually
# using the template from `cargo xtask create-worktree --help` output.
```

Verify with `br where` from inside the worktree.
```

- [ ] **Step 4: Commit**

```bash
git add .claude/rules/worktrees.md
git commit -m "rules/worktrees: xtask-first bootstrap + CLAUDE.local.md + Manual fallback"
```

---

### Task F4: Skill — `investigate-beads`

**Files:**
- Modify: `.claude/skills/investigate-beads/SKILL.md` (around lines 76-80)

- [ ] **Step 1: Replace the inline git commands**

Find this block:

```bash
git worktree add -b beads/<id>-<slug> .worktrees/<id>-<slug> main
echo "../../../.beads" > .worktrees/<id>-<slug>/.beads/redirect
```

Replace with:

```bash
cargo xtask create-worktree <id>
# Creates the worktree, .beads/redirect, and CLAUDE.local.md context stub.
# Slug is auto-derived from the beads title; pass `--slug X` to override.
# Fallback for fresh clones where the xtask is not yet built:
# see .claude/rules/worktrees.md § Manual bootstrap.
```

- [ ] **Step 2: Commit**

```bash
git add .claude/skills/investigate-beads/SKILL.md
git commit -m "skills/investigate-beads: use cargo xtask create-worktree"
```

---

### Task F5: Skill — `triage`

**Files:**
- Modify: `.claude/skills/triage/SKILL.md` (around lines 50-54)

- [ ] **Step 1: Replace the inline git commands**

Find:

```bash
git worktree add -b issue-<N> .worktrees/issue-<N> main
echo "../../../.beads" > .worktrees/issue-<N>/.beads/redirect
```

Replace with:

```bash
cargo xtask create-worktree --issue <N>
# Creates the worktree, .beads/redirect, and CLAUDE.local.md context stub.
# This step runs BEFORE the beads issue is created (step 6) — the `--issue` template
# intentionally has no Beads line. After step 6, either fill the bd-XXXX ID into
# CLAUDE.local.md manually, or re-run `cargo xtask create-worktree <bd-id>` to
# upgrade the section.
# Fallback for fresh clones where the xtask is not yet built:
# see .claude/rules/worktrees.md § Manual bootstrap.
```

- [ ] **Step 2: Commit**

```bash
git add .claude/skills/triage/SKILL.md
git commit -m "skills/triage: use cargo xtask create-worktree --issue"
```

---

### Task F6: Skill — `upgrade-cargo-deps`

**Files:**
- Modify: `.claude/skills/upgrade-cargo-deps/SKILL.md` (around lines 118-127)

- [ ] **Step 1: Replace the inline git commands**

Find the two-block sequence:

```bash
DATE=$(date +%Y-%m-%d)
git worktree add -b cargo-upgrade-$DATE .worktrees/cargo-upgrade-$DATE main
```

```bash
echo "../../../.beads" > .worktrees/cargo-upgrade-$DATE/.beads/redirect
```

Replace both with one block:

```bash
cargo xtask create-worktree --upgrade
# Creates a cargo-upgrade-YYYY-MM-DD worktree with .beads/redirect and CLAUDE.local.md.
# Fallback for fresh clones where the xtask is not yet built:
# see .claude/rules/worktrees.md § Manual bootstrap.
```

- [ ] **Step 2: Commit**

```bash
git add .claude/skills/upgrade-cargo-deps/SKILL.md
git commit -m "skills/upgrade-cargo-deps: use cargo xtask create-worktree --upgrade"
```

---

## Phase G — Final verification and handoff

### Task G1: Full verify pass

- [ ] **Step 1: Ask Chris to run `cargo xtask verify --skip-hub-build`**

Per `feedback_verification_not_background`, hand the command to Chris rather than running it in this session. If the only Rust changes are inside xtask + docs + skills, `--skip-hub-build` is sufficient. Re-running with full `cargo xtask verify` is only needed if any change touched `quarto-core`, `quarto-pandoc-types`, or anything else hub-client depends on — none of those are in the touched-file list, so `--skip-hub-build` covers the change.

Expected: green.

- [ ] **Step 2: Confirm clean working tree**

Run: `git status`
Expected: clean (all phase F edits already committed).

- [ ] **Step 3: Update beads with progress**

Append a comment to `bd-spsv`:

```bash
br comments add bd-spsv "Implementation complete on branch beads/bd-spsv-create-worktree-xtask. Smoke tested all 3 modes plus idempotency, preservation, and collision cases."
```

- [ ] **Step 4: Stop and hand off**

Per CLAUDE.md "NEVER push to the remote repository without explicit user permission" and `feedback_explain_shared_file_changes`, do NOT push. Summarize the final commit list to Chris and ask for permission before any push or PR.

---

## Open caveats called out in the design

1. **Self-bootstrap caveat:** this worktree was set up with manual git+echo. The new command cannot be used to bootstrap its own development worktree (chicken-and-egg). The first real-world end-to-end test of the new command happens after this PR lands on `main` and a developer creates the next worktree.

2. **`--upgrade` bool in ArgGroup:** Task A3 Step 5 explicitly tests this works in the installed clap version. If it does not, fall back per the note there before proceeding to Phase B.

3. **Filesystem-pure:** the xtask never calls `br create`, `br update`, or any state-changing beads command. Skill instructions in Phase F preserve the existing per-skill beads lifecycle steps.

4. **Idempotency scope (file-level only).** The design doc used the word "idempotent" loosely. The actual contract is:
   - `update_claude_local_md` is idempotent at the **file** level — re-running it on a file with an existing managed section replaces that section in place.
   - `cargo xtask create-worktree` is **not** idempotent at the **command** level — `git worktree add` errors fast on directory collision. A retry must first remove the partial worktree (the rollback path in `run()` does this on failure between `git_worktree_add` and `update_claude_local_md`).

5. **`--slug` grammar.** Overrides go through `validate_slug` (ASCII alnum + `-` + `_`, no leading/trailing dash, no `..`/`.`, max 64 chars). Auto-derived slugs already satisfy this by construction.

6. **Marker-collision defense.** Externally-sourced titles (`br`/`gh`) are passed through `marker_safe` before interpolation so a literal `<!-- END WORKTREE CONTEXT -->` in a title cannot terminate the managed section prematurely.
