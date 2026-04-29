# Tree-sitter Corpus CRLF Parity Check — Design

## Context

PR #139 (bd-0gsj) fixes a CRLF bug in `crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c` where pipe tables absorbed following paragraphs when input used `\r\n` line endings. The fix is one character (`!=` → `==`) plus a Rust regression test (`pipe_table_crlf_matches_lf`) that derives CRLF in-process and asserts the parsed S-expression matches LF.

Lead dev cscheid asked on the PR:

> We have dedicated tree-sitter tests too, but since this is whitespace related, I imagine that we would need a new strategy for those. What do you think about something like a script that converts all unix line breaks to windows linebreaks in the tree-sitter test files and then runs again? I think I would like the parses and errors to stay exactly the same. The source locations would change, but the tree-sitter test suite isn't checking source locations.

This design wires that strategy into the build.

## Goal

After `tree-sitter test` passes against the LF corpus, re-run the same suite against a CRLF-converted copy. Identical parses, identical errors. Source locations differ, but `tree-sitter test` does not compare them — corpus expectations are bare S-expressions.

This locks in the bug-fix and prevents any future grammar/scanner change from silently regressing CRLF behavior on Linux CI (where corpus files are checked out as LF).

## Non-goals

- Adding a Windows CI runner. Orthogonal workstream.
- Removing `eol=lf` from `crates/tree-sitter-doctemplate/grammar/.gitattributes`. Deferred to the Windows CI workstream.
- Covering the doctemplate grammar's corpus. Its scanner has no `\r` handling because newlines are not scanner-relevant in that grammar; CRLF risk is negligible.

## Mechanism

A new step in `cargo xtask verify`, sequenced after step 4 ("tree-sitter test"). Call it step 4b: "tree-sitter test (CRLF parity)".

1. Create a temp directory.
2. Mirror the grammar tree at `crates/tree-sitter-qmd/tree-sitter-markdown/` into the temp dir such that `tree-sitter test` invoked from the temp grammar dir picks up a CRLF copy of `test/corpus/`. Two viable mechanics:
   - **(a) Symlink/junction the grammar dir, then physically copy `test/corpus/` with conversion.** Lightweight. Windows: junction (`mklink /J`) works for directory links without admin privileges.
   - **(b) Full copy of the grammar dir.** Heavier (includes generated `src/parser.c`, bindings) but mechanically simpler.

   Implementation plan picks one based on what works cleanly cross-platform.
3. For each `*.txt` file in `test/corpus/`, normalize line endings to LF first (idempotency: handle files already CRLF), then replace LF with CRLF. The whole file is converted, including the `===`/`---` separators and the expected S-expression block — `tree-sitter test`'s parser of the corpus format is line-oriented and tolerates either ending.
4. Invoke `tree-sitter test` with the temp grammar dir as cwd.
5. Surface the same exit code as step 4. Skippable via a new `--skip-treesitter-crlf-tests` flag, mirroring the existing `--skip-treesitter-tests`.

## Failure modes and policy

Most likely outcome: all tests pass. The bug-fix in `scanner.c` was the only known CRLF-sensitive site; an audit during the fix found `\r` paired with `\n` everywhere else.

If a corpus test fails under CRLF:

- **Real grammar bug** — fix it, same as the pipe-table fix. Add a Rust regression test alongside `pipe_table_crlf_matches_lf` if the failing test isn't already covered by one.
- **Test legitimately depends on bare-LF semantics** — not expected for any markdown construct, but possible. Last resort: exclude that single corpus test from the CRLF run and document why in this file. Decision deferred until a concrete case appears.

## Why a script and not a Rust test

Considered a parametric Rust test in `crates/tree-sitter-qmd/bindings/rust/lib.rs` that walks corpus files, splits by `===`/`---`, and asserts in-process. Rejected because:

- The `tree-sitter` CLI is already required for step 4 of `cargo xtask verify` and CI, so the Rust path saves no dependency.
- Reusing `tree-sitter test` keeps error formatting, language-attribute handling, and any future corpus-format evolution in sync automatically — no custom corpus parser to maintain.

The existing `pipe_table_crlf_matches_lf` Rust unit test stays as fast in-process regression coverage independent of the CLI. The two layers complement each other: Rust unit test for the specific bug, xtask step for full corpus parity.

## Scope

- `cargo xtask verify` gains step 4b and a `--skip-treesitter-crlf-tests` flag.
- No grammar changes, no corpus changes, no CI workflow changes.
- Doctemplate grammar untouched.

---

## Implementation steps

**Goal.** Add a CRLF-parity sub-step inside step 4 of `cargo xtask verify`, gated by a new `--skip-treesitter-crlf-tests` flag.

**Files touched.**
- Create: `crates/xtask/src/treesitter_crlf.rs`
- Modify: `crates/xtask/src/main.rs` (add module, add CLI flag, thread it into config)
- Modify: `crates/xtask/src/verify.rs` (add config field, invoke after LF tests pass)

### Task 1: Create the CRLF conversion helper with unit tests

**Files:** Create `crates/xtask/src/treesitter_crlf.rs`.

