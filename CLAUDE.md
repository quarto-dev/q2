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

## **GIT PUSH POLICY**

**NEVER push to the remote repository without explicit user permission.** Always:
1. Stage and commit changes as needed
2. **Verify the full workspace compiles cleanly** (`cargo build --workspace`)
3. **Verify the full workspace tests pass** (`cargo nextest run --workspace`)
4. Ask the user for permission before pushing
5. Only push after receiving explicit approval

This applies even at the end of sessions. Prepare the commit but wait for approval to push.

## **WORK TRACKING**

We use bd (beads) for issue tracking instead of Markdown TODOs or external tools.
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
- [ ] Create shared base class in @databot/node
- [ ] Update StandalonePlatform to use shared base
- [ ] Update RStudioPlatform to use shared base
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
bd ready --json

# Create new issue
bd create "Issue title" -t bug|feature|task -p 0-4 -d "Description" --json

# Create with explicit ID (for parallel workers)
bd create "Issue title" --id worker1-100 -p 1 --json

# Create with labels
bd create "Issue title" -t bug -p 1 -l bug,critical --json

# Create multiple issues from markdown file
bd create -f feature-plan.md --json

# Update issue status
bd update <id> --status in_progress --json

# Link discovered work (old way)
bd dep add <discovered-id> <parent-id> --type discovered-from

# Create and link in one command (new way)
bd create "Issue title" -t bug -p 1 --deps discovered-from:<parent-id> --json

# Complete work
bd close <id> --reason "Done" --json

# Show dependency tree
bd dep tree <id>

# Get issue details
bd show <id> --json

# Import with collision detection
bd import -i .beads/issues.jsonl --dry-run             # Preview only
bd import -i .beads/issues.jsonl --resolve-collisions  # Auto-resolve
```

### Known Issues

- **Prefix mismatch error on `bd sync`**: You may see an error like `Import failed: prefix mismatch detected: database uses 'kyoto-' but found issues with prefixes: [k- (636 issues)]`. This is a known beads bug - ignore it. The local operations (create, close, update) still work correctly.

### Workflow

1. **Check for ready work**: Run `bd ready` to see what's unblocked
2. **Claim your task**: `bd update <id> --status in_progress`
3. **Work on it**: Implement, test, document
4. **Discover new work**: If you find bugs or TODOs, create issues:
   - Old way (two commands): `bd create "Found bug in auth" -t bug -p 1 --json` then `bd dep add <new-id> <current-id> --type discovered-from`
   - New way (one command): `bd create "Found bug in auth" -t bug -p 1 --deps discovered-from:<current-id> --json`
5. **Complete**: `bd close <id> --reason "Implemented"`
6. **Export**: Changes auto-sync to `.beads/issues.jsonl` (5-second debounce)

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

**This is non-negotiable. Never implement a fix before verifying the test fails. Stop and ask the user if you cannot think of a way to mechanically test the bad behavior. Only deviate if writing new features.**

**Do NOT close a beads test suite item unless all tests pass. If you feel you're low on tokens, report that and open subtasks to work on new sessions.**

## Workspace structure

### `crates/` - all Rust crates in the workspace

**Binaries:**
- `quarto`: main entry point for the `quarto` command line binary
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

## Coding instructions

- **CRITICAL** If you'll be writing code, read the special instructions on file claude-notes/instructions/coding.md

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
