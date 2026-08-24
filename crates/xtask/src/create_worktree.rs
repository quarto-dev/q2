//! `cargo xtask create-worktree` — set up a git worktree with a
//! marker-delimited CLAUDE.local.md context section.
//!
//! Three modes (exactly one required):
//!   - positional `<bd-id>` — braid strand (reads `braid show`)
//!   - `--issue <N>` — GitHub issue triage (reads `gh issue view`)
//!   - `--upgrade` — cargo dependency upgrade (date-based branch)
//!
//! Filesystem-only: never touches braid state. Skills own the strand
//! lifecycle. (braid needs no per-worktree redirect: a worktree under
//! `.worktrees/` resolves the skein via the repo-root `.braid.toml`
//! walk-up, or the committed `.braid-project` marker + user config.)

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
    Braid {
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

/// Local git branch name for a braid strand's worktree / topic branch:
/// `braid/<leaf>`, where `<leaf>` is `<id>-<slug>`. The `braid/` prefix is a
/// plain git namespace (renamed from the historical `beads/` — bd-yjh1y117); it
/// does not imply beads involvement. Remote refs use a work-type prefix
/// (`bugfix/`, `feature/`) chosen at push time, not here.
pub fn strand_branch(leaf: &str) -> String {
    format!("braid/{leaf}")
}

#[derive(clap::Args)]
#[command(group(clap::ArgGroup::new("mode").required(true).multiple(false)))]
pub struct Args {
    /// Braid strand ID, e.g. `bd-1d3e`. Reads `braid show <id>` for title and external_ref.
    #[arg(group = "mode")]
    pub beads_id: Option<String>,

    /// GitHub issue number, e.g. `157`. Reads `gh issue view`.
    #[arg(long, group = "mode")]
    pub issue: Option<u32>,

    /// Cargo dependency upgrade — uses today's date for branch name.
    #[arg(long, group = "mode")]
    pub upgrade: bool,

    /// Override auto-derived slug. In braid mode replaces the derived slug;
    /// in issue/upgrade modes appended as a suffix (for parallel-worktree workflows).
    #[arg(long)]
    pub slug: Option<String>,

    /// Base branch. When omitted, falls back to `main` — but in braid
    /// mode, if the strand has an open parent epic, a warning is
    /// printed nudging you toward the epic's integration branch.
    /// Pass `--base main` explicitly to silence that warning.
    #[arg(long)]
    pub base: Option<String>,
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
/// externally-sourced text (titles from `braid`/`gh`). Without this, a title
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
             recommend manual review of CLAUDE.local.md"
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
    let begin_line_start = content[..begin_pos].rfind('\n').map_or(0, |i| i + 1);

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
        SectionKind::Braid {
            id,
            title,
            github_url,
        } => {
            let title = marker_safe(title);
            let mut s = String::new();
            s.push_str("# Worktree Context\n\n");
            s.push_str("This is a **worktree** of the q2 repository. Main repo: `../..`\n\n");
            s.push_str(&format!("**Braid:** {id} \u{2014} {title}\n"));
            if let Some(url) = github_url {
                s.push_str(&format!("**GitHub:** {url}\n"));
            }
            s.push_str("**Plan:** _none yet \u{2014} replace this with `claude-notes/plans/YYYY-MM-DD-<name>.md` once you create the plan file._\n");
            s.push_str("**Skill:** `/investigate-beads` continues this worktree's work.\n");
            s.push('\n');
            s.push_str(&format!(
                "Run `braid show {id}` for current status and notes.\n"
            ));
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
                "**Braid:** _none yet \u{2014} run `braid search {number}` to find an existing strand, or `braid create` to file one, then replace this line with the bd- id._\n"
            ));
            s.push_str("**Plan:** _none yet \u{2014} replace this with `claude-notes/plans/YYYY-MM-DD-<name>.md` once you create the plan file._\n");
            s.push_str("**Skill:** `/triage` continues the investigation; file a braid strand once concrete work surfaces.\n");
            s
        }
        SectionKind::Upgrade { date } => {
            let mut s = String::new();
            s.push_str("# Worktree Context\n\n");
            s.push_str("This is a **worktree** of the q2 repository. Main repo: `../..`\n\n");
            s.push_str(&format!(
                "**Task:** Cargo dependency upgrade \u{2014} {date}\n"
            ));
            s.push_str("**Plan:** _none yet \u{2014} replace this with a plan file path if you create one._\n");
            s.push_str("**Skill:** `/upgrade-cargo-deps` continues this worktree's work.\n");
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

pub struct IssueMetadata {
    pub title: String,
    pub external_ref: Option<String>,
    /// The strand's parent epic, when this is a sub-task. Resolved in
    /// two steps because braid's `show --json` does not enrich
    /// dependency entries with the target's title/status: first the
    /// child's `dependencies` map yields the parent's id (the first
    /// entry whose `type` is `parent-child`), then a second
    /// `braid show <parent>` supplies the parent's title and status.
    /// Used by the default-base warning (bd-ojtq) — if the parent epic
    /// is open, the user probably meant to branch off an integration
    /// branch rather than `main`.
    pub parent_epic: Option<ParentEpic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParentEpic {
    pub id: String,
    pub title: String,
    pub status: String,
}

/// Spawn `braid show <id> --json` and return its stdout. Shared by the
/// child-strand fetch and the follow-up parent-epic fetch.
fn braid_show_json(id: &str) -> Result<String> {
    let output = Command::new("braid")
        .args(["show", id, "--json"])
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                anyhow::anyhow!(
                    "braid is required \u{2014} install from https://github.com/cscheid/braid (or `curl -fsSL .../install.sh | bash`); see project README"
                )
            } else {
                anyhow::Error::new(e).context("spawning `braid show`")
            }
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("braid show {id} failed:\n{stderr}");
    }

    String::from_utf8(output.stdout)
        .with_context(|| format!("`braid show {id} --json` produced non-UTF-8 output"))
}

