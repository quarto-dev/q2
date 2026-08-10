# Quarto Rust monorepo

## **ACTIVE PLAN (READ AFTER COMPACTION)**

After context compaction, IMMEDIATELY read the current plan file:

```
claude-notes/plans/CURRENT.md
```

This symlink points to the active plan. If it doesn't exist or is broken, ask the user which plan to follow.

## **TERMINAL RESET**

If the terminal output becomes corrupted (especially from truncated ANSI link sequences), reset it with:

```bash
printf '\033[0m' && printf '\033]8;;\007' && echo "Terminal reset"
```

When the user asks you to "reset the terminal", run this command.

## **Workflow: Plan-Driven Development (TDD)**

Always follow TDD workflow: write/update tests BEFORE implementing features. When creating plans, include test specifications as the first phase. Never skip to implementation without a test plan.

## **GIT PUSH POLICY**

**NEVER push to the remote repository without explicit user permission.** Always:
1. Stage and commit changes as needed
2. **Verify the full workspace compiles cleanly** (`cargo build --workspace`)
3. **Verify the full workspace tests pass** (`cargo nextest run --workspace`)
4. **Run `cargo xtask verify`** — at minimum `cargo xtask verify --skip-hub-build` for Rust-only changes; full `cargo xtask verify` when the WASM leg could be affected (any change under `quarto-core`, `quarto-pandoc-types`, or anything else hub-client depends on). This is the step that matches CI's `-D warnings` strictness; plain `cargo build` / `cargo nextest` from steps 2 and 3 do not.
5. Ask the user for permission before pushing
6. Only push after receiving explicit approval

This applies even at the end of sessions. Prepare the commit but wait for approval to push.

## Git Workflow

When asked to 'stage and commit everything' or 'commit all changes', stage ALL modified/untracked files (`git add -A`), not just the files Claude edited in the current session.

**Commit-and-continue during approved plan execution:** when executing a plan the user has already approved, commit at each clean phase boundary (pre-commit checklist in `claude-notes/instructions/review.md` passed, full workspace tests green) without stopping to ask, and report the commit in the running summary. Waiting for approval is only required for commits outside approved plan execution, for dirty states, and always for pushing.

### Snapshot Test Changes

When a commit includes updated or new snapshot files (`.snap` files under `snapshots/`), **always explicitly document these changes** in the commit message and in conversation with the user. Snapshot changes can hide unwanted regressions. Specifically:

1. **Report the count** of snapshot files added/modified/removed.
2. **Summarize what changed** — e.g., "45 HTML comment snapshots updated: comments now appear as `RawInline` instead of being dropped."
3. **Call out any surprising changes** — if a snapshot changed in a way that wasn't obviously expected from the code change, flag it for review.
4. After committing, **list the affected snapshot files** so the user can review the diffs before pushing.

## **WORK TRACKING**

