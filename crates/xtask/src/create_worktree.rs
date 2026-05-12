//! `cargo xtask create-worktree` — set up a git worktree with beads redirect
//! and a marker-delimited CLAUDE.local.md context section.
//!
//! Three modes (exactly one required):
//!   - positional `<bd-id>` — beads issue (reads `br show`)
//!   - `--issue <N>` — GitHub issue triage (reads `gh issue view`)
//!   - `--upgrade` — cargo dependency upgrade (date-based branch)
//!
//! Filesystem-only: never touches beads state. Skills own beads lifecycle.

use anyhow::Context;
use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::util::with_native_separators;

const BEGIN_MARKER: &str =
    "<!-- BEGIN WORKTREE CONTEXT — managed by cargo xtask create-worktree -->";
const END_MARKER: &str = "<!-- END WORKTREE CONTEXT -->";

const STOP_WORDS: &[&str] = &[
    "a", "an", "the", "and", "or", "in", "on", "of", "to", "for", "with", "from", "at", "by", "is",
    "as",
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

    /// Print the manual command checklist after creating the worktree.
    /// Default output is terse and points at CLAUDE.local.md for details.
    #[arg(short, long)]
    pub verbose: bool,
}

pub fn parse_external_ref_to_github_url(ext: Option<&str>) -> Option<String> {
    let ext = ext?;
    let n = ext.strip_prefix("gh-")?;
    if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) {
        Some(format!("https://github.com/quarto-dev/q2/issues/{n}"))
    } else {
        None
    }
}

/// Neutralize any occurrences of BEGIN/END marker substrings inside
/// externally-sourced text (titles from `br`/`gh`). Without this, a title
/// containing `<!-- END WORKTREE CONTEXT -->` would terminate the section
/// prematurely on the next idempotent strip pass.
fn marker_safe(s: &str) -> String {
    s.replace(BEGIN_MARKER, "[BEGIN marker scrubbed]")
        .replace(END_MARKER, "[END marker scrubbed]")
}

