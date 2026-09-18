//! `cargo xtask render-corpus-diff` — byte-identity corpus + capture/diff
//! harness for the pandoc-hybrid epic's P1 no-regression bar (Task 8 of
//! `claude-notes/plans/2026-09-18-pandoc-hybrid-P1-implementation.md`).
//!
//! ## What it does
//!
//! 1. Builds `q2` at the current worktree's `HEAD`.
//! 2. Checks out `--base <commit>` into a throwaway detached `git worktree`
//!    (outside this repo tree, in the system temp directory) and builds
//!    `q2` there too.
//! 3. Renders `--corpus` (default `docs`) with each binary into its own
//!    capture directory, stamping each with a manifest recording the
//!    commit it came from.
//! 4. Byte-diffs the two capture trees and reports the result.
//!
//! This is a **dev-only, local-only** tool (Tier `G` in the P1 plan's Test
//! Seam Spec) — it builds two real binaries from two real checkouts, which
//! takes real wall-clock time, and is never invoked from `cargo xtask
//! verify` or CI.
//!
//! ## Scope of the byte-identity claim (default `--corpus docs`)
//!
//! `q2 render docs/` proves byte-identity for the **`HtmlRender` leg of the
//! `docs/` corpus specifically** — nothing more. As of P1 Task 8, `docs/`
//! contains zero real `format: revealjs` documents (every occurrence is
//! inside a fenced code block) and exactly one live footnote (an inline
//! `^[...]`, no reference-style `[^a]` / `[^a]: …` pair anywhere). This
//! harness therefore does **not** exercise revealjs, the preview AST leg,
//! or footnote-reference-id dedup, even though a passing run superficially
//! looks like it covers "html, revealjs, and preview." Those legs are
//! covered instead by, respectively: `revealjs_features.rs` (revealjs
//! end-to-end render + per-slide footnote coalescing), `t1_5`/`pipeline.rs`
//! (reveal transform name-list pinning), `t2_5`/`pipeline.rs` (preview
//! transform name-list pinning), and `footnotes_dedup.rs` (footnote-
//! reference dedup, at the unit/integration level, not corpus level). See
//! `claude-notes/plans/2026-09-18-pandoc-hybrid-P1-implementation.md` Task
//! 8's acceptance criterion for the full record.
//!
//! ## Safety around shared worktree state
//!
//! The throwaway `--base` checkout is created with `git worktree add
//! --detach` at a path under the system temp directory — **never** under
//! this repo's `.worktrees/`, which is a shared, well-known location other
//! sessions may be using. `git worktree add`/`remove` register/unregister
//! in the *shared* `.git` (all worktrees of one repo share one
//! `.git/worktrees/` registry) even though the checkout directory itself
//! lives elsewhere; [`BaseWorktree`]'s `Drop` impl unregisters it again on
//! the way out (best-effort — a failure is reported, not silently eaten).

use anyhow::{Context, Result, bail};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::switch_task::current_worktree_root;
use crate::util::nested_command;

/// The file each capture directory carries recording which commit
/// produced it. Excluded from the byte-diff itself (its whole purpose is
/// giving [`check_captures`] something to compare, not being compared).
const MANIFEST_FILE_NAME: &str = "CORPUS_CAPTURE_COMMIT.txt";

/// Arguments for `cargo xtask render-corpus-diff`.
pub struct Args {
    /// Commit-ish to check out into a throwaway worktree and compare
    /// against `HEAD` (e.g. a `PLAN_BASE` SHA recorded in an SDD ledger).
    pub base: String,
    /// Corpus to render, relative to the repo root. Default: `docs`.
    pub corpus: String,
    /// Keep the capture directories and the throwaway `--base` worktree
    /// around after a successful run (normally only kept on failure, so
    /// a diff can be inspected).
    pub keep: bool,
}

