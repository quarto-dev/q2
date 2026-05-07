# Plan: `cargo xtask create-worktree` + CLAUDE.local.md worktree context

## Context

Worktree creation is currently manual: each skill (`triage`, `investigate-beads`, `upgrade-cargo-deps`) has a copy of the same `git worktree add` + `echo ... > .beads/redirect` bash commands — duplicated, no automation, no context left behind for the next session.

When starting a new Claude Code session in an existing worktree, there is no file that immediately answers: "what are we working on here, where is the main repo, what's the beads issue?" The developer (or Claude) has to re-explore. CLAUDE.local.md solves this — Claude Code loads it automatically at session start.

Two additional problems solved:
- `CLAUDE.local.md` is in Chris's global gitignore but NOT in q2's `.gitignore` — other contributors would accidentally commit one without the global rule
- Similar patterns in other quarto-dev projects confirm this is the right approach; q2 needs its own equivalent

**Goals:**
1. Single `cargo xtask create-worktree` command handles all worktree setup
2. Command prepends a clearly-marked worktree context section to `CLAUDE.local.md` (safe for existing content)
3. CLAUDE.local.md holds *context* only — beads tracks status, not this file
4. Works safely for all devs; `br` and `gh` are project dependencies — hard fail if missing

## CLAUDE.local.md design

The xtask prepends a delimited section to the file. Delimiter markers allow idempotent updates (re-running the command updates the section rather than duplicating it).

**Content:** worktree declaration, main repo relative path, beads ID (pointer only — run `br show` for live status), GitHub URL, plan file placeholder.

```markdown
<!-- BEGIN WORKTREE CONTEXT — managed by cargo xtask create-worktree -->
# Worktree Context

This is a **worktree** of the q2 repository. Main repo: `../..`

**Beads:** bd-1d3e — Fix CRLF test failures in quarto-doctemplate on Windows
**GitHub:** https://github.com/quarto-dev/q2/issues/157
**Plan:** <!-- fill in after creating: claude-notes/plans/YYYY-MM-DD-name.md -->

Run `br show bd-1d3e` for current status and notes.
<!-- END WORKTREE CONTEXT -->

```

For issue workflow (no pre-existing beads issue):
```markdown
<!-- BEGIN WORKTREE CONTEXT — managed by cargo xtask create-worktree -->
# Worktree Context

This is a **worktree** of the q2 repository. Main repo: `../..`

**GitHub issue:** #157 — <title>
**URL:** https://github.com/quarto-dev/q2/issues/157
**Beads:** (run `br search 157` to find or create a beads issue)
**Plan:** <!-- fill in after creating: claude-notes/plans/YYYY-MM-DD-name.md -->
<!-- END WORKTREE CONTEXT -->

```

For upgrade workflow:
```markdown
<!-- BEGIN WORKTREE CONTEXT — managed by cargo xtask create-worktree -->
# Worktree Context

This is a **worktree** of the q2 repository. Main repo: `../..`

**Task:** Cargo dependency upgrade — 2026-05-07
**Plan:** <!-- fill in if needed -->
<!-- END WORKTREE CONTEXT -->

```

## Command interface

```bash
cargo xtask create-worktree bd-1d3e              # beads workflow
cargo xtask create-worktree --issue 157          # GH issue triage workflow
cargo xtask create-worktree --upgrade            # cargo-upgrade (date-based branch)
```

Optional flags (all modes):
- `--slug <slug>` — override auto-derived slug (default: derived from title — see § Slug derivation)
- `--base <branch>` — base branch (default: `main`)

Mode selection enforced by `clap::ArgGroup(required=true, multiple=false)` so that exactly one of `<bd-id>` / `--issue` / `--upgrade` is set; clap auto-generates "exactly one of" error message. No runtime mode-validation needed.

## Files to modify