pub fn strip_managed_section(content: &str) -> Result<String> {
    let Some(begin_pos) = content.find(BEGIN_MARKER) else {
        return Ok(content.to_string());
    };

    // Warn (but proceed) if a second BEGIN appears after the first.
    let after_begin = &content[begin_pos + BEGIN_MARKER.len()..];
    if after_begin.contains(BEGIN_MARKER) {
        eprintln!(
            "warning: CLAUDE.local.md contains multiple BEGIN markers \u{2014} using the first; \
             recommend manual review of {}",
            "CLAUDE.local.md"
        );
    }

    let end_search_start = begin_pos + BEGIN_MARKER.len();
    let end_rel = content[end_search_start..]
        .find(END_MARKER)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "CLAUDE.local.md has BEGIN marker without END marker \u{2014} refusing to modify; \
                 resolve manually"
            )
        })?;
    let end_marker_end = end_search_start + end_rel + END_MARKER.len();

    // Strip from the start of the BEGIN line through one trailing newline after END.
    let begin_line_start = content[..begin_pos].rfind('\n').map(|i| i + 1).unwrap_or(0);

    let mut after_end = end_marker_end;
    let rest = &content[after_end..];
    if rest.starts_with("\r\n") {
        after_end += 2;
    } else if rest.starts_with('\n') {
        after_end += 1;
    }

    let mut out = String::with_capacity(content.len());
    out.push_str(&content[..begin_line_start]);
    out.push_str(&content[after_end..]);
    Ok(out)
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
            s.push_str(&format!("**Beads:** {id} \u{2014} {title}\n"));
            if let Some(url) = github_url {
                s.push_str(&format!("**GitHub:** {url}\n"));
            }
            s.push_str("**Plan:** <!-- fill in after creating: claude-notes/plans/YYYY-MM-DD-name.md -->\n");
            s.push('\n');
            s.push_str(&format!(
                "Run `br show {id}` for current status and notes.\n\n"
            ));
            s.push_str("## Initial setup\n\n");
            s.push_str("Prep this worktree (skip steps already done):\n\n");
            s.push_str("- `cargo xtask verify --skip-hub-build` \u{2014} confirm branch HEAD is\n");
            s.push_str("  green. `--skip-hub-build` keeps this Rust-only so no `npm install`\n");
            s.push_str("  is needed. If verify errors with \"tool not found\", run\n");
            s.push_str("  `cargo xtask dev-setup` first (one-time install of cargo-nextest\n");
            s.push_str("  and wasm-bindgen-cli).\n");
            s.push_str("- `npm install` \u{2014} only if hub-client work is in scope. Not yet\n");
            s.push_str("  part of `cargo xtask dev-setup` (tracked in bd-7giz).\n");
            s.push_str(&format!(
                "- `br update {id} --status in_progress` \u{2014} claim the beads issue.\n"
            ));
            s.push('\n');
            s.push_str("When you create a plan file at\n");
            s.push_str("`claude-notes/plans/YYYY-MM-DD-<name>.md`, edit the **Plan:** line\n");
            s.push_str("above to point at it.\n");
            s
        }
        SectionKind::Issue { number, title, url } => {
            let title = marker_safe(title);
            let mut s = String::new();
            s.push_str("# Worktree Context\n\n");
            s.push_str("This is a **worktree** of the q2 repository. Main repo: `../..`\n\n");
            s.push_str(&format!("**GitHub issue:** #{number} \u{2014} {title}\n"));
            s.push_str(&format!("**URL:** {url}\n"));
            s.push_str(&format!(
                "**Beads:** (run `br search {number}` to find or create a beads issue)\n"
            ));
            s.push_str("**Plan:** <!-- fill in after creating: claude-notes/plans/YYYY-MM-DD-name.md -->\n");
            s.push('\n');
            s.push_str("## Initial setup\n\n");
            s.push_str("Prep this worktree (skip steps already done):\n\n");
            s.push_str("- `cargo xtask verify --skip-hub-build` \u{2014} confirm branch HEAD is\n");
            s.push_str("  green. If verify errors with \"tool not found\", run\n");
            s.push_str("  `cargo xtask dev-setup` first.\n");
            s.push_str("- `npm install` \u{2014} only if hub-client work is in scope.\n");
            s.push('\n');
            s.push_str("No beads issue exists yet; the triage skill creates one when\n");
            s.push_str("investigation surfaces real work. When that happens, edit the\n");
            s.push_str("**Beads:** line above with the new bd-XXXX. Edit **Plan:** likewise\n");
            s.push_str("once a plan or triage doc exists.\n");
            s
        }
        SectionKind::Upgrade { date } => {
            let mut s = String::new();
            s.push_str("# Worktree Context\n\n");
            s.push_str("This is a **worktree** of the q2 repository. Main repo: `../..`\n\n");
            s.push_str(&format!(
                "**Task:** Cargo dependency upgrade \u{2014} {date}\n"
            ));
            s.push_str("**Plan:** <!-- fill in if needed -->\n\n");
            s.push_str("## Initial setup\n\n");
            s.push_str("The `upgrade-cargo-deps` skill drives this worktree end-to-end. If\n");
            s.push_str("running manually instead, start with\n");
            s.push_str("`cargo xtask verify --skip-hub-build` to confirm HEAD is green.\n");
            s
        }
    };

    format!("{BEGIN_MARKER}\n{body}{END_MARKER}\n")
}

pub fn update_claude_local_md(path: &Path, new_section: &str) -> Result<()> {
    // 1. Read existing content (or empty if file missing).
    let existing = if path.exists() {
        let meta = path
            .symlink_metadata()
            .with_context(|| format!("reading metadata of {}", path.display()))?;
        if !meta.is_file() {
            anyhow::bail!(
                "CLAUDE.local.md exists but is not a regular file: {}",
                path.display()
            );
        }
        fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?
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
        out.push_str(nl); // blank line separator
        out.push_str(&body);
    }
    if !out.ends_with(nl) {
        out.push_str(nl);
    }

    // 6. Atomic write: write to .tmp then rename over target.
    // Build the temp path by appending ".tmp" to the full OsStr — avoids the
    // `Path::with_extension` ambiguity around dots in extensions.
    let mut tmp_os = path.as_os_str().to_owned();
    tmp_os.push(".tmp");
    let tmp = PathBuf::from(tmp_os);
    fs::write(&tmp, out.as_bytes()).with_context(|| format!("writing {}", tmp.display()))?;
    fs::rename(&tmp, path)
        .with_context(|| format!("renaming {} to {}", tmp.display(), path.display()))?;

    Ok(())
}