pub fn run(args: Args) -> Result<()> {
    let head_root = current_worktree_root()?;
    let head_sha = rev_parse(&head_root, "HEAD")?;
    let base_sha = rev_parse(&head_root, &args.base)?;

    if head_sha == base_sha {
        bail!(
            "HEAD ({head_sha}) and --base {} resolve to the same commit \u{2014} refusing a \
             same-commit corpus diff (it would be trivially empty regardless of whether the \
             harness is wired to the right checkouts)",
            args.base
        );
    }

    let scratch = tempfile::Builder::new()
        .prefix("q2-render-corpus-diff-")
        .tempdir()
        .context("creating scratch tempdir for capture output")?;
    let capture_head = scratch.path().join("capture-head");
    let capture_base = scratch.path().join("capture-base");

    println!(
        "==> building q2 at HEAD ({head_sha}) in {}",
        head_root.display()
    );
    build_q2(&head_root)?;

    println!("==> checking out base ({base_sha}) into a throwaway worktree");
    let base_worktree = BaseWorktree::create(&head_root, &base_sha, args.keep)?;

    println!(
        "==> building q2 at base ({base_sha}) in {}",
        base_worktree.dir.display()
    );
    build_q2(&base_worktree.dir)?;

    ensure_examples_resource(&head_root, &args.corpus)?;
    ensure_examples_resource(&base_worktree.dir, &args.corpus)?;

    println!("==> rendering `{}` with the HEAD binary", args.corpus);
    capture_render(&head_root, &args.corpus, &capture_head, &head_sha)?;

    println!("==> rendering `{}` with the base binary", args.corpus);
    capture_render(&base_worktree.dir, &args.corpus, &capture_base, &base_sha)?;

    let diffs = diff_captures(&capture_head, &capture_base)?;

    if diffs.is_empty() {
        println!(
            "\n\u{2713} byte-identical: base {base_sha} and HEAD {head_sha} produce identical \
             output for corpus `{}` ({} files compared)",
            args.corpus,
            collect_files(&capture_head)?.len()
        );
        if args.keep {
            println!(
                "--keep set: captures left at {} and {}",
                capture_head.display(),
                capture_base.display()
            );
        }
        Ok(())
    } else {
        eprintln!(
            "\n\u{2717} corpus byte-diff is non-empty ({} differing entries) between base \
             {base_sha} and HEAD {head_sha}:",
            diffs.len()
        );
        for d in &diffs {
            eprintln!("  {d}");
        }
        eprintln!(
            "\ncaptures left at {} and {} for inspection",
            capture_head.display(),
            capture_base.display()
        );
        // Keep the capture dirs around on failure regardless of --keep so
        // the diff can be inspected — only a clean run honors --keep as a
        // request, a dirty run always leaves evidence behind.
        std::mem::forget(scratch);
        bail!(
            "corpus byte-diff is non-empty ({} entries) between base {base_sha} and HEAD {head_sha}",
            diffs.len()
        );
    }
}

/// A throwaway `git worktree --detach` checkout of `--base`, torn down on
/// drop. Lives outside this repo's tree (system temp dir) — never under
/// `.worktrees/`, which is shared with sibling sessions.
struct BaseWorktree {
    /// The repo (any worktree of it) to run `git worktree remove` from.
    repo: PathBuf,
    dir: PathBuf,
    keep: bool,
}

