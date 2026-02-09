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
4. **For changes to quarto-core or quarto-pandoc-types**: Run `cargo xtask verify` to ensure hub-client/WASM builds work
5. Ask the user for permission before pushing
6. Only push after receiving explicit approval

This applies even at the end of sessions. Prepare the commit but wait for approval to push.

## Git Workflow

When asked to 'stage and commit everything' or 'commit all changes', stage ALL modified/untracked files (`git add -A`), not just the files Claude edited in the current session.

### Snapshot Test Changes

When a commit includes updated or new snapshot files (`.snap` files under `snapshots/`), **always explicitly document these changes** in the commit message and in conversation with the user. Snapshot changes can hide unwanted regressions. Specifically:

1. **Report the count** of snapshot files added/modified/removed.
2. **Summarize what changed** — e.g., "45 HTML comment snapshots updated: comments now appear as `RawInline` instead of being dropped."
3. **Call out any surprising changes** — if a snapshot changed in a way that wasn't obviously expected from the code change, flag it for review.
4. After committing, **list the affected snapshot files** so the user can review the diffs before pushing.

## **WORK TRACKING**

We use br (beads_rust) for issue tracking instead of Markdown TODOs or external tools.

**Note:** `br` is non-invasive and never executes git commands. After `br sync --flush-only`, you must manually run `git add .beads/ && git commit`.
We use plans for additional context and bookkeeping. Write plans to `claude-notes/plans/YYYY-MM-DD-<description>.md`, and reference the plan file in the issues.

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

### Beads Quick Reference

```bash
# Find ready work (no blockers)
br ready --json

# Create new issue
br create "Issue title" -t bug|feature|task -p 0-4 -d "Description" --json

# Create with explicit ID (for parallel workers)
br create "Issue title" --id worker1-100 -p 1 --json

# Create with labels
br create "Issue title" -t bug -p 1 -l bug,critical --json

# Create multiple issues from markdown file
br create -f feature-plan.md --json

# Update issue status
br update <id> --status in_progress --json

# Link discovered work (old way)
br dep add <discovered-id> <parent-id> --type discovered-from

# Create and link in one command (new way)
br create "Issue title" -t bug -p 1 --deps discovered-from:<parent-id> --json

# Complete work
br close <id> --reason "Done" --json

# Show dependency tree
br dep tree <id>

# Get issue details
br show <id> --json

# Import with collision detection
br import -i .beads/issues.jsonl --dry-run             # Preview only
br import -i .beads/issues.jsonl --resolve-collisions  # Auto-resolve
```

### Workflow

1. **Check for ready work**: Run `br ready` to see what's unblocked
2. **Claim your task**: `br update <id> --status in_progress`
3. **Work on it**: Implement, test, document
4. **Discover new work**: If you find bugs or TODOs, create issues:
   - Old way (two commands): `br create "Found bug in auth" -t bug -p 1 --json` then `br dep add <new-id> <current-id> --type discovered-from`
   - New way (one command): `br create "Found bug in auth" -t bug -p 1 --deps discovered-from:<current-id> --json`
5. **Complete**: `br close <id> --reason "Implemented"`
6. **Sync and commit**:
   ```bash
   br sync --flush-only
   git add .beads/
   git commit -m "sync beads"
   ```

### Issue Types

- `bug` - Something broken that needs fixing
- `feature` - New functionality
- `task` - Work item (tests, docs, refactoring)
- `epic` - Large feature composed of multiple issues
- `chore` - Maintenance work (dependencies, tooling)

### Priorities

- `0` - Critical (security, data loss, broken builds)
- `1` - High (major features, important bugs)
- `2` - Medium (nice-to-have features, minor bugs)
- `3` - Low (polish, optimization)
- `4` - Backlog (future ideas)

### Dependency Types

- `blocks` - Hard dependency (issue X blocks issue Y)
- `related` - Soft relationship (issues are connected)
- `parent-child` - Epic/subtask relationship
- `discovered-from` - Track issues discovered during work

Only `blocks` dependencies affect the ready work queue.

## **CRITICAL - TEST-DRIVEN DEVELOPMENT**

When fixing ANY bug:
1. **FIRST**: Write the test
2. **SECOND**: Run the test and verify it fails as expected
3. **THIRD**: Implement the fix
4. **FOURTH**: Run the test and verify it passes
5. **FIFTH**: Run the full workspace test suite (`cargo nextest run --workspace`) to verify no regressions in other crates

**Step 5 is critical because this is a monorepo — changes in one crate (e.g. `pampa`) can break downstream crates (e.g. `qmd-syntax-helper`) that depend on it. Running only the modified crate's tests is NOT sufficient.**

**This is non-negotiable. Never implement a fix before verifying the test fails. Stop and ask the user if you cannot think of a way to mechanically test the bad behavior. Only deviate if writing new features.**

**Do NOT close a beads test suite item unless all tests pass. If you feel you're low on tokens, report that and open subtasks to work on new sessions.**

## Workspace structure

### `crates/` - all Rust crates in the workspace

**Binaries:**
- `quarto`: main entry point for the `quarto` command line binary (includes `quarto hub` subcommand)
- `hub`: collaborative editing server for Quarto projects (also available as `quarto hub`)
- `pampa`: parse qmd text and produce Pandoc AST and other formats
- `qmd-syntax-helper`: help users convert qmd files to the new syntax
- `validate-yaml`: exercise `quarto-yaml-validation`