pub struct BeadsMetadata {
    pub title: String,
    pub external_ref: Option<String>,
}

pub fn fetch_beads_metadata(id: &str) -> Result<BeadsMetadata> {
    let output = Command::new("br")
        .args(["show", id, "--json"])
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                anyhow::anyhow!(
                    "br is required \u{2014} install via `cargo install beads-rust` or see project README"
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
                anyhow::anyhow!("gh is required \u{2014} see https://cli.github.com/")
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

/// Absolute path to the main repository working tree, resolved via
/// `git rev-parse --path-format=absolute --git-common-dir`.
///
/// Worktree creation must anchor `.worktrees/<leaf>` to this root so the
/// new worktree always lands at `<main-repo>/.worktrees/<leaf>` regardless
/// of whether the command is invoked from the main worktree, a nested
/// worktree, or a subdirectory.
pub fn repo_root() -> Result<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .output()
        .context("spawning `git rev-parse --git-common-dir`")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "git rev-parse --git-common-dir failed (exit {:?}):\n{stderr}",
            output.status.code()
        );
    }
    let raw = String::from_utf8(output.stdout)
        .context("git common-dir path was not valid UTF-8")?
        .trim_end_matches(['\r', '\n'])
        .to_string();
    // git emits forward slashes on Windows; normalize so every downstream
    // `PathBuf::join` and `.display()` produces a consistent separator.
    let common_dir = with_native_separators(Path::new(&raw));
    common_dir
        .parent()
        .map(Path::to_path_buf)
        .with_context(|| format!("git common-dir has no parent: {}", common_dir.display()))
}

pub fn git_worktree_add(branch: &str, dir: &Path, base: &str) -> Result<()> {
    if dir.exists() {
        anyhow::bail!("worktree directory already exists: {}", dir.display());
    }

    // Pre-check: does the branch already exist locally?
    let check = Command::new("git")
        .args([
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ])
        .output()
        .context("spawning `git rev-parse`")?;
    if check.status.success() {
        anyhow::bail!(
            "branch already exists: {branch} \u{2014} remove it or pass --slug to disambiguate"
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
            ".beads/ directory missing in new worktree: {} \u{2014} was the base branch correct?",
            redirect.parent().unwrap().display()
        );
    }
    // LF line ending intentionally, even on Windows.
    fs::write(&redirect, "../../../.beads\n")
        .with_context(|| format!("writing {}", redirect.display()))?;
    Ok(())
}

