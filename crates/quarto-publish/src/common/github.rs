//! GitHub context discovery.
//!
//! Detect git/repo state, derive a site URL from `remote.origin.url`
//! (honoring a `CNAME` file when present), and surface the values
//! the gh-pages provider needs.
//!
//! Direct port of Q1's `core/github.ts` (with the URL parsing
//! tightened to cover `github.com`, `*.github.io`, and arbitrary
//! enterprise hosts uniformly).

use std::path::Path;

use crate::common::errors::unable_to_publish;
use crate::common::git::{
    GitError, git_branch_exists, git_origin_url, git_remote_branch_exists,
    git_remote_origin_exists, git_rev_parse,
};
use crate::types::PublishError;

const GITHUB_COM: &str = "github.com";
const GITHUB_IO: &str = "github.io";

/// What the gh-pages provider needs to know about the local repo
/// and its origin.
#[derive(Debug, Clone, Default)]
pub struct GitHubContext {
    pub git: bool,
    pub repo: bool,
    pub origin_url: Option<String>,
    pub repo_url: Option<String>,
    pub gh_pages_remote: bool,
    pub gh_pages_local: bool,
    pub site_url: Option<String>,
    pub organization: Option<String>,
    pub repository: Option<String>,
}

/// Read GitHub context for `dir`.
///
/// Returns a partially-populated context as soon as it can't dig
/// further: no git → `{git: false, ..}`; no repo → `{git: true,
/// repo: false}`; etc. Callers verify the prerequisites they
/// actually need (see [`verify_context`]).
pub fn github_context(dir: &Path) -> Result<GitHubContext, GitError> {
    let mut ctx = GitHubContext::default();

    // Is `git` callable at all? (Q1 uses `which("git")`; we just
    // try a `--version` call. If git is missing, all our other
    // helpers will return spawn errors.)
    let git_available = std::process::Command::new("git")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !git_available {
        return Ok(ctx);
    }
    ctx.git = true;

    // Is `dir` inside a git repo?
    let rev_parse_works = std::process::Command::new("git")
        .arg("rev-parse")
        .current_dir(dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !rev_parse_works {
        return Ok(ctx);
    }
    ctx.repo = true;

    // Does it have an origin?
    if !git_remote_origin_exists(dir)? {
        return Ok(ctx);
    }
    ctx.origin_url = git_origin_url(dir)?;

    // gh-pages on remote?
    if let Some(present) = git_remote_branch_exists("gh-pages", dir)? {
        ctx.gh_pages_remote = present;
    }
    // If the remote check failed altogether, fall back to local.
    if !ctx.gh_pages_remote {
        ctx.gh_pages_local = git_branch_exists("gh-pages", dir)?
            || git_rev_parse("refs/heads/gh-pages", dir)?.is_some();
    }

    // Site URL — CNAME wins, else derive from origin URL.
    ctx.site_url = derive_site_url(dir, ctx.origin_url.as_deref());

    // org/repo split from origin URL, when it parses.
    if let Some(url) = ctx.origin_url.as_deref() {
        if let Some(info) = parse_origin(url) {
            ctx.repo_url = Some(info.repo_url);
            ctx.organization = Some(info.organization);
            ctx.repository = Some(info.repository);
        }
    }

    Ok(ctx)
}

/// Variant of [`github_context`] that overrides the derived
/// `site_url` with the project's configured `website.site-url` when
/// available. The provider should call this — `github_context`
/// itself is the lower-level primitive.
pub fn github_context_for_publish(
    dir: &Path,
    configured_site_url: Option<&str>,
) -> Result<GitHubContext, GitError> {
    let mut ctx = github_context(dir)?;
    if let Some(url) = configured_site_url {
        let trimmed = url.trim();
        if !trimmed.is_empty() {
            ctx.site_url = Some(trimmed.to_string());
        }
    }
    Ok(ctx)
}

/// Verify the prerequisites for `provider` publishing. Translates
/// missing pieces into user-facing errors.
pub fn verify_context(ctx: &GitHubContext, provider: &'static str) -> Result<(), PublishError> {
    if !ctx.git {
        return Err(unable_to_publish(
            provider,
            "git does not appear to be installed on this system",
        ));
    }
    if !ctx.repo {
        return Err(unable_to_publish(
            provider,
            "the target directory is not a git repository",
        ));
    }
    if ctx.origin_url.is_none() {
        return Err(unable_to_publish(
            provider,
            "the git repository does not have a remote 'origin'",
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OriginInfo {
    repo_url: String,
    organization: String,
    repository: String,
}

/// Parse `git@host:org/repo[.git]` and `https://host/org/repo[.git]`.
fn parse_origin(url: &str) -> Option<OriginInfo> {
    let url = url.trim();

    // SSH form: `git@host:org/repo[.git]` (also `user@host:...`).
    if let Some(rest) = url.strip_prefix("git@") {
        if let Some((host, path)) = rest.split_once(':') {
            return parse_path(host, path);
        }
    }
    if let Some((user_host, path)) = url.split_once(':') {
        // Only treat `user@host:` as SSH; bare `host:port/...` is
        // not what we mean.
        if user_host.contains('@') && !url.starts_with("http") {
            if let Some((_, host)) = user_host.split_once('@') {
                return parse_path(host, path);
            }
        }
    }

    // HTTP(S) form.
    for prefix in ["https://", "http://"] {
        if let Some(rest) = url.strip_prefix(prefix) {
            if let Some((host, path)) = rest.split_once('/') {
                return parse_path(host, path);
            }
        }
    }

    None
}

fn parse_path(host: &str, path: &str) -> Option<OriginInfo> {
    let path = path.trim_start_matches('/');
    let (org, repo) = path.split_once('/')?;
    if org.is_empty() || repo.is_empty() {
        return None;
    }
    let repo = repo.trim_end_matches(".git");
    if repo.is_empty() {
        return None;
    }
    let scheme = if host.contains("github") {
        "https"
    } else {
        "https"
    };
    Some(OriginInfo {
        repo_url: format!("{scheme}://{host}/{org}/{repo}/"),
        organization: org.to_string(),
        repository: repo.to_string(),
    })
}

/// Derive the live site URL.
///
/// Priority:
///
/// 1. `<dir>/CNAME` (custom domain) — used verbatim, with `https://`
///    prepended if needed.
/// 2. github.com/org/repo → `https://org.github.io/repo/` (or
///    `https://org.github.io/` for the user's root site).
/// 3. Other hosts → `None` (not derivable without explicit config).
fn derive_site_url(dir: &Path, origin_url: Option<&str>) -> Option<String> {
    let cname = dir.join("CNAME");
    if cname.exists() {
        if let Ok(contents) = std::fs::read_to_string(&cname) {
            let url = contents.trim();
            if !url.is_empty() {
                if url.starts_with("http://") || url.starts_with("https://") {
                    return Some(url.to_string());
                }
                return Some(format!("https://{url}"));
            }
        }
    }

    let info = parse_origin(origin_url?)?;
    // We only know how to derive a site URL for github.com (and
    // GitHub Enterprise hosts that use the same `<server>/<org>/<repo>`
    // → `<org>.<server-with-pages-suffix>` convention; for now we
    // limit derivation to github.com to avoid guessing wrong on
    // enterprise installs).
    let host = origin_url.and_then(|u| host_of(u)).unwrap_or_default();
    if host != GITHUB_COM {
        return None;
    }
    let domain = format!("{}.{}", info.organization, GITHUB_IO);
    if domain == info.repository {
        // User/org's root site.
        Some(format!("https://{domain}/"))
    } else {
        Some(format!("https://{domain}/{}/", info.repository))
    }
}

fn host_of(url: &str) -> Option<String> {
    let url = url.trim();
    if let Some(rest) = url.strip_prefix("git@") {
        if let Some((host, _)) = rest.split_once(':') {
            return Some(host.to_string());
        }
    }
    for prefix in ["https://", "http://"] {
        if let Some(rest) = url.strip_prefix(prefix) {
            if let Some((host, _)) = rest.split_once('/') {
                return Some(host.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    use crate::common::git::run_git;

    fn init_repo(dir: &Path) {
        run_git(&["init", "--initial-branch=main"], dir).unwrap();
        run_git(&["config", "user.name", "T"], dir).unwrap();
        run_git(&["config", "user.email", "t@e.com"], dir).unwrap();
    }

    fn add_origin(dir: &Path, url: &str) {
        run_git(&["remote", "add", "origin", url], dir).unwrap();
    }

    // ── parse_origin ────────────────────────────────────────────

    #[test]
    fn parse_origin_https_github_com() {
        let info = parse_origin("https://github.com/quarto-dev/quarto-cli.git").unwrap();
        assert_eq!(info.organization, "quarto-dev");
        assert_eq!(info.repository, "quarto-cli");
        assert_eq!(info.repo_url, "https://github.com/quarto-dev/quarto-cli/");
    }

    #[test]
    fn parse_origin_https_github_com_no_dot_git() {
        let info = parse_origin("https://github.com/quarto-dev/quarto-cli").unwrap();
        assert_eq!(info.repository, "quarto-cli");
    }

    #[test]
    fn parse_origin_ssh_github_com() {
        let info = parse_origin("git@github.com:quarto-dev/quarto-cli.git").unwrap();
        assert_eq!(info.organization, "quarto-dev");
        assert_eq!(info.repository, "quarto-cli");
    }

    #[test]
    fn parse_origin_returns_none_for_garbage() {
        assert!(parse_origin("not a url").is_none());
        assert!(parse_origin("").is_none());
        assert!(parse_origin("https://github.com/").is_none());
    }

    #[test]
    fn parse_origin_works_for_enterprise_hosts() {
        // We parse it (org/repo split is universal); deriving the
        // site URL for non-github.com is what's intentionally
        // limited.
        let info = parse_origin("git@ghe.example.com:team/proj.git").unwrap();
        assert_eq!(info.organization, "team");
        assert_eq!(info.repository, "proj");
    }

    // ── derive_site_url ─────────────────────────────────────────

    #[test]
    fn derive_site_url_from_user_root_site() {
        let temp = TempDir::new().unwrap();
        let url = derive_site_url(
            temp.path(),
            Some("https://github.com/octocat/octocat.github.io.git"),
        );
        assert_eq!(url.as_deref(), Some("https://octocat.github.io/"));
    }

    #[test]
    fn derive_site_url_from_project_repo() {
        let temp = TempDir::new().unwrap();
        let url = derive_site_url(
            temp.path(),
            Some("https://github.com/quarto-dev/quarto-cli.git"),
        );
        assert_eq!(
            url.as_deref(),
            Some("https://quarto-dev.github.io/quarto-cli/")
        );
    }

    #[test]
    fn derive_site_url_returns_none_for_non_github_host() {
        let temp = TempDir::new().unwrap();
        let url = derive_site_url(temp.path(), Some("git@gitlab.com:foo/bar.git"));
        assert!(url.is_none());
    }

    #[test]
    fn derive_site_url_honors_cname_with_scheme() {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join("CNAME"), "https://docs.example.com\n").unwrap();
        let url = derive_site_url(temp.path(), Some("https://github.com/foo/bar.git"));
        assert_eq!(url.as_deref(), Some("https://docs.example.com"));
    }

    #[test]
    fn derive_site_url_prepends_https_to_bare_cname() {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join("CNAME"), "docs.example.com").unwrap();
        let url = derive_site_url(temp.path(), Some("https://github.com/foo/bar.git"));
        assert_eq!(url.as_deref(), Some("https://docs.example.com"));
    }

    // ── github_context ──────────────────────────────────────────

    #[test]
    fn github_context_for_non_repo_dir() {
        let temp = TempDir::new().unwrap();
        let ctx = github_context(temp.path()).unwrap();
        assert!(ctx.git, "git binary should be detected");
        assert!(!ctx.repo, "tempdir is not a git repo");
        assert!(ctx.origin_url.is_none());
        assert!(!ctx.gh_pages_remote);
    }

    #[test]
    fn github_context_for_repo_without_origin() {
        let temp = TempDir::new().unwrap();
        init_repo(temp.path());
        let ctx = github_context(temp.path()).unwrap();
        assert!(ctx.git);
        assert!(ctx.repo);
        assert!(ctx.origin_url.is_none());
    }

    #[test]
    fn github_context_for_repo_with_origin_no_remote_branch() {
        let temp = TempDir::new().unwrap();
        init_repo(temp.path());
        // Origin URL points somewhere ls-remote can't reach (a
        // local nonexistent path) — the helper degrades gracefully.
        // Use a bare repo so the URL *does* resolve, just without
        // the gh-pages branch.
        let bare = TempDir::new().unwrap();
        run_git(&["init", "--bare"], bare.path()).unwrap();
        add_origin(temp.path(), &bare.path().to_string_lossy());

        let ctx = github_context(temp.path()).unwrap();
        assert!(ctx.git);
        assert!(ctx.repo);
        assert_eq!(
            ctx.origin_url.as_deref(),
            Some(bare.path().to_str().unwrap())
        );
        assert!(!ctx.gh_pages_remote);
        assert!(!ctx.gh_pages_local);
    }

    #[test]
    fn github_context_detects_gh_pages_remote() {
        // Set up a bare remote *with* a gh-pages branch.
        let bare = TempDir::new().unwrap();
        run_git(&["init", "--bare"], bare.path()).unwrap();
        let work = TempDir::new().unwrap();
        init_repo(work.path());
        fs::write(work.path().join("a.txt"), "a").unwrap();
        run_git(&["add", "a.txt"], work.path()).unwrap();
        run_git(&["commit", "-m", "x"], work.path()).unwrap();
        run_git(&["checkout", "--orphan", "gh-pages"], work.path()).unwrap();
        run_git(&["rm", "-rf", "."], work.path()).unwrap();
        fs::write(work.path().join("index.html"), "ok").unwrap();
        run_git(&["add", "index.html"], work.path()).unwrap();
        run_git(&["commit", "-m", "page"], work.path()).unwrap();
        add_origin(work.path(), &bare.path().to_string_lossy());
        run_git(&["push", "origin", "gh-pages"], work.path()).unwrap();

        // Fresh clone for the consumer.
        let clone = TempDir::new().unwrap();
        run_git(
            &[
                "clone",
                &bare.path().to_string_lossy(),
                &clone.path().to_string_lossy(),
            ],
            std::env::current_dir().unwrap().as_path(),
        )
        .unwrap();

        let ctx = github_context(clone.path()).unwrap();
        assert!(
            ctx.gh_pages_remote,
            "should detect gh-pages branch on origin"
        );
    }

    // ── verify_context ──────────────────────────────────────────

    #[test]
    fn verify_context_rejects_no_git() {
        let ctx = GitHubContext::default();
        let err = verify_context(&ctx, "gh-pages").unwrap_err();
        assert!(
            err.to_string()
                .contains("git does not appear to be installed")
        );
    }

    #[test]
    fn verify_context_rejects_no_repo() {
        let ctx = GitHubContext {
            git: true,
            ..Default::default()
        };
        let err = verify_context(&ctx, "gh-pages").unwrap_err();
        assert!(err.to_string().contains("not a git repository"));
    }

    #[test]
    fn verify_context_rejects_no_origin() {
        let ctx = GitHubContext {
            git: true,
            repo: true,
            ..Default::default()
        };
        let err = verify_context(&ctx, "gh-pages").unwrap_err();
        assert!(err.to_string().contains("does not have a remote 'origin'"));
    }

    #[test]
    fn github_context_for_publish_overrides_site_url() {
        let temp = TempDir::new().unwrap();
        init_repo(temp.path());
        let bare = TempDir::new().unwrap();
        run_git(&["init", "--bare"], bare.path()).unwrap();
        add_origin(temp.path(), &bare.path().to_string_lossy());
        let ctx =
            github_context_for_publish(temp.path(), Some("https://docs.example.com")).unwrap();
        assert_eq!(ctx.site_url.as_deref(), Some("https://docs.example.com"));
    }

    #[test]
    fn github_context_for_publish_keeps_derived_site_url_when_none_configured() {
        let temp = TempDir::new().unwrap();
        init_repo(temp.path());
        // Real github.com URL → derived site URL flows through.
        run_git(
            &["remote", "add", "origin", "https://github.com/foo/bar.git"],
            temp.path(),
        )
        .unwrap();
        let ctx = github_context_for_publish(temp.path(), None).unwrap();
        assert_eq!(ctx.site_url.as_deref(), Some("https://foo.github.io/bar/"));
    }

    #[test]
    fn verify_context_accepts_full_context() {
        let ctx = GitHubContext {
            git: true,
            repo: true,
            origin_url: Some("https://github.com/foo/bar.git".into()),
            ..Default::default()
        };
        verify_context(&ctx, "gh-pages").unwrap();
    }
}