- [ ] **Step 1.1: Write the helper file with the LF→CRLF function and unit tests (test-first; the tests describe the contract).**

```rust
//! Tree-sitter CRLF parity check.
//!
//! Re-runs `tree-sitter test` against a copy of the corpus where every
//! line ending has been converted to CRLF. Locks in the scanner-level
//! CRLF handling so future grammar changes cannot silently regress it
//! on Linux CI (where corpus files are checked out as LF).

use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Command;

/// Convert all line endings in `input` to CRLF. Idempotent: input that
/// already has CRLF endings is unchanged. Lone `\r` characters (rare,
/// classic-Mac) are left alone.
pub(crate) fn to_crlf(input: &str) -> String {
    // Normalize first: drop any existing \r before \n so the second
    // pass produces exactly one \r per \n.
    let normalized: String = input.replace("\r\n", "\n");
    normalized.replace('\n', "\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_string_unchanged() {
        assert_eq!(to_crlf(""), "");
    }

    #[test]
    fn lf_becomes_crlf() {
        assert_eq!(to_crlf("a\nb\n"), "a\r\nb\r\n");
    }

    #[test]
    fn crlf_input_is_idempotent() {
        assert_eq!(to_crlf("a\r\nb\r\n"), "a\r\nb\r\n");
    }

    #[test]
    fn mixed_input_normalizes_to_crlf() {
        assert_eq!(to_crlf("a\r\nb\nc\r\n"), "a\r\nb\r\nc\r\n");
    }

    #[test]
    fn no_newlines_unchanged() {
        assert_eq!(to_crlf("abc"), "abc");
    }
}
```

- [ ] **Step 1.2: Run the unit tests to verify they pass.**

Run: `cargo nextest run -p xtask treesitter_crlf`
Expected: 5 tests pass.

- [ ] **Step 1.3: Commit.**

```bash
git add crates/xtask/src/treesitter_crlf.rs
git commit -m "Add CRLF conversion helper for tree-sitter parity check"
```

### Task 2: Add the parity-check runner

**Files:** Modify `crates/xtask/src/treesitter_crlf.rs`.

- [ ] **Step 2.1: Append the runner function. It copies the grammar dir to a tempdir, converts every `test/corpus/**/*.txt` to CRLF in the copy, then runs `tree-sitter test` from the tempdir.**

```rust
/// Run `tree-sitter test` against a CRLF-converted copy of the grammar's
/// corpus. The grammar source files are copied unchanged; only files
/// matching `test/corpus/**/*.txt` are transformed.
pub(crate) fn run_parity_check(grammar_dir: &Path) -> Result<()> {
    let tempdir = tempfile::tempdir()
        .context("Failed to create tempdir for tree-sitter CRLF parity check")?;
    let dest = tempdir.path();

    copy_dir_recursive(grammar_dir, dest, &|relative| {
        // Skip transient build output that may exist in the source tree.
        !relative.starts_with("target") && !relative.starts_with("node_modules")
    })?;

    convert_corpus_to_crlf(&dest.join("test").join("corpus"))?;

    let status = Command::new("tree-sitter")
        .arg("test")
        .current_dir(dest)
        .status()
        .context("Failed to invoke `tree-sitter test` for CRLF parity run")?;

    if !status.success() {
        bail!("Tree-sitter CRLF parity tests failed");
    }
    Ok(())
}

fn copy_dir_recursive(
    src: &Path,
    dst: &Path,
    keep: &dyn Fn(&Path) -> bool,
) -> Result<()> {
    std::fs::create_dir_all(dst)
        .with_context(|| format!("create_dir_all {}", dst.display()))?;
    for entry in std::fs::read_dir(src)
        .with_context(|| format!("read_dir {}", src.display()))?
    {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let name = entry.file_name();
        let from = entry.path();
        let to = dst.join(&name);
        let relative = Path::new(&name);
        if !keep(relative) {
            continue;
        }
        if file_type.is_dir() {
            copy_dir_recursive(&from, &to, &|sub| {
                let mut joined = std::path::PathBuf::from(relative);
                joined.push(sub);
                keep(&joined)
            })?;
        } else if file_type.is_file() {
            std::fs::copy(&from, &to)
                .with_context(|| format!("copy {} -> {}", from.display(), to.display()))?;
        }
    }
    Ok(())
}

fn convert_corpus_to_crlf(corpus_dir: &Path) -> Result<()> {
    if !corpus_dir.is_dir() {
        bail!(
            "Expected corpus directory at {} after copy",
            corpus_dir.display()
        );
    }
    for entry in walkdir::WalkDir::new(corpus_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        if entry.path().extension().and_then(|s| s.to_str()) != Some("txt") {
            continue;
        }
        let contents = std::fs::read_to_string(entry.path())
            .with_context(|| format!("read {}", entry.path().display()))?;
        let crlf = to_crlf(&contents);
        std::fs::write(entry.path(), crlf)
            .with_context(|| format!("write {}", entry.path().display()))?;
    }
    Ok(())
}
```