pub fn run(args: Args) -> Result<()> {
    // Always anchor new worktrees to the main repo root so the command works
    // identically from main, from another worktree, or from any subdirectory.
    let root = repo_root()?;

    // Mode is enforced by clap::ArgGroup(required, single).
    let plan = if let Some(id) = args.beads_id.as_deref() {
        plan_beads(id, args.slug.as_deref(), &args.base, &root)?
    } else if let Some(n) = args.issue {
        plan_issue(n, args.slug.as_deref(), &args.base, &root)?
    } else if args.upgrade {
        plan_upgrade(args.slug.as_deref(), &args.base, &root)?
    } else {
        unreachable!("clap ArgGroup guarantees one mode is set");
    };

    git_worktree_add(&plan.branch, &plan.dir, &plan.base)?;

    // From here on, on any error we roll back the worktree+branch we just
    // created so a retry is not blocked by directory/branch collision.
    let post = (|| -> Result<()> {
        // Test-only injection point: lets the Phase E smoke test exercise the
        // rollback path without modifying production logic.
        if std::env::var("Q2_CREATE_WORKTREE_INJECT_FAIL").as_deref() == Ok("after_worktree_add") {
            anyhow::bail!("Q2_CREATE_WORKTREE_INJECT_FAIL=after_worktree_add (test hook)");
        }
        write_beads_redirect(&plan.dir)?;
        let section = build_section(&plan.kind);
        let claude_local = plan.dir.join("CLAUDE.local.md");
        update_claude_local_md(&claude_local, &section)?;
        Ok(())
    })();

    if let Err(e) = post {
        eprintln!("error after worktree creation: {e:#}");
        eprintln!(
            "rolling back worktree {} and branch {} ...",
            plan.dir.display(),
            plan.branch
        );
        let mut rollback_issues: Vec<String> = Vec::new();

        match Command::new("git")
            .arg("worktree")
            .arg("remove")
            .arg("--force")
            .arg(plan.dir.as_os_str())
            .output()
        {
            Ok(out) if out.status.success() => {}
            Ok(out) => rollback_issues.push(format!(
                "`git worktree remove --force {}` failed:\n    {}\n  manual cleanup: git worktree remove --force {}",
                plan.dir.display(),
                String::from_utf8_lossy(&out.stderr).trim().replace('\n', "\n    "),
                plan.dir.display(),
            )),
            Err(spawn_err) => rollback_issues.push(format!(
                "could not spawn `git worktree remove`: {spawn_err}\n  manual cleanup: git worktree remove --force {}",
                plan.dir.display()
            )),
        }

        match Command::new("git")
            .args(["branch", "-D", &plan.branch])
            .output()
        {
            Ok(out) if out.status.success() => {}
            Ok(out) => rollback_issues.push(format!(
                "`git branch -D {}` failed:\n    {}\n  manual cleanup: git branch -D {}",
                plan.branch,
                String::from_utf8_lossy(&out.stderr)
                    .trim()
                    .replace('\n', "\n    "),
                plan.branch,
            )),
            Err(spawn_err) => rollback_issues.push(format!(
                "could not spawn `git branch -D`: {spawn_err}\n  manual cleanup: git branch -D {}",
                plan.branch
            )),
        }

        if rollback_issues.is_empty() {
            eprintln!("rollback complete.");
        } else {
            eprintln!("rollback incomplete \u{2014} manual steps required:");
            for issue in &rollback_issues {
                eprintln!("  - {issue}");
            }
        }
        return Err(e);
    }

    print_summary(&plan, args.verbose);
    Ok(())
}

struct Plan {
    branch: String,
    dir: PathBuf,
    base: String,
    kind: SectionKind,
}

fn plan_beads(id: &str, slug_override: Option<&str>, base: &str, repo_root: &Path) -> Result<Plan> {
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
        dir: repo_root.join(".worktrees").join(&leaf),
        base: base.to_string(),
        kind: SectionKind::Beads {
            id: id.to_string(),
            title: meta.title,
            github_url,
        },
    })
}

fn plan_issue(
    number: u32,
    slug_suffix: Option<&str>,
    base: &str,
    repo_root: &Path,
) -> Result<Plan> {
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
        dir: repo_root.join(".worktrees").join(&leaf),
        base: base.to_string(),
        kind: SectionKind::Issue {
            number,
            title: gh.title,
            url: gh.url,
        },
    })
}

fn plan_upgrade(slug_suffix: Option<&str>, base: &str, repo_root: &Path) -> Result<Plan> {
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
        dir: repo_root.join(".worktrees").join(&leaf),
        base: base.to_string(),
        kind: SectionKind::Upgrade { date },
    })
}

fn print_summary(plan: &Plan, verbose: bool) {
    println!("Created worktree: {}/", plan.dir.display());
    println!("  Branch:  {}", plan.branch);
    match &plan.kind {
        SectionKind::Beads {
            id,
            title,
            github_url,
        } => {
            println!("  Beads:   {id} \u{2014} {title}");
            if let Some(url) = github_url {
                println!("  GitHub:  {url}");
            }
        }
        SectionKind::Issue { number, title, url } => {
            println!("  Issue:   #{number} \u{2014} {title}");
            println!("  URL:     {url}");
        }
        SectionKind::Upgrade { date } => {
            println!("  Task:    Cargo dependency upgrade \u{2014} {date}");
        }
    }
    println!();

    if !verbose {
        // Terse default: point at the worktree and CLAUDE.local.md.
        println!("Next: cd {}", plan.dir.display());
        println!(
            "Open a Claude Code session there, or read CLAUDE.local.md for the setup checklist."
        );
        println!();
        println!("(Pass -v / --verbose for the manual command list.)");
        return;
    }

    // Verbose: include the manual command checklist inline.
    println!("Next steps:");
    println!("  cd {}", plan.dir.display());
    println!();
    println!("  Open a Claude Code session there \u{2014} CLAUDE.local.md has the same");
    println!("  checklist, expanded. Or run the prep yourself:");
    println!();
    println!("    cargo xtask verify --skip-hub-build      # confirm branch HEAD is green");
    println!("    npm install                              # hub-client deps (only if in scope)");
    if let SectionKind::Beads { id, .. } = &plan.kind {
        println!("    br update {id} --status in_progress   # claim the beads issue");
    }
    println!();
    println!("  Once a plan file exists at claude-notes/plans/YYYY-MM-DD-<name>.md,");
    println!("  edit the **Plan:** line in CLAUDE.local.md to point at it.");
}