impl BaseWorktree {
    fn create(repo: &Path, sha: &str, keep: bool) -> Result<Self> {
        // Reserve a unique path via tempfile, then immediately vacate it —
        // `git worktree add` requires the target not to exist (or to be
        // empty), and tempfile already guarantees uniqueness.
        let reserved = tempfile::Builder::new()
            .prefix("q2-plan-base-")
            .tempdir()
            .context("reserving a unique path for the base worktree")?
            .keep();
        fs::remove_dir(&reserved)
            .with_context(|| format!("clearing reserved path {}", reserved.display()))?;

        let output = Command::new("git")
            .args(["worktree", "add", "--detach"])
            .arg(&reserved)
            .arg(sha)
            .current_dir(repo)
            .output()
            .context("spawning `git worktree add --detach`")?;
        if !output.status.success() {
            bail!(
                "`git worktree add --detach {} {sha}` failed (exit {:?}):\n{}",
                reserved.display(),
                output.status.code(),
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(Self {
            repo: repo.to_path_buf(),
            dir: reserved,
            keep,
        })
    }
}

impl Drop for BaseWorktree {
    fn drop(&mut self) {
        if self.keep {
            eprintln!(
                "--keep set: leaving throwaway base worktree at {}",
                self.dir.display()
            );
            return;
        }
        let output = Command::new("git")
            .args(["worktree", "remove", "--force"])
            .arg(&self.dir)
            .current_dir(&self.repo)
            .output();
        match output {
            Ok(out) if out.status.success() => {}
            Ok(out) => eprintln!(
                "warning: `git worktree remove --force {}` failed:\n{}\n  manual cleanup: \
                 git worktree remove --force {}",
                self.dir.display(),
                String::from_utf8_lossy(&out.stderr),
                self.dir.display()
            ),
            Err(e) => eprintln!(
                "warning: could not spawn `git worktree remove --force {}`: {e}\n  manual \
                 cleanup: git worktree remove --force {}",
                self.dir.display(),
                self.dir.display()
            ),
        }
    }
}

fn rev_parse(repo: &Path, commitish: &str) -> Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--verify", commitish])
        .current_dir(repo)
        .output()
        .with_context(|| format!("spawning `git rev-parse --verify {commitish}`"))?;
    if !output.status.success() {
        bail!(
            "`git rev-parse --verify {commitish}` failed in {}:\n{}",
            repo.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn build_q2(root: &Path) -> Result<()> {
    let mut cmd = nested_command("cargo");
    cmd.args(["build", "--bin", "q2"]).current_dir(root);
    let status = cmd
        .status()
        .with_context(|| format!("spawning `cargo build --bin q2` in {}", root.display()))?;
    if !status.success() {
        bail!("`cargo build --bin q2` failed in {}", root.display());
    }
    Ok(())
}

#[cfg(windows)]
fn q2_binary_name() -> &'static str {
    "q2.exe"
}
#[cfg(not(windows))]
fn q2_binary_name() -> &'static str {
    "q2"
}

/// `docs/examples/` (the `.embed-example-iframe` resource tree) is
/// generated by `cargo xtask stage-doc-examples` into the *main repo's*
/// checkout (it always resolves via the shared git-common-dir, regardless
/// of which worktree invokes it — see that command's own module doc) and
/// is gitignored. Neither this worktree nor a freshly created throwaway
/// `--base` worktree has it locally. Since the example projects it stages
/// are untouched by this refactor, it's safe (and much faster than
/// re-staging per checkout) to copy the one already-staged tree into
/// whichever checkout is missing it, rather than regenerating it.
fn ensure_examples_resource(checkout_root: &Path, corpus: &str) -> Result<()> {
    let examples_dir = checkout_root.join(corpus).join("examples");
    if examples_dir.is_dir() {
        return Ok(());
    }
    let main_repo = crate::create_worktree::repo_root()?;
    let reference = main_repo.join(corpus).join("examples");
    if !reference.is_dir() {
        bail!(
            "{} is missing and no staged reference exists at {} \u{2014} run `cargo xtask \
             stage-doc-examples` first",
            examples_dir.display(),
            reference.display()
        );
    }
    copy_dir_recursive(&reference, &examples_dir)
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst).with_context(|| format!("creating directory {}", dst.display()))?;
    for entry in walkdir::WalkDir::new(src) {
        let entry = entry.context("walking directory tree to copy")?;
        let rel = entry
            .path()
            .strip_prefix(src)
            .expect("walkdir entries are rooted at src");
        if rel.as_os_str().is_empty() {
            continue;
        }
        let target = dst.join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)
                .with_context(|| format!("creating directory {}", target.display()))?;
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("creating directory {}", parent.display()))?;
            }
            fs::copy(entry.path(), &target).with_context(|| {
                format!("copying {} to {}", entry.path().display(), target.display())
            })?;
        }
    }
    Ok(())
}