pub fn fetch_issue_metadata(id: &str) -> Result<IssueMetadata> {
    let parsed = parse_strand(id, &braid_show_json(id)?)?;

    // braid's dependency entries don't carry the target's title/status,
    // so resolve the parent epic with a second `braid show`. This is
    // best-effort: the parent-epic warning is informational, so a
    // failed/garbled parent fetch degrades to "no parent" rather than
    // aborting worktree creation.
    let parent_epic = match parsed.parent_id {
        Some(parent_id) => {
            match braid_show_json(&parent_id).and_then(|s| parse_strand(&parent_id, &s)) {
                Ok(parent) => Some(ParentEpic {
                    id: parent_id,
                    title: parent.title,
                    status: parent.status,
                }),
                Err(e) => {
                    eprintln!("note: could not resolve parent epic {parent_id}: {e:#}");
                    None
                }
            }
        }
        None => None,
    };

    Ok(IssueMetadata {
        title: parsed.title,
        external_ref: parsed.external_ref,
        parent_epic,
    })
}

/// The fields of one `braid show --json` object that the worktree
/// tooling cares about. `parent_id` is the target of the first
/// `parent-child` dependency, if any — the parent's title/status are
/// fetched separately (braid does not enrich dependency entries).
pub struct ParsedStrand {
    pub title: String,
    pub status: String,
    pub external_ref: Option<String>,
    pub parent_id: Option<String>,
}