pub fn detect_line_ending(content: &str) -> &'static str {
    // Sniff up to first 1 KiB, snapped to a char boundary so slicing is valid.
    let mut sniff_end = content.len().min(1024);
    while sniff_end > 0 && !content.is_char_boundary(sniff_end) {
        sniff_end -= 1;
    }
    let sniff = &content[..sniff_end];

    let mut crlf_count = sniff.matches("\r\n").count();
    let lf_total = sniff.matches('\n').count();
    let bare_lf = lf_total - crlf_count;

    // Boundary case: the sniff may end with `\r` and the matching `\n` falls
    // just past the window. Peek one byte ahead so a CRLF pair split exactly
    // on the 1 KiB boundary is not mis-classified as LF.
    if sniff.ends_with('\r') && content.as_bytes().get(sniff_end) == Some(&b'\n') {
        crlf_count += 1;
    }

    if crlf_count > 0 && bare_lf == 0 {
        "\r\n"
    } else {
        "\n"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn repo_root_returns_absolute_directory_with_dot_git() {
        // Test runs inside this checkout, so repo_root() must succeed and
        // point at a directory containing a `.git` entry (file or dir).
        let root = repo_root().expect("repo_root() should succeed inside a git checkout");
        assert!(
            root.is_absolute(),
            "repo_root() must return an absolute path, got {}",
            root.display()
        );
        assert!(
            root.is_dir(),
            "repo_root() must point at an existing directory, got {}",
            root.display()
        );
        let git_entry = root.join(".git");
        assert!(
            git_entry.exists(),
            "repo_root() result {} should contain a .git entry",
            root.display()
        );
    }

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
        assert_eq!(
            lf_total, crlf_pairs,
            "bare LFs found in CRLF output: {:?}",
            out
        );
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
        assert_eq!(
            parse_external_ref_to_github_url(Some("linear-ABC-12")),
            None
        );
    }

    #[test]
    fn external_ref_malformed_gh_returns_none() {
        // Non-numeric suffix
        assert_eq!(parse_external_ref_to_github_url(Some("gh-foo")), None);
        // Empty suffix
        assert_eq!(parse_external_ref_to_github_url(Some("gh-")), None);
    }

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

    #[test]
    fn detect_le_crlf_split_at_sniff_boundary() {
        // \r at byte 1023 (last byte of sniff window), \n at byte 1024 (first
        // byte past the window). The sniff sees no \r\n pair and no bare \n
        // — without the boundary fix, this would mis-classify as LF.
        let mut s = String::with_capacity(1100);
        s.push_str(&"x".repeat(1023));
        s.push('\r');
        s.push('\n');
        s.push_str("more");
        assert_eq!(detect_line_ending(&s), "\r\n");
    }

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

    #[test]
    fn strip_no_marker_returns_input_unchanged() {
        let input = "# My notes\nfoo bar\n";
        assert_eq!(strip_managed_section(input).unwrap(), input);
    }

    #[test]
    fn strip_full_managed_section() {
        let input =
            format!("{BEGIN_MARKER}\n# Worktree Context\nstuff\n{END_MARKER}\n# My notes\nfoo\n");
        assert_eq!(strip_managed_section(&input).unwrap(), "# My notes\nfoo\n");
    }

    #[test]
    fn strip_section_in_middle_of_file() {
        let input = format!("# Header\n\n{BEGIN_MARKER}\nbody\n{END_MARKER}\n\n# Footer\n");
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
}