/// Render `corpus` (relative to `checkout_root`) with the `q2` binary
/// already built in `checkout_root/target/debug/`, relocate the result to
/// `out_dir`, and stamp it with a manifest recording `commit_sha`.
///
/// Deliberately does **not** pass `--output-dir <out_dir>`: for a
/// full-project render (as opposed to a single-doc render) that flag only
/// affects `determine_output_paths`' per-file target, not
/// `ResourceResolverContext`'s `allowed_output_roots()` security boundary,
/// which is still derived from `project.output_dir` (the project's
/// *configured* output directory, e.g. website's default `_site`) —
/// pointing `--output-dir` outside the project tree makes every single
/// file's destination fail the "is not under any allowed root" check.
/// Instead this renders to the project's own configured location and
/// relocates the result itself, entirely outside the security boundary.
fn capture_render(
    checkout_root: &Path,
    corpus: &str,
    out_dir: &Path,
    commit_sha: &str,
) -> Result<()> {
    let q2_bin = checkout_root
        .join("target")
        .join("debug")
        .join(q2_binary_name());
    if !q2_bin.is_file() {
        bail!(
            "expected a `q2` binary at {} \u{2014} the build step must have failed silently",
            q2_bin.display()
        );
    }

    // `docs/_site` (the website-type default) is gitignored build output;
    // start from a clean slate so a stale prior render can't leak into
    // the capture.
    let rendered_site = checkout_root.join(corpus).join("_site");
    if rendered_site.exists() {
        fs::remove_dir_all(&rendered_site)
            .with_context(|| format!("clearing stale {}", rendered_site.display()))?;
    }

    let status = Command::new(&q2_bin)
        .arg("render")
        .arg(corpus)
        .current_dir(checkout_root)
        .status()
        .with_context(|| format!("spawning {}", q2_bin.display()))?;
    if !status.success() {
        bail!("`{} render {corpus}` failed", q2_bin.display());
    }

    if !rendered_site.is_dir() {
        bail!(
            "expected rendered output at {} but it does not exist \u{2014} does `{corpus}` render \
             to something other than the website default `_site`?",
            rendered_site.display()
        );
    }

    if let Some(parent) = out_dir.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating directory {}", parent.display()))?;
    }
    // Prefer a plain rename (cheap); the scratch capture dir and the
    // checkout can be on different filesystems (e.g. a git worktree under
    // the system temp dir vs. one under the repo tree), so fall back to
    // copy + remove on cross-device rename failures.
    if fs::rename(&rendered_site, out_dir).is_err() {
        copy_dir_recursive(&rendered_site, out_dir)?;
        fs::remove_dir_all(&rendered_site).with_context(|| {
            format!(
                "removing {} after copying it to the capture directory",
                rendered_site.display()
            )
        })?;
    }

    write_manifest(out_dir, commit_sha)
}

fn write_manifest(dir: &Path, sha: &str) -> Result<()> {
    fs::write(dir.join(MANIFEST_FILE_NAME), format!("{sha}\n"))
        .with_context(|| format!("writing capture manifest into {}", dir.display()))
}

/// The precondition check named in T8.2: a missing capture directory is a
/// loud, named-path error — never a silent skip.
fn require_capture_dir(dir: &Path) -> Result<()> {
    if !dir.is_dir() {
        bail!("capture directory not found: {}", dir.display());
    }
    Ok(())
}

fn read_manifest(dir: &Path) -> Result<String> {
    let path = dir.join(MANIFEST_FILE_NAME);
    let contents = fs::read_to_string(&path)
        .with_context(|| format!("capture manifest not found: {}", path.display()))?;
    Ok(contents.trim().to_string())
}

