# Quarto Rust monorepo

## **WORK TRACKING**

We use bd (beads) for issue tracking instead of Markdown TODOs or external tools.
We use plans for additional context and bookkeeping. Write plans to `claude-notes/plans/YYYY-MM-DD-<description>.md`, and reference the plan file in the issues.

### Quick Reference

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

# Label management
bd label add <id> <label> --json
bd label remove <id> <label> --json
bd label list <id> --json
bd label list-all --json

# Filter issues by label
bd list --label bug,critical --json

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

## Workspace structure

### `crates` - corresponds to the crates in the public quarto-markdown repo

- `crates/qmd-syntax-helper`: a binary to help users convert qmd files to the new syntax
- `crates/quarto-error-reporting`: a library to help create uniform, helpful, beautiful error messages
- `crates/quarto-markdown-pandoc`: a binary to parse qmd text and produce Pandoc AST and other formats
- `crates/quarto-source-map`: a library to help maintain information about the source location of data structures in text files
- `crates/quarto-yaml`: a YAML parser that produces YAML objects and accurate fine-grained source location of elements
- `crates/tree-sitter-qmd`: tree-sitter grammars for block and inline parsers
- `crates/wasm-qmd-parser`: A WASM module with some entry points from `crates/quarto-markdown-pandoc`

## General Instructions

- in this repository, "qmd" means "quarto markdown", the dialect of markdown we are developing. Although we aim to be largely compatible with Pandoc, discrepancies in the behavior might not be bugs.
- the qmd format only supports the inline syntax for a link [link](./target.html), and not the reference-style syntax [link][1].
- Always strive for test documents as small as possible. Prefer a large number of small test documents instead of small number of large documents.
- When fixing bugs, always try to isolate and fix one bug at a time.
- **CRITICAL - TEST FIRST**: When fixing bugs using tests, you MUST run the failing test BEFORE implementing any fix. This is non-negotiable. Verify the test fails in the expected way, then implement the fix, then verify the test passes.
- If you need to fix parser bugs, you will find use in running the application with "-v", which will provide a large amount of information from the tree-sitter parsing process, including a print of the concrete syntax tree out to stderr.
- use "cargo run --" instead of trying to find the binary location, which will often be outside of this crate.
- when calling shell scripts, ALWAYS BE MINDFUL of the current directory you're operating in. use `pwd` as necessary to avoid confusing yourself over commands that use relative paths.
- When a cd command fails for you, that means you're confused about the current directory. In this situations, ALWAYS run `pwd` before doing anything else.
- use `jq` instead of `python3 -m json.tool` for pretty-printing. When processing JSON in a shell pipeline, prefer `jq` when possible.
- Always create a plan. Always work on the plan one item at a time.
- In the tree-sitter-markdown and tree-sitter-markdown-inline directories, you rebuild the parsers using "tree-sitter generate; tree-sitter build". Make sure the shell is in the correct directory before running those. Every time you change the tree-sitter parsers, rebuild them and run "tree-sitter test". If the tests fail, fix the code. Only change tree-sitter tests you've just added; do not touch any other tests. If you end up getting stuck there, stop and ask for my help.
- When attempting to find binary differences between files, always use `xxd` instead of other tools.
- .c only works in JSON formats. Inside Lua filters, you need to use Pandoc's Lua API. Study https://raw.githubusercontent.com/jgm/pandoc/refs/heads/main/doc/lua-filters.md and make notes to yourself as necessary (use claude-notes in this directory)
- Sometimes you get confused by macOS's weird renaming of /tmp. Prefer to use temporary directories local to the project you're working on (which you can later clean)
- The documentation in docs/ is a user-facing Quarto website. There, you should document usage and not technical details.