/// Parse a single `braid show --json` object. Split from
/// [`fetch_issue_metadata`] so unit tests can drive it with fixture
/// JSON without spawning the real `braid` binary.
///
/// braid's `show --json` is a single object (not an array, as beads'
/// was) whose `dependencies` is a **keyed map** —
/// `{"<target>:<type>": {"depends_on_id": ..., "type": ...}}` — rather
/// than an enriched array.
pub fn parse_strand(id: &str, stdout: &str) -> Result<ParsedStrand> {
    let v: serde_json::Value = serde_json::from_str(stdout)
        .with_context(|| format!("parsing JSON from `braid show {id} --json`"))?;

    let title = v
        .get("title")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("`braid show {id}` JSON missing `title` field"))?
        .to_string();

    let status = v
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    let external_ref = v
        .get("external_ref")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    // First `parent-child` edge in the `dependencies` map gives the
    // parent epic's id. Multiple parents are unusual; if present we
    // just take one (map order is unspecified) — the warning is
    // informational, not load-bearing.
    let parent_id = v
        .get("dependencies")
        .and_then(|v| v.as_object())
        .into_iter()
        .flatten()
        .find_map(|(_key, dep)| {
            if dep.get("type").and_then(|v| v.as_str())? != "parent-child" {
                return None;
            }
            dep.get("depends_on_id")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        });

    Ok(ParsedStrand {
        title,
        status,
        external_ref,
        parent_id,
    })
}