/// Validate that both capture directories exist and carry manifests
/// recording *different* commits, returning `(sha_a, sha_b)`. This is the
/// harness's whole precondition surface (T8.2) — a missing directory, a
/// missing manifest, or two manifests agreeing on one commit are all
/// loud, named errors, never a silent no-op.
pub(crate) fn check_captures(dir_a: &Path, dir_b: &Path) -> Result<(String, String)> {
    require_capture_dir(dir_a)?;
    require_capture_dir(dir_b)?;
    let sha_a = read_manifest(dir_a)?;
    let sha_b = read_manifest(dir_b)?;
    if sha_a == sha_b {
        bail!(
            "both captures record the same commit ({sha_a}) \u{2014} refusing to diff a capture \
             against itself; a diff between two builds of the same commit is trivially empty \
             and proves nothing"
        );
    }
    Ok((sha_a, sha_b))
}

fn collect_files(dir: &Path) -> Result<BTreeSet<PathBuf>> {
    let mut set = BTreeSet::new();
    for entry in walkdir::WalkDir::new(dir) {
        let entry = entry.context("walking capture directory")?;
        if entry.file_type().is_file() {
            let rel = entry
                .path()
                .strip_prefix(dir)
                .expect("walkdir entries are rooted at dir")
                .to_path_buf();
            if rel == Path::new(MANIFEST_FILE_NAME) {
                continue;
            }
            set.insert(rel);
        }
    }
    Ok(set)
}