We use **braid** for issue tracking instead of Markdown TODOs or external tools.
braid stores all issues for the project in a **skein** (a single
[automerge](https://automerge.org) CRDT document); a single issue is a
**strand**. The skein — synced through a sync server — is the **source of
truth**. There is no git involvement and no `.beads/`-style JSONL to commit:
edits converge through the CRDT, not through merge tooling. (We migrated off
beads_rust on 2026-06-08; see `claude-notes/plans/2026-06-08-braid-migration.md`.)

**`braid` is non-invasive and never executes git commands.** Unlike the old
`br sync --flush-only; git add .beads/` dance, there is **nothing to commit**
after issue work — the skein syncs itself. (A `.braid/snapshot.jsonl` backup
*is* committed periodically, but it is **backup-only and one-directional** —
see the snapshot policy below. Never `braid import` it back.)

For the authoritative, version-matched command guide, run `braid agents-info`
(or invoke the `/braid` skill). The quick reference below is a convenience
summary, not the contract.

We use plans for additional context and bookkeeping. Write plans to
`claude-notes/plans/YYYY-MM-DD-<description>.md`, and reference the plan file
in the strands.

### File Structure
Plan files should include:

1. **Overview**: Brief description of the plan's goals and context
2. **Checklist**: A markdown checklist of all work items using `- [ ]` syntax
3. **Details**: Additional context, design decisions, or implementation notes as needed

### Maintaining Progress
As you work through a plan:

1. **Update the plan file** after completing each work item
2. **Check off items** by changing `- [ ]` to `- [x]`
3. **Keep the plan file current** - it serves as both a roadmap and progress tracker
4. **Add new items** if you discover additional work during implementation

### Excerpt from a simple Plan File

```markdown
...

## Work Items

- [x] Review current runtime service implementations
- [x] Identify common patterns
- [ ] Update StandalonePlatform to use shared base
- [ ] Update tests
- [ ] Update documentation
```

### When to Use Plan Files

Create plan files for:
- Multi-step features spanning multiple packages
- Complex refactoring that requires coordination
- Tasks where tracking progress helps ensure nothing is missed

Complex plans can have phases, and work items are then split into multiple lists, one for each phase.

For simple tasks (single file changes, bug fixes), the TodoWrite tool is sufficient.

### Braid Quick Reference

```bash
# Find ready work (active + unblocked)
braid ready --json

# Create new strand (prints its id; braid assigns a bd-<random> id)
braid create "Strand title" -t bug|feature|task -p 0-4 -d "Description" --json

# Create with labels
braid create "Strand title" -t bug -p 1 -l bug -l critical --json

# Create and link discovered work in one shot
braid create "Found bug in auth" -t bug -p 1 --deps discovered-from:<current-id> --json

# Update status
braid update <id> --status in_progress --json

# Link existing strands (id depends on target)
braid dep add <discovered-id> <parent-id> --type discovered-from

# Complete work
braid close <id> --reason "Done"

# Show an epic's descendant tree / one strand's details
braid dep tree <id>
braid show <id> --json

# Backup snapshot (one-directional — see snapshot policy; NEVER import it back)
braid export > .braid/snapshot.jsonl
```

Notes on the move from beads:
- **No explicit `--id`.** braid assigns collision-free ids; with a CRDT,
  parallel workers never need to pre-agree on ids. (The migration *preserved*
  every existing `bd-XXXX` id via `braid import`, so source references stay
  valid.)
- **No `br create -f <file>` bulk create.** Use `braid import <jsonl>` for bulk.
- **No `br sync --flush-only` / `git add .beads/`.** The skein is the source of
  truth; there is nothing to commit after issue work.

### Workflow

1. **Check for ready work**: `braid ready` to see what's unblocked
2. **Claim your task**: `braid update <id> --status in_progress`
3. **Work on it**: implement, test, document; leave a trail with
   `braid comment <id> "..."`
4. **Discover new work**: file it and link it in one shot:
   `braid create "Found bug in auth" -t bug -p 1 --deps discovered-from:<current-id> --json`
5. **Complete**: `braid close <id> --reason "Implemented"`

That's the whole loop — **no sync-and-commit step.** braid syncs the skein to
the server on every command. (`braid sync` forces a round trip if you want to
confirm convergence.)

### Issue Types

- `bug` - Something broken that needs fixing
- `feature` - New functionality
- `task` - Work item (tests, docs, refactoring)
- `epic` - Large feature composed of multiple strands
- `chore` - Maintenance work (dependencies, tooling)
- `docs` - Documentation work (braid adds this type)
- `question` - Open question to resolve (braid adds this type)

### Priorities

- `0` - Critical (security, data loss, broken builds)
- `1` - High (major features, important bugs)
- `2` - Medium (nice-to-have features, minor bugs)
- `3` - Low (polish, optimization)
- `4` - Backlog (future ideas)

### Dependency Types

- `blocks` - Hard dependency (X depends on / is blocked by Y)
- `parent-child` - Epic/subtask relationship
- `related` - Soft relationship (strands are connected)
- `discovered-from` - Track strands discovered during work
- braid also accepts `conditional-blocks`, `waits-for`, `replies-to`,
  `duplicates`, `supersedes`, `caused-by`.

**What gates `ready`:** `blocks`, `conditional-blocks`, and `waits-for` make a
strand unready while their target is active. `parent-child` does **not** block
the child (children stay workable); instead an open child blocks the *parent's*
close. `related`/`discovered-from` and the rest are informational.

> Note: this differs subtly from beads, where `parent-child` could make a child
> read as blocked. In braid the child is always workable and the parent refuses
> to close while children are open — the intended epic semantics.

### Snapshot backup policy (READ THIS)

The skein (automerge CRDT) is the **single source of truth**. We additionally
commit a `.braid/snapshot.jsonl` (`braid export`) to the repo so issues stay
greppable in PRs, diffable in git history, and recoverable. This snapshot is
**backup-only and strictly one-directional**:

- It flows **automerge → file only** (`cargo xtask braid-snapshot`, or
  `braid export > .braid/snapshot.jsonl`). It is **never** an import or sync
  source back into the skein. **Never run `braid import .braid/snapshot.jsonl`.**
- On any git **conflict** in `.braid/snapshot.jsonl`, do **not** hand-merge:
  resolve by regenerating from the live skein (`braid export`). The CRDT is
  authoritative; the file is a photograph. (Yes, this means the snapshot on one
  branch may show strand state created on another — "cross-branch
  contamination" is expected and fine, because the snapshot is not the truth.)
- The snapshot lives on whatever work branch you're on; it is not special.

The only time JSONL is ever imported is the **one-time migration** (beads'
`.beads/issues.jsonl` → braid), which is already done.

## Where information lives (memory vs. repo)

Claude's project-memory system (`~/.claude/projects/.../memory/`) is
**per-user and per-machine**. It does not sync to colleagues, does not
appear in code review, and cannot be corrected via a PR. Do not put
project-wide facts there.

Before writing a `project`-type memory, ask two questions:

1. **Would a colleague benefit from seeing this in their own Claude
   session?** If yes, it belongs in the repo, not in memory.
2. **Is it already captured in a repo artifact?** If yes, memory is
   redundant.

Where project-wide information should live instead:

- **`CLAUDE.md`** — always-on guidance, project conventions, commands.
- **`claude-notes/plans/`** — decisions, rationale, in-progress work.
- **`claude-notes/research/`** — findings, audits, reference material.
- **Code comments** — invariants local to specific code.
- **Commit messages + `braid`** — temporal context, who did what when.

What memory is *actually* appropriate for:

- `user` type — facts about the user (role, expertise, what they're
  working on this week). Not visible in code.
- `feedback` type — preferences the user has expressed about how I
  should work with them (style, terseness, confirmation thresholds).
- `reference` type — pointers to external systems (Linear project
  IDs, dashboard URLs, Slack channels).
- `project` type — narrow cases where something affects *my*
  behavior with *this user* specifically and isn't already captured
  anywhere in the repo.

**Red flags** that a proposed memory should go in the repo instead:

- It's an architectural decision ("we chose X over Y").
- It's project state everyone needs ("crate Z is being removed").
- It duplicates something already in `CLAUDE.md` or a plan/research
  note.
- Future agents running on a colleague's machine would be confused or
  misled without it.

## **CRITICAL - TEST-DRIVEN DEVELOPMENT**

When fixing ANY bug:
1. **FIRST**: Write the test
2. **SECOND**: Run the test and verify it fails as expected
3. **THIRD**: Implement the fix
4. **FOURTH**: Run the test and verify it passes
5. **FIFTH**: Run the full workspace test suite (`cargo nextest run --workspace`) to verify no regressions in other crates

**Step 5 is critical because this is a monorepo — changes in one crate (e.g. `pampa`) can break downstream crates (e.g. `qmd-syntax-helper`) that depend on it. Running only the modified crate's tests is NOT sufficient.**

**This is non-negotiable. Never implement a fix before verifying the test fails. Stop and ask the user if you cannot think of a way to mechanically test the bad behavior. Only deviate if writing new features.**

**Do NOT close a braid test-suite strand unless all tests pass. If you feel you're low on tokens, report that and open subtasks to work on new sessions.**

## Workspace structure

### `crates/` - all Rust crates in the workspace

**Binaries:**
- `quarto`: main entry point for the `quarto` command line binary (includes `quarto hub` subcommand)
- `hub`: collaborative editing server for Quarto projects (also available as `quarto hub`)
- `pampa`: parse qmd text and produce Pandoc AST and other formats
- `qmd-syntax-helper`: help users convert qmd files to the new syntax

**Core libraries:**
- `quarto-core`: core rendering infrastructure for Quarto
- `quarto-util`: shared utilities for Quarto crates
- `quarto-error-catalog`: Quarto's `Q-*` error-code catalog data + the `CatalogProvider` it installs into `quarto-error-reporting`

**Externalized foundation crates** (published to crates.io from their own `posit-dev/` repos; consumed here as version deps, no longer in `crates/`):
- `quarto-error-reporting`: uniform, helpful, beautiful error messages — now **catalog-agnostic** (the `Q-*` data lives in the in-tree `quarto-error-catalog`). Repo: `posit-dev/quarto-error-reporting`. The `json` wire shape is behind a default-off `json` feature; q2's wire-shape consumers enable it.
- `quarto-source-map`: maintain source location information for data structures. Repo: `posit-dev/quarto-source-map`. (See `claude-notes/plans/2026-06-26-extract-error-reporting-foundation.md` for the extraction.)
- `quarto-yaml`: YAML parser with accurate fine-grained source locations. Repo: `posit-dev/quarto-yaml` (a two-crate workspace). q2 consumes it as a version dep. (See `claude-notes/plans/2026-06-29-yaml-stack-extraction-handoff.md` for the extraction.)
- `quarto-yaml-validation`: schema validation for YAML objects. Published from the same `posit-dev/quarto-yaml` workspace, but **q2 does not depend on it** — it was extracted purely for external Posit consumers; the demo binary `validate-yaml` was its only in-tree consumer and was deleted at cutover.

**Parsing libraries:**
- `quarto-xml`: source-tracked XML parsing
- `quarto-parse-errors`: parse error infrastructure

**Pandoc/document processing:**
- `quarto-pandoc-types`: Pandoc AST type definitions
- `quarto-doctemplate`: Pandoc-compatible document template engine
- `quarto-csl`: CSL (Citation Style Language) parsing with source tracking
- `quarto-citeproc`: citation processing engine using CSL styles

**Tree-sitter grammars:**
- `tree-sitter-qmd`: tree-sitter grammar for qmd (a single unified grammar parsing both block structure and inline content)
- `tree-sitter-doctemplate`: tree-sitter grammar for document templates
- `quarto-treesitter-ast`: generic tree-sitter AST traversal utilities

**WASM:**
- `wasm-qmd-parser`: WASM module with entry points from `pampa` (see [crates/wasm-qmd-parser/CLAUDE.md](crates/wasm-qmd-parser/CLAUDE.md) for build instructions)

### `hub-client/` - Quarto Hub web client

A React/TypeScript web application for collaborative editing of Quarto projects. Uses Automerge for real-time sync and the WASM build of `wasm-qmd-parser` for live preview rendering.

**Key directories:**
- `src/components/` - React components (Editor, FileSidebar, tabs, etc.)
- `src/services/` - Services for Automerge sync, presence, storage
- `src/hooks/` - React hooks for presence, scroll sync, etc.

**Development:**

This project uses npm workspaces. Always run `npm install` from the **repo root**, not from hub-client:

```bash
# From repo root - install all workspace dependencies
npm install

# Run dev server (from hub-client directory)
cd hub-client
npm run dev        # Start dev server with HMR
npm run dev:fresh  # Clear cache and start fresh
npm run build      # Production build

# Local production mode (mirrors production setup)
npm run local-prod # Run hub + static server with proxy (requires built client)
```

**Important:** Never run `npm install` from hub-client directly - dependencies are hoisted to the root `node_modules/`.

**Local production mode:**

`npm run local-prod` runs a setup that mirrors the production deployment:
- Starts the `hub` binary (port 3001) with `--allow-insecure-auth` for local dev
- Serves the built hub-client (port 8080) with a Node.js proxy that forwards `/auth` and `/ws` to the hub
- Uses the same cache headers as production (`/assets/` immutable, `/` no-cache)

This helps catch reverse proxy configuration issues, WebSocket routing problems, and cache header behavior early. Before running:
1. Build the hub binary: `cargo build --bin hub` (or `--release`)
2. Build the client with local sync server: `cd hub-client && npm run build:local-prod`

Open `http://127.0.0.1:8080` in your browser. Data is stored in `.local-prod-data/` (gitignored). Ctrl-C shuts down both processes gracefully.

**Important:** Use `npm run build:local-prod` instead of `npm run build:all` for local-prod mode. This sets `VITE_DEFAULT_SYNC_SERVER=ws://127.0.0.1:8080/ws` so the client connects to your local hub instead of the public `wss://sync.automerge.org`.

**Local production with nginx:**

For testing the actual nginx configuration and headers:
```bash
# Prerequisites: local nginx installation (brew install nginx)
cd hub-client
npm run local-prod:nginx
```

This runs your local nginx with the hub binary on the host. Useful for validating nginx configs, gzip compression, security headers before deploying.

**Differences from production:**
- HTTP instead of HTTPS (no TLS)
- No OIDC authentication (uses `--allow-insecure-auth`)
- `local-prod`: Node.js proxy instead of nginx (faster, recommended for daily dev)
- `local-prod:nginx`: local nginx + hub on host (production has both but with TLS)
- Single-machine setup (production uses separate EC2 + EBS + S3)

## Architecture Notes

### VFS Path Conventions

All VFS file paths use the `/project/` prefix. When resolving file paths in WASM context, always account for this prefix. Never assume bare paths will work in the VFS layer.

### Crate Layout

- `pampa` is the core Quarto engine crate
- `quarto-core` handles higher-level orchestration
- `wasm-quarto-hub-client` is the WASM client (NOT wasm-qmd-parser)
- Always check `git diff` for uncommitted changes before starting work on a continuation session

### Document profile checkpoint

The render pipeline has a **profile checkpoint** between `MetadataMergeStage` and `PreEngineSugaringStage`: `DocumentProfileStage` extracts a typed, serializable `DocumentProfile` (title, outline, authors, etc.) into a `PipelineData::AtProfile` variant, and `UnwrapProfileStage` immediately hands the AST back to downstream stages. Project-scoped features (sidebars, cross-document links, incremental rebuilds, eventual `freeze`) are meant to consume this profile without re-running engines or user filters. Profiles are **read-only** — any feature that needs state not yet in the profile should move its producer earlier in the pipeline and add a field (with a `profile_version` bump), not back-patch. See `claude-notes/designs/document-profile-contract.md` for the full contract and `claude-notes/plans/2026-04-23-website-project-epic.md` for the epic.

### No DOM postprocessor — port Quarto 1 DOM postprocessors as AST transforms

Q2 emits HTML **directly from the Pandoc AST** and has **no post-Pandoc DOM-mutation stage**. Quarto 1, by contrast, leans heavily on DOM postprocessors (functions that parse the rendered HTML into a `Document` and mutate it — e.g. reveal's `applyStretch`, which hoists a stretched `<img>` to be a direct child of its `<section>`). **When porting such a feature, re-express it as an AST transform** (operate on `Block`/`Inline` so the writer emits the right HTML the first time) rather than adding a DOM postprocessor. A DOM-postprocessor stage is a large new architectural surface with no current consumer; do not introduce one without an extremely strong, explicitly-discussed reason. Worked example: revealjs auto-stretch unwraps a `Paragraph[Image]` into a `Plain[Image]` in `crates/quarto-core/src/revealjs/auto_stretch.rs` to get `section > img.r-stretch`, instead of mutating the DOM after the fact (bd-zkstclhl).

### Transform pipeline phases — format-agnostic semantics before format-specific presentation

The single transform pipeline (`build_transform_pipeline` in `crates/quarto-core/src/pipeline.rs`) runs every output format, splicing format-specific transforms in at chosen positions. It must visit transforms in **non-decreasing phase rank**: `Normalization` → `Crossref` → `Navigation` → `Finalization`. The hard rule: **a format-specific *presentation* transform that consumes semantic structure (a float, caption, number, or resolved `@ref`) must run in `Finalization`, after `crossref-render` — never earlier.** Each `AstTransform` declares its phase via `fn phase(&self) -> TransformPhase` (defaults to `Unclassified`; every pipeline member overrides it). The invariant is enforced by `test_build_transform_pipeline_phase_ordering`, which is format-neutral (loops over format strings, no `is_revealjs` branch). When adding a new format-specific late transform, add it via a named seam like `reveal_finalization_transforms`/`footer_render_stage`, not an inline `if is_revealjs { … }` at an arbitrary position. This is why a revealjs presentation transform (auto-stretch) once silently broke crossrefs by running before they were numbered (bd-w0c6d38k). Full contract + author rule + the preview-pipeline shape contract: `claude-notes/designs/transform-pipeline-phases.md`.

## hub-client Commit Instructions

**IMPORTANT**: When making commits that include changes to `hub-client/`, you MUST also update `hub-client/changelog.md`.

**Two-commit workflow** (required because the changelog entry needs the commit hash):
1. **First commit**: Make your hub-client changes and commit them
2. **Second commit**: Update `hub-client/changelog.md` with the hash from step 1

Entries are grouped by date under level-three headers. Add your entry under today's date header (create it if needed):
```
### YYYY-MM-DD

- [`<short-hash>`](https://github.com/quarto-dev/q2/commits/<short-hash>): One-sentence description
```

Example:
```
### 2026-01-10

- [`e6f742c`](https://github.com/quarto-dev/q2/commits/e6f742c): Refactor navigation to VS Code-style collapsible sidebar
```

The changelog is rendered in the About section of the hub-client UI.

## Testing instructions

- **CRITICAL**: Use `cargo nextest run` instead of `cargo test`.
- **CRITICAL**: Do NOT pipe `cargo nextest run` through `tail` or other commands - it causes hangs. Run it directly.
- **CRITICAL**: If you'll be writing tests, read the special instructions on file claude-notes/instructions/testing.md
- **CRITICAL**: For hub-client changes, passing tests alone is NOT sufficient. You must also verify that `npm run build:all` (from `hub-client/`) succeeds before claiming work is done. The production build (`tsc -b && vite build`) is stricter than `tsc --noEmit` and `vitest` — it uses project references mode and catches errors the other tools miss.
- **CRITICAL**: For any CLI- or user-visible feature, passing tests alone is NOT sufficient. See **End-to-end verification before declaring success** below.
- **Windows**: Some crates must be manually excluded from tests. See claude-notes/instructions/windows-dev.md for details.

## End-to-end verification before declaring success

Tests passing is **necessary but not sufficient** to declare a feature complete. Before reporting a feature done, you MUST:

1. **Exercise the feature end-to-end through the binary a real user would run.** For CLI features, that means `cargo run --bin q2 -- render <fixture>.qmd` (or the equivalent). For hub-client features, that means a real browser session against a running hub. In-process tests that call library functions directly do NOT count as end-to-end verification — they may bypass config branches, CLI argument parsing, file I/O, or pipeline builders that the real binary uses.
2. **Inspect the actual output.** Read the generated file, view the rendered HTML in a browser if UI is involved, grep for the expected markup. Do not infer success from the absence of errors.
3. **Record the end-to-end example in your communications.** Either in the session transcript (when reporting completion) or in the plan document for the feature, include:
   - the exact invocation used,
   - a snippet of the observed output demonstrating the feature,
   - an explicit note that the output was inspected.
4. **Prefer test helpers that drive the binary.** When adding tests for a CLI-visible feature, route through `render_document_to_file` (or the equivalent end-to-end entry point) with realistic config — not `render_qmd_to_html` with `HtmlRenderConfig::default()`. If the feature activates only under a specific config branch, make sure at least one regression test hits that branch.

If you cannot test a feature end-to-end (e.g. no access to a browser for a hub-client change), **say so explicitly** rather than claiming success based on unit tests alone. "Tests pass, I did not verify the real render path" is a valid and honest status update.

**Why this matters:** tests verify the contract the test author had in mind. Real invocations verify the contract the user is relying on. These are not the same thing.

Past incidents where they diverged:
- **2026-04-20**: `CodeHighlightStage` never ran under `quarto render` because the CLI path used a different branch of `render_qmd_to_html` than the tests. Every test passed; no rendered document had highlighting. See `claude-notes/plans/2026-04-19-syntax-highlighting-design.md` ("Phase 2 post-mortem") and the process-improvement plan at `claude-notes/plans/2026-04-20-end-to-end-verification-process.md`.
- **2026-05-20**: `q2 preview` silently served a stale render after Rust changes to `quarto-core`. `cargo build --bin q2` succeeded and the preview *ran*, but the iframe loaded a WASM image built before the changes — the embedded SPA's `wasm-quarto-hub-client_bg.wasm` is only refreshed when the WASM is rebuilt explicitly. See **Verifying Rust changes in `q2 preview`** below.

## Verifying Rust changes in `q2 preview`

`q2 preview` embeds the SPA bundle at `q2-preview-spa/dist/` into the
binary via `include_dir!`. The SPA loads the WASM at
`hub-client/wasm-quarto-hub-client/wasm_quarto_hub_client_bg.wasm`,
which is a build artifact of the `wasm-quarto-hub-client` crate (which
in turn depends on `quarto-core`, `pampa`, etc.).

A plain `cargo build --bin q2` does NOT rebuild the WASM, and the
preview will silently run pre-change code. Tests will all pass; the
render path will look correct; the preview iframe will not.

To pick up Rust changes in `q2 preview`, run the full chain:

```bash
cd hub-client && npm run build:wasm   # rebuild WASM from current Rust
cargo xtask build-q2-preview-spa      # bundle WASM into q2-preview-spa/dist/
cargo build --bin q2                  # re-embed dist/ via include_dir!
```

`cargo xtask verify` (without `--skip-hub-build`) runs steps 1 and 2
as part of the hub-build leg; after it finishes you still need step 3
manually if you want the next `q2 preview` invocation to be fresh.

For the deeper context (which crate produces which artifact, why the
chain doesn't auto-fire, how to diagnose stale-WASM symptoms) see
`claude-notes/instructions/preview-spa-rebuild.md`.

## Verifying TypeScript changes in `q2 mcp`

Same trap, different artifact: `q2 mcp` embeds the esbuild bundle at
`ts-packages/quarto-hub-mcp/dist-bundle/` via `include_dir!` and runs
it with ambient Node. A plain `cargo build --bin q2` re-embeds
whatever bundle was last built — after changing `quarto-hub-mcp`,
`quarto-sync-client`, or `quarto-automerge-schema`, run:

```bash
cargo xtask build-hub-mcp-bundle      # rebuild dist-bundle/ from TS sources
cargo build --bin q2                  # re-embed via include_dir!
```

`cargo xtask build-all` includes the bundle step (ordered before the
Rust build). Diagnose staleness with `q2 mcp --launcher-info`, which
prints the embedded bundle's git commit, dirty flag, and build time.
Fresh clones build fine without the bundle (a placeholder is embedded,
with a cargo warning); `q2 mcp` then fails at runtime pointing at the
xtask. Design: `claude-notes/plans/2026-06-11-q2-mcp-hub-auth.md`
(bd-81cfshmw).

## Build Commands

- WASM build: `npm run build:all` (NOT `cargo build --target wasm32-unknown-unknown`)
- Always verify WASM changes with the correct build command
- Fresh clone builds require dist/ directories to exist; run full build before testing

## Cutting a release

To ship a signed, multi-platform `q2` binary release (the `Release`
GitHub Actions workflow), follow the runbook:
**`claude-notes/instructions/release-runbook.md`**. It covers the
version-bump → tag → monitor → verify procedure and the non-obvious
gotchas (tag must equal `Cargo.toml` version; linux is gnu not musl;
signing happens in the release job; etc.). Do not improvise a release —
the preflight job and `--locked` builds are unforgiving.

## Full Project Verification

**IMPORTANT**: Before committing changes that affect `quarto-core`, `quarto-pandoc-types`, or other crates used by `wasm-quarto-hub-client`, run full verification:

```bash
cargo xtask verify           # Full verification (Rust + hub-client builds + tests)
```

This runs:
1. `cargo build --workspace` - Build all Rust crates
2. `cargo nextest run --workspace` - Run all Rust tests
3. `npm run build --if-present -w ts-packages/...` - Build the ts-packages
   workspaces, then smoke-check the quarto-hub-mcp server (`node
   dist/index.js --help` must survive ESM module resolution). These `dist/`
   outputs are consumed at runtime by Node consumers (the MCP server);
   hub-client bundles ts-packages from source, so nothing else builds them.
4. `cd hub-client && npm run build:all` - Build hub-client (includes WASM)
5. `cd hub-client && npm run test:ci` - Run hub-client tests

**Skip options** (for faster iteration):
```bash
cargo xtask verify --skip-rust-tests    # Skip Rust tests
cargo xtask verify --skip-hub-tests     # Skip hub-client tests
cargo xtask verify --skip-hub-build     # Skip hub-client build entirely
cargo xtask verify --e2e                # Include slower e2e browser tests
```

**Why this matters**: The `wasm-quarto-hub-client` crate depends on `quarto-core` types like `RenderOutput`. Changes to these types will break the WASM build even if `cargo build --workspace` succeeds (WASM uses a separate build target).

## Custom Lint Checks

Run project-specific lint checks with:

```bash
cargo xtask lint           # Run all lint checks
cargo xtask lint --verbose # Show all files being checked
cargo xtask lint --quiet   # Only show errors
```

### Current Lint Rules

- **external-sources-in-macro**: Detects references to `external-sources/` in compile-time macros like `include_dir!`, `include_str!`, `include_bytes!`. These break builds because `external-sources/` is not version-controlled.
- **add-file-with-id**: Restricts `SourceContext::add_file_with_id` to blessed modules (`config_sources.rs`, `metadata_merge.rs`, `span_assert.rs`, plus two temporarily-blessed files pending PR #478 / bd-x113wg9v). The API pairs an arbitrary FileId with arbitrary content; binding an *assumed* file to a diagnostic's resolved id renders byte offsets against the wrong text (bd-m6wmztln). Use `quarto_core::config_sources::bind_config_source` instead. Test code is skipped. Suppress a provably-safe use with `// lint:allow(add-file-with-id)` on the line or the line above, with a reason. Introduced with bd-jrq4hroi (audit: bd-nv4p0eb1).
- **metadata-as-str**: Detects `meta.get("key")…as_str()` reads of document metadata. A bare YAML string in front-matter context is stored as `ConfigValueKind::PandocInlines`, for which `ConfigValue::as_str()` returns `None` — silently dropping the option. Use `as_plain_text()` instead (handles both `Scalar(String)` and `PandocInlines`). Only flags chains whose `.get(<string literal>)` receiver is a metadata expression (final identifier `meta`/`metadata`); internal map reads and test code are skipped. Suppress a deliberate scalar-only read with a `// lint:allow(metadata-as-str)` comment on the line or the line above. Introduced with bd-y89ihf0i.

### Adding New Lint Rules

Add new rules in `crates/xtask/src/lint/`. Each rule should:
1. Implement a `check(path: &Path, content: &str) -> Result<Vec<Violation>>` function
2. Be called from `lint/mod.rs::check_file()`
3. Include unit tests

## Coding instructions

- **CRITICAL** If you'll be writing code, read the special instructions on file claude-notes/instructions/coding.md

## Debugging Approach

When diagnosing issues, do NOT jump to conclusions (e.g., 'race condition') before gathering evidence. Check the actual error path, inspect runtime values, and verify hypotheses with targeted tests before proposing fixes.

## Performance profiling

- **CRITICAL**: If you're investigating a performance hotspot (Chrome profile on hub-client, slow CLI run, suspicious Big-O), read `claude-notes/instructions/performance-profiling.md` before starting. It codifies the native-proxy-first workflow we use: build a representative fixture, scale it geometrically, add env-gated counters, confirm the complexity class empirically, *then* design a fix. Do not iterate on performance fixes in the browser.
- `QUARTO_PERF_STATS=1` is the shared env var for all perf-collection output in the tree. Individual gauges identify themselves with an output prefix like `perf.<gauge-name>` (e.g. `perf.intern` from the JSON writer's `SourceInfoSerializer` in `crates/pampa/src/writers/json.rs`, left in place after bd-h5l7 as a reference for the instrumentation pattern).

## Claude Code hooks

This repository has Claude Code hooks configured in `.claude/settings.json`.

**Post-tool-use hook**: Automatically runs `cargo fmt` on any Rust file after it's edited or written.

**Required tools** (must be installed on the system):
- `jq` - for parsing JSON input in hook scripts
- `rustfmt` - for formatting Rust code (usually installed via `rustup component add rustfmt`)

## General Instructions

- in Claude Code conversations, "Rust Quarto" means this project, and "TypeScript Quarto" or "TS Quarto" means the current version of Quarto in the quarto-dev/quarto-cli repository.
- in this repository, "qmd" means "quarto markdown", the dialect of markdown we are developing. Although we aim to be largely compatible with Pandoc, discrepancies in the behavior might not be bugs.
- the qmd format only supports the inline syntax for a link [link](./target.html), and not the reference-style syntax [link][1].
- When fixing bugs, always try to isolate and fix one bug at a time.
- If you need to fix parser bugs, you will find use in running the application with "-v", which will provide a large amount of information from the tree-sitter parsing process, including a print of the concrete syntax tree out to stderr.
- use "cargo run --" instead of trying to find the binary location, which will often be outside of this crate.
- when calling shell scripts, ALWAYS BE MINDFUL of the current directory you're operating in. use `pwd` as necessary to avoid confusing yourself over commands that use relative paths.
- When a cd command fails for you, that means you're confused about the current directory. In this situations, ALWAYS run `pwd` before doing anything else.
- use `jq` instead of `python3 -m json.tool` for pretty-printing. When processing JSON in a shell pipeline, prefer `jq` when possible.
- Always create a plan. Always work on the plan one item at a time.
- The qmd grammar is unified: there is a single grammar directory, `crates/tree-sitter-qmd/tree-sitter-markdown` (there is no longer a separate `tree-sitter-markdown-inline` directory). In that directory you rebuild the parser using "tree-sitter generate; tree-sitter build". Make sure the shell is in the correct directory before running those. Every time you change the tree-sitter parser, rebuild it and run "tree-sitter test". If the tests fail, fix the code. Only change tree-sitter tests you've just added; do not touch any other tests. If you end up getting stuck there, stop and ask for my help.
- When attempting to find binary differences between files, always use `xxd` instead of other tools.
- .c only works in JSON formats. Inside Lua filters, you need to use Pandoc's Lua API. Study https://raw.githubusercontent.com/jgm/pandoc/refs/heads/main/doc/lua-filters.md and make notes to yourself as necessary (use claude-notes in this directory)
- Sometimes you get confused by macOS's using many different /private/tmp directories linked to /tmp. Prefer to use temporary directories local to the project you're working on (which you can later clean)
- When using `echo` on Bash, be careful about escaping. `!` requires you to use single quotes. BAD, DO NOT USE: echo "![](hello)". GOOD, DO USE: '![](hello)'.
- The documentation in docs/ is a user-facing Quarto website. There, you should document usage and not technical details.
- **The docs/ website is rendered with Quarto 2, NOT Quarto 1.** Always use `cargo run --bin q2 -- render docs/` (or `cargo run --bin q2 -- preview docs/`), never `quarto render` / `quarto preview`. The user's system `quarto` binary may be symlinked to a quarto-cli dev checkout and is *not* what builds this site. Verifying changes with Q1 produces misleading results: Q1 may reject Q2-specific YAML schema entries that Q2 accepts, and vice versa.
- **CRITICALLY IMPORTANT**. IF YOU EVER FIND YOURSELF WANTING TO WRITE A HACKY SOLUTION (OR A "TODO" THAT UNDOES EXISTING WORK), STOP AND ASK THE USER. THAT MEANS YOUR PLAN IS NOT GOOD ENOUGH

## External Sources Policy

**NEVER reference `external-sources/` directly in compiled code, build scripts, or embedded resources.**

The `external-sources/` directory contains reference implementations (like `quarto-cli`) that are useful for:
- Understanding how features work in TypeScript Quarto
- Copying resources that need to be maintained locally
- Analysis and documentation (claude-notes/)

However, any resources that are needed at compile time or runtime **must be copied to a local directory** within the repository. This ensures:
1. **Build reproducibility**: Builds work without `external-sources/` being checked out
2. **Version control**: Changes to resources are tracked in the repository
3. **CI/CD compatibility**: CI builds don't need to check out quarto-cli

### Current Local Resource Directories

- `resources/scss/` - SCSS resources (Bootstrap, themes, templates) - see `resources/scss/README.md`
- `resources/` (future) - Other resources as needed

### Updating Local Resources

When TypeScript Quarto updates resources (e.g., Bootstrap version bump):
1. Copy updated files from `external-sources/` to the appropriate local directory
2. Update any related documentation (README.md files)
3. Run relevant tests to verify compatibility
4. Commit the updated resources

### Acceptable Uses of external-sources/

- Reading files for analysis or understanding
- Referencing in documentation and claude-notes/
- One-time copying of files to local directories
- Running TypeScript Quarto for comparison testing

### Prohibited Uses of external-sources/

- `include_dir!()` or similar macros pointing to external-sources/
- Build scripts that read from external-sources/
- Test fixtures that depend on external-sources/ (copy them locally)
- Runtime file paths referencing external-sources/