| File | Change |
|---|---|
| `.gitignore` | Add `CLAUDE.local.md` line — protects the **main repo root** case (a contributor without a global gitignore rule could otherwise commit one). `.worktrees/` is already gitignored (line 32), so worktree-internal CLAUDE.local.md is already safe. Use top-level pattern (matches root + recursive — gitignore semantics: bare filename matches in any directory unless prefixed with `/`). |
| `crates/xtask/Cargo.toml` | Add `time = { version = "0.3", features = ["macros", "formatting"] }` to `[dependencies]` for date formatting (already in `Cargo.lock` transitively — zero compile cost). Both features are required: `macros` for `format_description!`, `formatting` for `OffsetDateTime::format`. |
| `crates/xtask/src/main.rs` | Add `CreateWorktree { ... }` struct-style variant to `Command` enum (matches existing pattern — see `DevSetup`, `Lint`, `Verify`) + `mod create_worktree;` + update top-level doc comment |
| `crates/xtask/src/create_worktree.rs` | New file — full implementation |
| `.claude/rules/xtask.md` | Add `create-worktree` row to commands table |
| `.claude/rules/worktrees.md` | Add § CLAUDE.local.md; replace § Fresh worktree bootstrap with xtask-first guidance + new § Manual bootstrap (fallback when xtask unbuilt — referenced from skills) |
| `.claude/skills/investigate-beads/SKILL.md` | Replace inline git commands with `cargo xtask create-worktree <id>`; add "pass `--slug X` to override auto-derived slug" guidance |
| `.claude/skills/triage/SKILL.md` | Replace inline git commands with `cargo xtask create-worktree --issue <N>`; explicitly note this runs BEFORE the skill's beads-creation step (the `--issue` template has no Beads line on purpose) |
| `.claude/skills/upgrade-cargo-deps/SKILL.md` | Replace inline git commands with `cargo xtask create-worktree --upgrade` |

## Implementation: create_worktree.rs

```rust
#[derive(clap::Args)]
#[command(group(clap::ArgGroup::new("mode").required(true).multiple(false)))]
pub struct Args {
    /// Beads issue ID, e.g. `bd-1d3e`. Reads `br show <id>` for title and external_ref.
    #[arg(group = "mode")]
    beads_id: Option<String>,

    /// GitHub issue number, e.g. `157`. Reads `gh issue view`.
    #[arg(long, group = "mode")]
    issue: Option<u32>,

    /// Cargo dependency upgrade — uses today's date for branch name.
    #[arg(long, group = "mode")]
    upgrade: bool,

    /// Override auto-derived slug. Behavior depends on mode:
    /// beads = full override of derived slug; issue / upgrade = appended as suffix
    /// (for test isolation or parallel-worktree workflows).
    #[arg(long)]
    slug: Option<String>,

    /// Base branch.
    #[arg(long, default_value = "main")]
    base: String,
}
```

**Steps in `pub fn run(args: Args) -> Result<()>`:**

### 1. Mode determination
Handled by `clap::ArgGroup` (required + single) — no runtime check needed. Match on which `Option<...>`/`bool` is set.

### 2. Fetch metadata

**Beads mode:** run `br show <id> --json`, parse JSON array:
- `.[0].title` — used for slug derivation + CLAUDE.local.md template
- `.[0].external_ref` — used for GitHub URL when present (format: `gh-157` → `https://github.com/quarto-dev/q2/issues/157`)

If `external_ref` is `null` or absent: omit the **GitHub** line in the CLAUDE.local.md template; do not error. If non-`gh-` external ref (e.g. some future system): omit and warn on stderr.

If `br` exits non-zero: surface stderr verbatim plus prefix `br show <id> failed:`. On success, ignore stderr (br can emit non-fatal warnings on stderr — e.g. sync-state notes — that should not be surfaced). If `br` not found in PATH: error `br is required — install via cargo install beads-rust or see project README`.

**Issue mode:** run `gh issue view <N> --repo quarto-dev/q2 --json title,url`, parse result.

If `gh` issue does not exist: surface gh's error verbatim. (`gh issue view` will not match a PR — `gh pr view` is a separate subcommand.) If `gh` not found: error `gh is required — see https://cli.github.com/`.

**Upgrade mode:** no fetch. Use today's date (see § Date formatting).

### 3. Derive slug