/// Byte-diff two capture directories, after validating [`check_captures`]'s
/// preconditions. Returns a human-readable description per differing
/// entry; an empty vec means byte-identical.
pub(crate) fn diff_captures(dir_a: &Path, dir_b: &Path) -> Result<Vec<String>> {
    check_captures(dir_a, dir_b)?;

    let files_a = collect_files(dir_a)?;
    let files_b = collect_files(dir_b)?;

    let mut diffs = Vec::new();
    for rel in files_a.union(&files_b) {
        let path_a = dir_a.join(rel);
        let path_b = dir_b.join(rel);
        match (path_a.is_file(), path_b.is_file()) {
            (true, false) => diffs.push(format!("only in A: {}", rel.display())),
            (false, true) => diffs.push(format!("only in B: {}", rel.display())),
            (true, true) => {
                let a =
                    fs::read(&path_a).with_context(|| format!("reading {}", path_a.display()))?;
                let b =
                    fs::read(&path_b).with_context(|| format!("reading {}", path_b.display()))?;
                if a != b {
                    diffs.push(format!("content differs: {}", rel.display()));
                }
            }
            (false, false) => unreachable!("path came from a directory walk of one of the two"),
        }
    }
    Ok(diffs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    // T8.2 — precondition check: missing capture dir is a loud, named
    // error, never a silent skip. Named revert hunk: replacing
    // `require_capture_dir`'s `bail!` with `return Ok(())` must turn this
    // RED (the assertion on the error text fails, and — undischarged by
    // this in-process test but true at the CLI — the process would exit
    // 0 instead of non-zero).
    #[test]
    fn check_captures_errors_loudly_on_missing_dir() {
        let existing = tempdir().unwrap();
        write_manifest(existing.path(), "abc123").unwrap();
        let missing = existing.path().join("does-not-exist");

        let err = check_captures(&missing, existing.path()).unwrap_err();

        let msg = err.to_string();
        assert!(
            msg.contains("capture directory not found"),
            "error must flag the missing capture directory specifically, not fall through to \
             a different check: {msg}"
        );
        assert!(
            msg.contains(&missing.display().to_string()),
            "error must name the missing path: {msg}"
        );
    }

    #[test]
    fn check_captures_errors_loudly_on_missing_manifest() {
        let a = tempdir().unwrap();
        let b = tempdir().unwrap();
        write_manifest(b.path(), "def456").unwrap();
        // `a` exists as a directory but was never stamped with a manifest.

        let err = check_captures(a.path(), b.path()).unwrap_err();

        assert!(
            err.to_string().contains("manifest"),
            "error must mention the missing manifest: {err}"
        );
    }

    // Refactor-induced vacuity check: two captures of the same commit
    // must never be accepted, or an "empty diff" survives the harness
    // being wired to two builds of one checkout.
    #[test]
    fn check_captures_refuses_two_manifests_with_the_same_commit() {
        let a = tempdir().unwrap();
        let b = tempdir().unwrap();
        write_manifest(a.path(), "same-sha").unwrap();
        write_manifest(b.path(), "same-sha").unwrap();

        let err = check_captures(a.path(), b.path()).unwrap_err();

        assert!(
            err.to_string().contains("same commit"),
            "error must call out the same-commit vacuity trap: {err}"
        );
    }

    #[test]
    fn check_captures_accepts_two_manifests_with_different_commits() {
        let a = tempdir().unwrap();
        let b = tempdir().unwrap();
        write_manifest(a.path(), "sha-a").unwrap();
        write_manifest(b.path(), "sha-b").unwrap();

        let (sha_a, sha_b) = check_captures(a.path(), b.path()).unwrap();

        assert_eq!(sha_a, "sha-a");
        assert_eq!(sha_b, "sha-b");
    }

    #[test]
    fn diff_captures_empty_for_byte_identical_trees() {
        let a = tempdir().unwrap();
        let b = tempdir().unwrap();
        fs::create_dir_all(a.path().join("sub")).unwrap();
        fs::create_dir_all(b.path().join("sub")).unwrap();
        fs::write(a.path().join("index.html"), b"hello").unwrap();
        fs::write(b.path().join("index.html"), b"hello").unwrap();
        fs::write(a.path().join("sub/page.html"), b"nested").unwrap();
        fs::write(b.path().join("sub/page.html"), b"nested").unwrap();
        write_manifest(a.path(), "sha-a").unwrap();
        write_manifest(b.path(), "sha-b").unwrap();

        let diffs = diff_captures(a.path(), b.path()).unwrap();

        assert!(diffs.is_empty(), "expected no diffs, got {diffs:?}");
    }

    #[test]
    fn diff_captures_reports_content_and_presence_differences() {
        let a = tempdir().unwrap();
        let b = tempdir().unwrap();
        fs::write(a.path().join("index.html"), b"hello").unwrap();
        fs::write(b.path().join("index.html"), b"goodbye").unwrap();
        fs::write(a.path().join("only-a.html"), b"x").unwrap();
        fs::write(b.path().join("only-b.html"), b"y").unwrap();
        write_manifest(a.path(), "sha-a").unwrap();
        write_manifest(b.path(), "sha-b").unwrap();

        let diffs = diff_captures(a.path(), b.path()).unwrap();

        assert_eq!(diffs.len(), 3, "expected 3 differing entries: {diffs:?}");
        assert!(
            diffs
                .iter()
                .any(|d| d.contains("content differs: index.html"))
        );
        assert!(diffs.iter().any(|d| d.contains("only in A: only-a.html")));
        assert!(diffs.iter().any(|d| d.contains("only in B: only-b.html")));
    }

    #[test]
    fn diff_captures_ignores_the_manifest_file_itself() {
        let a = tempdir().unwrap();
        let b = tempdir().unwrap();
        write_manifest(a.path(), "sha-a").unwrap();
        write_manifest(b.path(), "sha-b").unwrap();

        // Manifests deliberately disagree (different SHAs) — if the diff
        // didn't exclude MANIFEST_FILE_NAME this would report a spurious
        // "content differs" entry for it.
        let diffs = diff_captures(a.path(), b.path()).unwrap();

        assert!(
            diffs.is_empty(),
            "manifest file must not appear in the diff: {diffs:?}"
        );
    }
}