/// Build the user-facing warning shown when `--base` was left at its
/// default but the issue has an open parent epic. Returned as a
/// `String` so it's directly testable without capturing stderr.
pub fn default_base_warning(child_id: &str, parent: &ParentEpic) -> String {
    format!(
        "\nwarning: `--base` not specified \u{2014} falling back to `main`.\n  \
         {child_id} has parent {parent_id} ({status}): {parent_title}\n  \
         Sub-tasks of an open epic typically branch off the epic's integration\n  \
         branch (commonly `feature/<name>`). If that's what you meant, re-run\n  \
         with `--base <branch>`. Pass `--base main` explicitly to silence this.\n",
        child_id = child_id,
        parent_id = parent.id,
        status = parent.status,
        parent_title = parent.title,
    )
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

pub fn run(args: Args) -> Result<()> {
    // Always anchor new worktrees to the main repo root so the command works
    // identically from main, from another worktree, or from any subdirectory.
    let root = repo_root()?;

    // `args.base.is_none()` means the user didn't pass `--base`; we
    // fall back to `main` (the pre-bd-ojtq behaviour) but remember
    // *that* we fell back, so braid mode can warn when a parent epic
    // is open.
    let base_explicit = args.base.is_some();
    let base = args.base.clone().unwrap_or_else(|| "main".to_string());

    // Mode is enforced by clap::ArgGroup(required, single).
    let plan = if let Some(id) = args.beads_id.as_deref() {
        plan_braid(id, args.slug.as_deref(), &base, &root)?
    } else if let Some(n) = args.issue {
        plan_issue(n, args.slug.as_deref(), &base, &root)?
    } else if args.upgrade {
        plan_upgrade(args.slug.as_deref(), &base, &root)?
    } else {
        unreachable!("clap ArgGroup guarantees one mode is set");
    };

    // bd-ojtq: nudge the user when `--base` was implicit and this
    // sub-task has an open parent epic. The warning is informational;
    // the worktree gets created on `main` regardless. To silence the
    // warning, the user re-runs with `--base main` (or whatever
    // integration branch they meant).
    if !base_explicit
        && let (Some(id), Some(parent)) = (args.beads_id.as_deref(), plan.parent_epic.as_ref())
        && parent.status == "open"
    {
        eprintln!("{}", default_base_warning(id, parent));
    }

    git_worktree_add(&plan.branch, &plan.dir, &plan.base)?;

    // From here on, on any error we roll back the worktree+branch we just
    // created so a retry is not blocked by directory/branch collision.
    let post = (|| -> Result<()> {
        // Test-only injection point: lets the Phase E smoke test exercise the
        // rollback path without modifying production logic.
        if std::env::var("Q2_CREATE_WORKTREE_INJECT_FAIL").as_deref() == Ok("after_worktree_add") {
            anyhow::bail!("Q2_CREATE_WORKTREE_INJECT_FAIL=after_worktree_add (test hook)");
        }
        // No beads redirect: braid worktrees under `.worktrees/` resolve
        // the skein via the repo-root `.braid.toml` walk-up (or the
        // committed `.braid-project` marker + user config).
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

    print_summary(&plan);
    Ok(())
}

struct Plan {
    branch: String,
    dir: PathBuf,
    base: String,
    kind: SectionKind,
    /// bd-ojtq: when in braid mode, the strand's parent epic (if any),
    /// passed up so `run` can emit the default-base warning. `None`
    /// for issue / upgrade modes — those don't have epic structure.
    parent_epic: Option<ParentEpic>,
}

fn plan_braid(id: &str, slug_override: Option<&str>, base: &str, repo_root: &Path) -> Result<Plan> {
    let meta = fetch_issue_metadata(id)?;
    let slug = match slug_override {
        Some(s) => {
            validate_slug(s)?;
            s.to_string()
        }
        None => derive_slug(&meta.title)?,
    };
    let leaf = format!("{id}-{slug}");
    let github_url = parse_external_ref_to_github_url(meta.external_ref.as_deref());
    if github_url.is_none()
        && let Some(other) = meta
            .external_ref
            .as_deref()
            .filter(|s| !s.is_empty() && !s.starts_with("gh-"))
    {
        eprintln!("note: external_ref {other:?} is not a `gh-` reference; omitting GitHub line");
    }
    Ok(Plan {
        branch: strand_branch(&leaf),
        dir: repo_root.join(".worktrees").join(&leaf),
        base: base.to_string(),
        kind: SectionKind::Braid {
            id: id.to_string(),
            title: meta.title,
            github_url,
        },
        parent_epic: meta.parent_epic,
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
        parent_epic: None,
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
        parent_epic: None,
    })
}

fn print_summary(plan: &Plan) {
    println!("Created worktree: {}/", plan.dir.display());
    println!("  Branch:  {}", plan.branch);
    match &plan.kind {
        SectionKind::Braid {
            id,
            title,
            github_url,
        } => {
            println!("  Braid:   {id} \u{2014} {title}");
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
    println!("Next:");
    println!("  cd {}", plan.dir.display());
    println!();
    println!("  Open a Claude Code session there \u{2014} CLAUDE.local.md gives it the");
    println!("  worktree context (branch, braid/GitHub link, base). Copy whichever of");
    println!("  the prep commands below apply:");
    println!();
    println!("  # Once per machine (skip if already done)");
    println!(
        "  cargo xtask dev-setup                       # installs cargo-nextest, wasm-bindgen-cli"
    );
    println!();
    println!("  # Per worktree");
    println!("  cargo xtask verify --skip-hub-build         # confirm HEAD is green (Rust only)");
    println!("  npm install                                 # only if hub-client work is in scope");
    match &plan.kind {
        SectionKind::Braid { id, .. } => {
            println!();
            println!("  # Per braid strand (this worktree)");
            println!("  braid update {id} --status in_progress   # claim it");
            println!("  # `/investigate-beads` reloads context if you need it.");
        }
        SectionKind::Issue { .. } => {
            println!();
            println!("  # `/triage` continues the investigation \u{2014} it files a braid strand");
            println!("  # when concrete work surfaces.");
        }
        SectionKind::Upgrade { .. } => {
            println!();
            println!("  # `/upgrade-cargo-deps` continues this worktree's work.");
        }
    }
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

    // ──────────────────────────────────────────────────────────────
    // bd-ojtq: parent-epic detection in braid JSON +
    //          default-base warning text
    // ──────────────────────────────────────────────────────────────

    /// Helper: synthesize a `braid show --json` payload. braid returns
    /// a single object (not an array) whose `dependencies` is a keyed
    /// map `{"<target>:<type>": {depends_on_id, type, ...}}`. The real
    /// command emits more fields; the parser only reads title / status
    /// / external_ref / dependencies, so the fixtures stay tight.
    fn braid_json(title: &str, dependencies: serde_json::Value) -> String {
        serde_json::to_string(&serde_json::json!({
            "id": "bd-test",
            "title": title,
            "status": "open",
            "dependencies": dependencies,
        }))
        .unwrap()
    }

    /// Build a braid dependency map entry under the canonical
    /// `"<target>:<type>"` key.
    fn dep(target: &str, dep_type: &str) -> (String, serde_json::Value) {
        (
            format!("{target}:{dep_type}"),
            serde_json::json!({ "depends_on_id": target, "type": dep_type }),
        )
    }

    #[test]
    fn parse_strand_reads_title_and_no_parent_when_no_dependencies() {
        let json = braid_json("standalone task", serde_json::json!({}));
        let parsed = parse_strand("bd-test", &json).unwrap();
        assert_eq!(parsed.title, "standalone task");
        assert!(parsed.parent_id.is_none());
        assert!(parsed.external_ref.is_none());
        assert_eq!(parsed.status, "open");
    }

    #[test]
    fn parse_strand_extracts_parent_child_dep() {
        // Mirrors the real shape from `braid show bd-068k --json`: one
        // parent-child entry alongside a non-parent blocks edge. The
        // parent's title/status are NOT on the dependency entry (braid
        // does not enrich) — only the id is recovered here.
        let mut deps = serde_json::Map::new();
        for (k, v) in [
            dep("bd-sibling", "blocks"),
            dep("bd-parent", "parent-child"),
        ] {
            deps.insert(k, v);
        }
        let json = braid_json("Phase C.5 sub-task", serde_json::Value::Object(deps));
        let parsed = parse_strand("bd-test", &json).unwrap();
        assert_eq!(parsed.parent_id.as_deref(), Some("bd-parent"));
    }

    #[test]
    fn parse_strand_ignores_non_parent_edges() {
        // Strands with only `blocks` / `related` / `discovered-from`
        // edges should NOT report a parent.
        let mut deps = serde_json::Map::new();
        for (k, v) in [
            dep("bd-other", "discovered-from"),
            dep("bd-blocker", "blocks"),
        ] {
            deps.insert(k, v);
        }
        let json = braid_json(
            "task with non-parent edges",
            serde_json::Value::Object(deps),
        );
        let parsed = parse_strand("bd-test", &json).unwrap();
        assert!(
            parsed.parent_id.is_none(),
            "non-parent-child edges must not surface as a parent; got: {:?}",
            parsed.parent_id
        );
    }

    #[test]
    fn parse_strand_takes_some_parent_when_multiple_present() {
        // Defensive: if a sub-task has multiple parent-child edges
        // (unusual but legal), we surface one. braid's dependency map
        // has unspecified order, so we only assert that a real parent
        // is chosen — the warning is informational, not load-bearing.
        let mut deps = serde_json::Map::new();
        for (k, v) in [dep("bd-p1", "parent-child"), dep("bd-p2", "parent-child")] {
            deps.insert(k, v);
        }
        let json = braid_json("doubly parented", serde_json::Value::Object(deps));
        let parsed = parse_strand("bd-test", &json).unwrap();
        let parent = parsed.parent_id.expect("a parent id");
        assert!(
            parent == "bd-p1" || parent == "bd-p2",
            "chose a real parent-child target; got {parent}"
        );
    }

    #[test]
    fn parse_strand_reads_external_ref() {
        let json = serde_json::to_string(&serde_json::json!({
            "id": "bd-test",
            "title": "with external ref",
            "status": "in_progress",
            "external_ref": "gh-123",
            "dependencies": {},
        }))
        .unwrap();
        let parsed = parse_strand("bd-test", &json).unwrap();
        assert_eq!(parsed.external_ref.as_deref(), Some("gh-123"));
        assert_eq!(parsed.status, "in_progress");
    }

    #[test]
    fn default_base_warning_names_child_parent_and_branch_convention() {
        let parent = ParentEpic {
            id: "bd-kw93".to_string(),
            title: "q2 preview epic".to_string(),
            status: "open".to_string(),
        };
        let msg = default_base_warning("bd-kw93.7", &parent);
        // Child + parent IDs both surfaced.
        assert!(
            msg.contains("bd-kw93.7"),
            "child id named in warning: {msg}"
        );
        assert!(msg.contains("bd-kw93"), "parent id named in warning: {msg}");
        // Parent title surfaced (so the user can identify which epic).
        assert!(
            msg.contains("q2 preview epic"),
            "parent title surfaced: {msg}"
        );
        // Convention named so the user knows what to type.
        assert!(
            msg.contains("feature/"),
            "branch convention named so the user knows the recovery: {msg}"
        );
        // The override path is documented.
        assert!(
            msg.contains("--base"),
            "warning names the flag to silence/override: {msg}"
        );
    }

    #[test]
    fn strand_branch_uses_the_braid_prefix() {
        assert_eq!(
            strand_branch("bd-abcd-some-slug"),
            "braid/bd-abcd-some-slug"
        );
        let b = strand_branch("bd-x");
        assert!(
            b.starts_with("braid/"),
            "strand branch must use the braid/ prefix: {b}"
        );
        assert!(
            !b.starts_with("beads/"),
            "historical beads/ prefix must be gone: {b}"
        );
    }

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
        build_section(&SectionKind::Braid {
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
        // (Plain byte filter is fine for a small test buffer; no `bytecount` dep.)
        #[allow(clippy::naive_bytecount)]
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
        let s = build_section(&SectionKind::Braid {
            id: "bd-1d3e".into(),
            title: "Fix X".into(),
            github_url: Some("https://github.com/quarto-dev/q2/issues/42".into()),
        });
        assert!(s.starts_with(BEGIN_MARKER));
        assert!(s.trim_end().ends_with(END_MARKER));
        assert!(s.contains("**Braid:** bd-1d3e — Fix X"));
        assert!(s.contains("**GitHub:** https://github.com/quarto-dev/q2/issues/42"));
        assert!(s.contains("**Skill:** `/investigate-beads`"));
        assert!(s.contains("Run `braid show bd-1d3e`"));
        assert!(s.contains("Main repo: `../..`"));
    }

    #[test]
    fn section_beads_without_github_omits_line() {
        let s = build_section(&SectionKind::Braid {
            id: "bd-zzzz".into(),
            title: "T".into(),
            github_url: None,
        });
        assert!(!s.contains("**GitHub:**"));
        assert!(s.contains("**Braid:** bd-zzzz — T"));
        assert!(s.contains("**Skill:** `/investigate-beads`"));
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
        assert!(s.contains("**Braid:** _none yet"));
        assert!(s.contains("braid search 157"));
        assert!(s.contains("**Skill:** `/triage`"));
        assert!(!s.contains("**Braid:** bd-")); // no resolved braid id
    }

    #[test]
    fn section_upgrade() {
        let s = build_section(&SectionKind::Upgrade {
            date: "2026-05-11".into(),
        });
        assert!(s.contains("**Task:** Cargo dependency upgrade — 2026-05-11"));
        assert!(s.contains("**Skill:** `/upgrade-cargo-deps`"));
        assert!(!s.contains("**Braid:**"));
        assert!(!s.contains("**GitHub:**"));
    }

    #[test]
    fn section_strips_marker_from_title() {
        // A title that literally contains the END marker must not be interpolated
        // verbatim — `strip_managed_section` would otherwise pick it up as the
        // section terminator on the next run.
        let evil = format!("real title {END_MARKER} oops");
        let s = build_section(&SectionKind::Braid {
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