- [ ] **Step 2.2: Verify `tempfile` and `walkdir` are available to xtask. Inspect `crates/xtask/Cargo.toml`. If either is missing, add it under `[dependencies]` matching the workspace version.**

Run: `cargo build -p xtask`
Expected: success.

- [ ] **Step 2.3: Commit.**

```bash
git add crates/xtask/src/treesitter_crlf.rs crates/xtask/Cargo.toml
git commit -m "Add tree-sitter CRLF parity runner"
```

### Task 3: Wire the parity check into `cargo xtask verify`

**Files:** Modify `crates/xtask/src/verify.rs`, `crates/xtask/src/main.rs`.

- [ ] **Step 3.1: In `crates/xtask/src/main.rs`, register the new module and add the CLI flag. Add `mod treesitter_crlf;` near the other `mod` declarations. In the `Verify` variant of `Command`, add the flag immediately after `skip_treesitter_tests`:**

```rust
        /// Skip the CRLF parity run of tree-sitter grammar tests.
        #[arg(long)]
        skip_treesitter_crlf_tests: bool,
```

In the doc comment block on `Verify`, append a line after step 4 noting the parity sub-run.

In the `match cli.command` arm for `Verify`, destructure `skip_treesitter_crlf_tests` and pass it through to `VerifyConfig`.

- [ ] **Step 3.2: In `crates/xtask/src/verify.rs`, add the config field and wire the call. Add `pub skip_treesitter_crlf_tests: bool,` to `VerifyConfig` and `false` to its `Default`. Inside the existing step 4 block (the one that runs `tree-sitter test`), after the success print line, add:**

```rust
        if !config.skip_treesitter_crlf_tests {
            println!("\n  ↳ Re-running with CRLF line endings...");
            crate::treesitter_crlf::run_parity_check(&ts_dir)
                .context("Tree-sitter CRLF parity check failed")?;
            println!("  ✓ CRLF parity check complete");
        } else {
            println!("\n  ↳ Skipping CRLF parity check");
        }
```

Add `use anyhow::Context;` if it isn't already imported in scope (the file already imports `Context`).

- [ ] **Step 3.3: Build to confirm everything compiles.**

Run: `cargo build -p xtask`
Expected: success.

- [ ] **Step 3.4: Commit.**

```bash
git add crates/xtask/src/main.rs crates/xtask/src/verify.rs
git commit -m "Wire CRLF parity check into cargo xtask verify"
```

### Task 4: End-to-end verification

- [ ] **Step 4.1: Run the verify step in isolation to confirm CRLF parity passes against the fixed grammar.**

Run: `cargo xtask verify --skip-rust-build --skip-rust-tests --skip-hub-build --skip-hub-tests --skip-trace-viewer-build --skip-trace-viewer-tests`
Expected: step 4 completes, including the "↳ Re-running with CRLF line endings..." line, exits 0.

- [ ] **Step 4.2: Confirm the new flag actually skips the sub-run.**

Run: `cargo xtask verify --skip-treesitter-crlf-tests --skip-rust-build --skip-rust-tests --skip-hub-build --skip-hub-tests --skip-trace-viewer-build --skip-trace-viewer-tests`
Expected: step 4 prints "↳ Skipping CRLF parity check"; exits 0.

- [ ] **Step 4.3: Sanity-check that the parity run actually catches regressions. Temporarily revert `crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c` line 2259 from `==` back to `!=`. Re-run the verify step from 4.1.**

Expected: step 4's LF tests still pass (the LF corpus does not exercise the CRLF code path), but the CRLF parity sub-run fails on the pipe-table tests.

Restore the fix afterwards: `git checkout -- crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c`.

- [ ] **Step 4.4: Final commit if any cleanup needed (e.g. doc comment polish).** Otherwise skip.

### Task 5: Reply to cscheid on PR #139

- [ ] **Step 5.1: Post a reply on the PR linking the design doc and summarizing the approach.** Use `gh pr comment 139 --repo quarto-dev/q2 --body @C:\tmp\reply.md` after drafting the reply locally. The reply should affirm his strategy, note the implementation lives in `cargo xtask verify` rather than a standalone script (consistent with the project's xtask convention), and call out that any test legitimately depending on bare-LF semantics will surface during the parity run and we'll handle it case-by-case.

---

## Self-review

- **Spec coverage.** All four spec sections (Goal, Mechanism, Failure modes, Scope) map to tasks: Mechanism → Tasks 1–3; Goal → Task 4 (e2e); Failure modes → Task 4.3 (regression simulation) and Task 5 (note in reply); Scope → tasks only touch xtask, no grammar/CI changes.
- **Placeholders.** None. Every code step has the exact code; every command has expected output.
- **Type consistency.** `to_crlf` and `run_parity_check` are referenced consistently. Config field name `skip_treesitter_crlf_tests` matches CLI flag (`--skip-treesitter-crlf-tests` after clap's kebab conversion) and `VerifyConfig` field across `main.rs` and `verify.rs`.
- **One open question.** Whether `tempfile` and `walkdir` are already xtask deps — Step 2.2 verifies and adds if needed. Not a blocker.