**Core libraries:**
- `quarto-core`: core rendering infrastructure for Quarto
- `quarto-util`: shared utilities for Quarto crates
- `quarto-error-reporting`: uniform, helpful, beautiful error messages
- `quarto-source-map`: maintain source location information for data structures

**Parsing libraries:**
- `quarto-yaml`: YAML parser with accurate fine-grained source locations
- `quarto-yaml-validation`: validate YAML objects using schemas
- `quarto-xml`: source-tracked XML parsing
- `quarto-parse-errors`: parse error infrastructure

**Pandoc/document processing:**
- `quarto-pandoc-types`: Pandoc AST type definitions
- `quarto-doctemplate`: Pandoc-compatible document template engine
- `quarto-csl`: CSL (Citation Style Language) parsing with source tracking
- `quarto-citeproc`: citation processing engine using CSL styles

**Tree-sitter grammars:**
- `tree-sitter-qmd`: tree-sitter grammars for block and inline parsers
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
```

**Important:** Never run `npm install` from hub-client directly - dependencies are hoisted to the root `node_modules/`.

## Architecture Notes

### VFS Path Conventions

All VFS file paths use the `/project/` prefix. When resolving file paths in WASM context, always account for this prefix. Never assume bare paths will work in the VFS layer.

### Crate Layout

- `pampa` is the core Quarto engine crate
- `quarto-core` handles higher-level orchestration
- `wasm-quarto-hub-client` is the WASM client (NOT wasm-qmd-parser)
- Always check `git diff` for uncommitted changes before starting work on a continuation session

## hub-client Commit Instructions

**IMPORTANT**: When making commits that include changes to `hub-client/`, you MUST also update `hub-client/changelog.md`.

**Two-commit workflow** (required because the changelog entry needs the commit hash):
1. **First commit**: Make your hub-client changes and commit them
2. **Second commit**: Update `hub-client/changelog.md` with the hash from step 1

Entries are grouped by date under level-three headers. Add your entry under today's date header (create it if needed):
```
### YYYY-MM-DD

- [`<short-hash>`](https://github.com/quarto-dev/kyoto/commits/<short-hash>): One-sentence description
```

Example:
```
### 2026-01-10

- [`e6f742c`](https://github.com/quarto-dev/kyoto/commits/e6f742c): Refactor navigation to VS Code-style collapsible sidebar
```

The changelog is rendered in the About section of the hub-client UI.

## Testing instructions

- **CRITICAL**: Use `cargo nextest run` instead of `cargo test`.
- **CRITICAL**: Do NOT pipe `cargo nextest run` through `tail` or other commands - it causes hangs. Run it directly.
- **CRITICAL**: If you'll be writing tests, read the special instructions on file claude-notes/instructions/testing.md

## Build Commands

- WASM build: `npm run build:all` (NOT `cargo build --target wasm32-unknown-unknown`)
- Always verify WASM changes with the correct build command
- Fresh clone builds require dist/ directories to exist; run full build before testing

## Full Project Verification

**IMPORTANT**: Before committing changes that affect `quarto-core`, `quarto-pandoc-types`, or other crates used by `wasm-quarto-hub-client`, run full verification:

```bash
cargo xtask verify           # Full verification (Rust + hub-client builds + tests)
```

This runs:
1. `cargo build --workspace` - Build all Rust crates
2. `cargo nextest run --workspace` - Run all Rust tests
3. `cd hub-client && npm run build:all` - Build hub-client (includes WASM)
4. `cd hub-client && npm run test:ci` - Run hub-client tests

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

### Adding New Lint Rules

Add new rules in `crates/xtask/src/lint/`. Each rule should:
1. Implement a `check(path: &Path, content: &str) -> Result<Vec<Violation>>` function
2. Be called from `lint/mod.rs::check_file()`
3. Include unit tests

## Coding instructions

- **CRITICAL** If you'll be writing code, read the special instructions on file claude-notes/instructions/coding.md

## Debugging Approach

When diagnosing issues, do NOT jump to conclusions (e.g., 'race condition') before gathering evidence. Check the actual error path, inspect runtime values, and verify hypotheses with targeted tests before proposing fixes.

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
- In the tree-sitter-markdown and tree-sitter-markdown-inline directories, you rebuild the parsers using "tree-sitter generate; tree-sitter build". Make sure the shell is in the correct directory before running those. Every time you change the tree-sitter parsers, rebuild them and run "tree-sitter test". If the tests fail, fix the code. Only change tree-sitter tests you've just added; do not touch any other tests. If you end up getting stuck there, stop and ask for my help.
- When attempting to find binary differences between files, always use `xxd` instead of other tools.
- .c only works in JSON formats. Inside Lua filters, you need to use Pandoc's Lua API. Study https://raw.githubusercontent.com/jgm/pandoc/refs/heads/main/doc/lua-filters.md and make notes to yourself as necessary (use claude-notes in this directory)
- Sometimes you get confused by macOS's using many different /private/tmp directories linked to /tmp. Prefer to use temporary directories local to the project you're working on (which you can later clean)
- When using `echo` on Bash, be careful about escaping. `!` requires you to use single quotes. BAD, DO NOT USE: echo "![](hello)". GOOD, DO USE: '![](hello)'.
- The documentation in docs/ is a user-facing Quarto website. There, you should document usage and not technical details.
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