If `--slug` provided: use it verbatim (no validation — caller's responsibility).

Otherwise from title, with explicit handling for kebab boundaries, stop-words, and empty result:

1. Lowercase the title.
2. Split on whitespace **and** `-` (so `quarto-doctemplate` becomes `["quarto", "doctemplate"]`, preserving kebab boundaries).
3. For each token: keep only ASCII alphanumerics (`[a-z0-9]`). This drops punctuation including smart quotes, en-dashes, parens, brackets, colons. Non-ASCII Unicode (CJK, accented chars) is also dropped — explicit ASCII-only policy keeps slugs predictable on every filesystem and shell. If a contributor needs different characters, `--slug` overrides.
4. Drop empty tokens.
5. Drop stop-words: `a`, `an`, `the`, `and`, `or`, `in`, `on`, `of`, `to`, `for`, `with`, `from`, `at`, `by`, `is`, `as`. Document this list in a `const STOP_WORDS: &[&str]` so it's discoverable + testable.
6. Take first **4** remaining tokens.
7. Join with `-`.
8. **Empty-result fallback:** if step 7 produces an empty string, return error `unable to derive slug from title "<title>" — pass --slug <name> to override`.

Worked example: "Fix CRLF test failures in quarto-doctemplate on Windows"
- after step 1–2: `["fix", "crlf", "test", "failures", "in", "quarto", "doctemplate", "on", "windows"]`
- after step 3–4: same (already alphanumeric)
- after step 5 (drop `in`, `on`): `["fix", "crlf", "test", "failures", "quarto", "doctemplate", "windows"]`
- after step 6 (first 4): `["fix", "crlf", "test", "failures"]`
- after step 7: `fix-crlf-test-failures` ✓

### 4. Determine branch + directory

| Mode | Default branch | Default directory | When `--slug X` provided |
|---|---|---|---|
| Beads | `beads/<id>-<derived-slug>` | `.worktrees/<id>-<derived-slug>` | `<derived-slug>` is **replaced** by `<X>` (override) |
| Issue | `issue-<N>` | `.worktrees/issue-<N>` | Appended as suffix → `issue-<N>-<X>` (for test isolation) |
| Upgrade | `cargo-upgrade-<YYYY-MM-DD>` | `.worktrees/cargo-upgrade-<YYYY-MM-DD>` | Appended → `cargo-upgrade-<DATE>-<X>` |

Rationale for asymmetry: in beads mode the slug carries the issue title and is the natural override target. In issue + upgrade modes the canonical directory format is the stable identity (issue number, date) — `--slug X` exists only to enable parallel-worktree workflows or test isolation, so it's a suffix.

Error if the resulting directory already exists.

### 5. `git worktree add`

```
git worktree add -b <branch> <dir> <base>
```

Pre-check: if `<dir>` already exists or `<branch>` already exists locally, return clear error before invoking git (`worktree directory already exists: <dir>`, `branch already exists: <branch> — remove it or pass --slug to disambiguate`). Otherwise propagate git's exit + stderr verbatim. We don't try to recover from git failures — surface them.

### 6. `.beads/redirect`

Write `../../../.beads\n` to `<dir>/.beads/redirect` (LF line ending, even on Windows — git on Windows reads it fine and avoids gratuitous CRLF noise).

(`.beads/` already exists in the worktree from git; `.beads/redirect` is already in `.beads/.gitignore` — no git noise.)

### 7. CLAUDE.local.md — prepend with markers (idempotent)

**Markers (constants):**
- `BEGIN_MARKER`: `<!-- BEGIN WORKTREE CONTEXT — managed by cargo xtask create-worktree -->`
- `END_MARKER`: `<!-- END WORKTREE CONTEXT -->`

**Algorithm:**

1. Path: `<dir>/CLAUDE.local.md`.
2. If the path exists, check `metadata().is_file()` — if it's a directory, symlink, or junction (Windows reparse point) that does not point at a regular file, error: `CLAUDE.local.md exists but is not a regular file: <path>`. (The xtask only writes to fresh worktrees; we will not silently overwrite an existing target.) If the file does not exist, treat existing content as empty.
3. Read existing content if present (`fs::read_to_string` — fails on non-UTF-8, which is desired).
4. **Detect old section.** Search for `BEGIN_MARKER`. If absent → no strip needed; new content prepends with a single blank line separator before existing content (or no separator if existing content is empty).
5. **If `BEGIN_MARKER` present:**
   - Find the **first** occurrence (multiple BEGIN markers indicate prior corruption — use first, log a stderr warning suggesting manual review).
   - Search **after** the first BEGIN for `END_MARKER`. If `END_MARKER` is missing, error: `CLAUDE.local.md has BEGIN marker without END marker — refusing to modify; resolve manually`. Do not consume to EOF.
   - Strip everything from the start of `BEGIN_MARKER` line through the end of `END_MARKER` line, plus exactly one trailing newline if present (but not more — preserves blank lines authored by the user).
6. **Line-ending handling.** Detect `\r\n` vs `\n` once on read (sniff first 1KB). Decision rule:
   - File new, empty, or no newline observed in sniff window → default to **LF**.
   - Sniff window contains `\r\n` (any count) → use **CRLF** for the entire write.
   - Sniff window contains only `\n` → use **LF**.
   - Mixed `\r\n` + bare `\n` → use **LF** (treat as primarily-LF file with stray CR; do not propagate inconsistency).

   Write the new section using the chosen ending; never mix endings within a single write. (Worktree CLAUDE.local.md is gitignored, so `.gitattributes` does not normalize at commit; detection has to be runtime-correct.)
7. **Prepend new section** (template from `## CLAUDE.local.md design`) followed by a single blank line, followed by the (possibly stripped) remaining content. Ensure final file ends with a single trailing newline.
8. Write atomically: write to `<path>.tmp` then rename. (Avoids half-written file on crash.)

This makes the command **idempotent**: running it twice updates the section in place without duplicating or destroying other content.

**Failure cases tabulated:**

| Condition | Behavior |
|---|---|
| File missing | Create with new section + trailing newline |
| File present, no BEGIN | Prepend section + blank line + existing content |
| File present, BEGIN + END | Strip section, prepend new |
| File present, BEGIN without END | Error, refuse to modify |
| File present, multiple BEGIN | Warn, strip from first BEGIN |
| Path is directory / non-file | Error, refuse to modify |
| Non-UTF-8 content | Error from `read_to_string`, surface |

### 8. Print summary

```
Created worktree: .worktrees/bd-1d3e-fix-crlf-test-failures/
  Branch:  beads/bd-1d3e-fix-crlf-test-failures
  Beads:   bd-1d3e — Fix CRLF test failures in quarto-doctemplate on Windows
  GitHub:  https://github.com/quarto-dev/q2/issues/157

Next steps:
  1. Fill in plan file path in CLAUDE.local.md (once plan is created)
  2. cd .worktrees/bd-1d3e-fix-crlf-test-failures && npm install  (if hub-client in scope)
  3. Start Claude Code session in .worktrees/bd-1d3e-fix-crlf-test-failures/
  4. Run: br update bd-1d3e --status in_progress
```

## Date formatting (upgrade mode)

Add `time = { version = "0.3", features = ["macros", "formatting"] }` as a direct `[dependencies]` entry in `crates/xtask/Cargo.toml`. Both `time` and `chrono` are already in the workspace `Cargo.lock` transitively — adding `time` as a direct dep has zero compile cost. The `macros` feature enables `format_description!`; the `formatting` feature enables `OffsetDateTime::format` — neither is in the default feature set.

Format using `time::OffsetDateTime::now_utc().format(&time::macros::format_description!("[year]-[month]-[day]"))`.

Hand-rolling YYYY-MM-DD from `std::time::SystemTime` requires re-implementing Gregorian calendar conversion (epoch-seconds → year/month/day with leap-year + month-length math). ~50 lines of date arithmetic where `time` provides one well-tested function call. Not worth the dependency-zero principle in this case.

## main.rs changes

Match the existing `Command` enum's struct-style pattern (used by `DevSetup`, `Lint`, `Verify`, `BuildAll`). Tuple-style `CreateWorktree(Args)` is incompatible with how the rest of `main.rs` declares fields directly inside the variant — we use a flattened struct embedding via `#[command(flatten)]` instead.

Add to top-level doc comment: `- 'create-worktree': Create git worktree with beads redirect and CLAUDE.local.md`

Add module declaration: `mod create_worktree;`

Add to `Command` enum:
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

Add match arm:
```rust
Command::CreateWorktree { args } => create_worktree::run(args),
```

## Skills update

The xtask is **filesystem-only** — does NOT create or update beads issues. Each skill keeps its existing beads creation/update logic; only the raw git bash gets replaced. The xtask never invokes `br update`, `br create`, or any other state-changing beads command. (We considered having it `br update <id> --notes "worktree at .worktrees/..."` but rejected: skills are the right layer to track lifecycle, the xtask should stay narrow + pure.)

All three skills currently embed:
```bash
git worktree add -b beads/<id>-<slug> .worktrees/<id>-<slug> main
echo "../../../.beads" > .worktrees/<id>-<slug>/.beads/redirect
```

Replace with:
```bash
cargo xtask create-worktree <id>
# Creates worktree, beads redirect, and CLAUDE.local.md stub.
# By default derives slug from `br show` title; pass `--slug X` to override.
# Fallback for fresh clones where xtask is not yet built:
# see .claude/rules/worktrees.md § Manual bootstrap.
```

**Per-skill notes:**

- **`investigate-beads`:** beads issue exists before worktree creation (skill walks dep graph then creates worktree). xtask reads `br show <id>` for title + external_ref → CLAUDE.local.md gets full Beads + GitHub lines.
- **`triage`:** beads issue is created in step 6, **after** worktree creation in step 3. The `--issue` mode template intentionally has no Beads line — it shows `(run br search 157 to find or create a beads issue)`. Skill text needs an explicit callout: "step 3 sets up the worktree; the beads issue lands in step 6 and the developer fills its ID into CLAUDE.local.md manually (or re-runs `cargo xtask create-worktree <bd-id>` to upgrade the section)."
- **`upgrade-cargo-deps`:** no beads issue at worktree-creation time. CLAUDE.local.md template is the upgrade variant.

## worktrees.md additions

Three changes:

**1. Replace § Fresh worktree bootstrap** with xtask-first guidance:

```markdown
## Fresh worktree bootstrap

Use `cargo xtask create-worktree <bd-id>` (or `--issue N` / `--upgrade`) for new worktrees —
it handles `git worktree add`, `.beads/redirect`, and the CLAUDE.local.md context stub in
one shot. After it finishes, `npm install` from the new worktree if hub-client is in scope:

```bash
cargo xtask create-worktree bd-XXXX
cd .worktrees/<id>-<slug>
npm install                              # only if hub-client work is in scope
cargo xtask verify --skip-hub-build      # confirm green at branch HEAD
```

If the xtask is not yet built (fresh clone, or branch where `cargo build -p xtask` has
not run), see § Manual bootstrap below.
```

**2. New § CLAUDE.local.md** after § Beads Redirect:

```markdown
## CLAUDE.local.md

`cargo xtask create-worktree` prepends a worktree context section to `CLAUDE.local.md`.
Claude Code loads it automatically — no need to run `br show` to orient at session start.

The section contains: worktree declaration, main repo path (`../..`), beads ID,
GitHub URL, and a placeholder for the plan file path (fill in manually after creating
the plan).

Status lives in beads, not in this file. Run `br show <id>` for current status + notes.

The section is delimited by `<!-- BEGIN/END WORKTREE CONTEXT -->` markers so it can be
updated by re-running the xtask without disturbing other content. The command is
idempotent.
```

**3. New § Manual bootstrap** (the fallback referenced from skills + § Fresh worktree bootstrap):

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

The `.gitignore` change protects the **main repo root** case: a contributor without a
global `CLAUDE.local.md` ignore rule could otherwise commit one accidentally. Worktree-
internal `CLAUDE.local.md` files are already covered by the existing `.worktrees/` entry.

## Verification

End-to-end verification covers all three modes plus idempotency. Per CLAUDE.md "End-to-end verification before declaring success" — record exact invocations + observed output snippets in the implementation PR description.

```bash
# Build xtask
cargo build -p xtask

# Help (sanity-check clap config)
cargo xtask create-worktree --help

# --- Beads mode ---
cargo xtask create-worktree bd-1d3e --slug e2e-beads
cat .worktrees/bd-1d3e-e2e-beads/.beads/redirect       # → ../../../.beads
cat .worktrees/bd-1d3e-e2e-beads/CLAUDE.local.md       # → worktree context section with Beads + GitHub lines
(cd .worktrees/bd-1d3e-e2e-beads && br where)          # → main .beads/ via redirect

# --- Issue mode ---
# Pick any open issue from the repo for the smoke test; #1 may not exist or may be a PR.
ISSUE=$(gh issue list --repo quarto-dev/q2 --state open --limit 1 --json number --jq '.[0].number')
cargo xtask create-worktree --issue "$ISSUE" --slug e2e-issue
cat .worktrees/issue-${ISSUE}-e2e-issue/CLAUDE.local.md  # → no Beads line, has GitHub line
# (Note: issue-mode directory format follows the issue-<N> convention; --slug suffix
# is appended for test isolation only.)

# --- Upgrade mode ---
cargo xtask create-worktree --upgrade --slug e2e-upgrade
cat .worktrees/cargo-upgrade-<DATE>-e2e-upgrade/CLAUDE.local.md  # → upgrade variant template

# --- Idempotency ---
# Re-run the same command; should update the section in place, not duplicate.
cargo xtask create-worktree bd-1d3e --slug e2e-beads
grep -c "BEGIN WORKTREE CONTEXT" .worktrees/bd-1d3e-e2e-beads/CLAUDE.local.md  # → 1

# --- Preserve existing content ---
# Add user content below the managed section, re-run, confirm preserved.
echo -e "\n# My notes\nfoo" >> .worktrees/bd-1d3e-e2e-beads/CLAUDE.local.md
cargo xtask create-worktree bd-1d3e --slug e2e-beads
grep "My notes" .worktrees/bd-1d3e-e2e-beads/CLAUDE.local.md   # → present

# --- Failure cases (manual checks) ---
# 1. Existing directory collision
mkdir -p .worktrees/collision-test
cargo xtask create-worktree bd-1d3e --slug collision-test     # → clear error, no git operation

# 2. Missing END marker (corrupt CLAUDE.local.md)
printf '<!-- BEGIN WORKTREE CONTEXT -->\nbroken\n' > .worktrees/bd-1d3e-e2e-beads/CLAUDE.local.md
cargo xtask create-worktree bd-1d3e --slug e2e-beads          # → refuses, asks for manual fix

# Cleanup
cd <main repo root>
git worktree remove .worktrees/bd-1d3e-e2e-beads
git worktree remove .worktrees/issue-${ISSUE}-e2e-issue
git worktree remove .worktrees/cargo-upgrade-<DATE>-e2e-upgrade
rm -rf .worktrees/collision-test
# `git branch -d` works for the e2e branches (no commits added during the smoke).
# If a branch has commits (e.g. you committed during the recipe), use `git branch -D`.
git branch -d beads/bd-1d3e-e2e-beads issue-${ISSUE}-e2e-issue cargo-upgrade-<DATE>-e2e-upgrade
```

**Self-bootstrap note for PR reviewers:** the worktree `bd-spsv-create-worktree-xtask` was itself created with the manual git+echo commands the new xtask replaces (chicken-and-egg: the command being added cannot be used to set up its own development worktree). After this PR lands on `main`, the next worktree any developer creates should be the first end-to-end real-world test of the new command.

## Unit test coverage

Add `#[cfg(test)] mod tests` to `create_worktree.rs` covering pure functions (no fs/network):

- `derive_slug` — title with stop-words / kebab boundaries / unicode / mixed case / empty / very short
- `update_claude_local_md` — file missing, BEGIN+END, BEGIN-only, multiple BEGIN, CRLF vs LF, no-newline-in-sniff, mixed-endings, non-UTF-8 (via `&[u8]` round-trip)
- `parse_external_ref_to_github_url` — `gh-157`, null, malformed, non-`gh-` prefix
- Marker-byte stability: `const _: () = assert!(BEGIN_MARKER.contains('\u{2014}'));` — locks against accidental ASCII-hyphen substitution by an editor

The fs/network entry points (`run`, `git_worktree_add`, `fetch_beads_metadata`) are exercised end-to-end via the verification recipes above; not unit-tested.

## Out of scope

- `npm install` in command — tracked by bd-7giz (`dev-setup` extension)
- Automatic CLAUDE.local.md updates at session end — personal `/end` skill territory
- xtask updating beads itself (e.g. `br update <id> --notes ...`) — skills own beads lifecycle, xtask stays filesystem-pure
- Custom per-team stop-word lists / slug strategies — `--slug` override covers edge cases
