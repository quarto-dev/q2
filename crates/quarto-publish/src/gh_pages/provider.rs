//! `GhPagesProvider` — full publish flow.
//!
//! Mirrors Q1's `src/publish/gh-pages/gh-pages.ts`, restructured
//! into the `prepare → commit → verify` shape. The worktree
//! management is split from the trait method bodies for clarity.
//!
//! Key design choices vs. Q1:
//!
//! - **No mutation of the user's working directory.** Q1 stashes,
//!   checks out gh-pages, commits, pushes, then restores. We do
//!   everything inside a `git worktree`, so the user's main
//!   working tree is never touched.
//! - **First-publish path uses an orphan worktree.** Avoids the Q1
//!   pattern of "checkout --orphan" in the user's main repo.
//! - **Cleanup via Drop.** When `commit` is *not* called (e.g.
//!   under `--dry-run` or on a `prepare` error after the worktree
//!   is created), the worktree is removed automatically.

use std::path::{Path, PathBuf};

use async_trait::async_trait;

use crate::common::errors::unable_to_publish;
use crate::common::git::{
    GitError, git_rev_parse, git_user_identity_configured, run_git, run_git_allow_failure,
};
use crate::common::github::{github_context_for_publish, verify_context};
use crate::common::wait::{DeployCheck, DeployProbe, WaitConfig, WaitOutcome, wait_for_deploy};
use crate::host::PublishHost;
use crate::provider::{PreparedPublish, PublishProvider};
use crate::renderer::{PublishRenderFlags, PublishRenderer};
use crate::types::{
    AccountToken, PublishAction, PublishDestination, PublishError, PublishEvent, PublishInput,
    PublishOutcome, PublishRecord, PublishSummary, PublishUx,
};

pub const PROVIDER_NAME: &str = "gh-pages";
pub const PROVIDER_DESCRIPTION: &str = "GitHub Pages";
const WORKTREE_PREFIX: &str = "quarto-publish-worktree-";
const COMMIT_MESSAGE: &str = "Built site for gh-pages";

pub struct GhPagesProvider;

impl GhPagesProvider {
    pub fn new() -> Self {
        Self
    }
}

impl Default for GhPagesProvider {
    fn default() -> Self {
        Self::new()
    }
}

/// Per-publish state that crosses `prepare → commit`.
///
/// Held inside `PreparedPublish::provider_state` as a `Box<dyn Any>`
/// (downcast inside `commit`).
///
/// `Drop` removes the worktree if `commit` was not called — that's
/// what makes `--dry-run` leave no residue.
struct GhPagesState {
    /// Project root (the `cwd` we run git commands from for paths
    /// that aren't inside the worktree).
    project_dir: PathBuf,
    /// Where we built the deploy.
    worktree_path: PathBuf,
    /// Random id written into `.nojekyll`; used by `verify` to
    /// match against the live site.
    deploy_id: String,
    /// Site URL we'll point the user at on success.
    site_url: Option<String>,
    /// SHA the local gh-pages branch in the worktree resolves to
    /// after `prepare`.
    commit_sha: String,
    /// File count + bytes of what's in the worktree (for the
    /// summary line).
    file_count: usize,
    bytes: u64,
    /// True when origin/gh-pages did not exist before prepare —
    /// commit must `--set-upstream` on first push.
    is_first_publish: bool,
    /// Set to false after commit() so Drop becomes a no-op.
    keep: bool,
}

impl Drop for GhPagesState {
    fn drop(&mut self) {
        if self.keep {
            return;
        }
        // Best-effort cleanup. Errors are logged but never panic.
        let _ = run_git_allow_failure(
            &[
                "worktree",
                "remove",
                "--force",
                &self.worktree_path.to_string_lossy(),
            ],
            &self.project_dir,
        );
        let _ = std::fs::remove_dir_all(&self.worktree_path);
        // Also prune the local branch we created. Otherwise repeated
        // dry-run invocations would accumulate detached gh-pages
        // refs.
        let _ = run_git_allow_failure(&["branch", "-D", "gh-pages"], &self.project_dir);
    }
}

#[async_trait]
impl PublishProvider for GhPagesProvider {
    fn name(&self) -> &'static str {
        PROVIDER_NAME
    }

    fn description(&self) -> &'static str {
        PROVIDER_DESCRIPTION
    }

    async fn publish_record(
        &self,
        input: &PublishInput,
        _host: &dyn PublishHost,
    ) -> Result<Option<PublishRecord>, PublishError> {
        let ctx = github_context_for_publish(&input.project_dir, input.site_url.as_deref())
            .map_err(map_git_err)?;
        if ctx.gh_pages_remote {
            Ok(Some(PublishRecord {
                id: "gh-pages".to_string(),
                url: ctx.site_url.clone().or_else(|| ctx.origin_url.clone()),
                code: None,
            }))
        } else {
            Ok(None)
        }
    }

    async fn authorize_token(
        &self,
        input: &PublishInput,
        _host: &dyn PublishHost,
    ) -> Result<Option<AccountToken>, PublishError> {
        let ctx = github_context_for_publish(&input.project_dir, input.site_url.as_deref())
            .map_err(map_git_err)?;
        verify_context(&ctx, PROVIDER_NAME)?;
        Ok(Some(AccountToken::anonymous()))
    }

    async fn prepare(
        &self,
        _account: &AccountToken,
        input: &PublishInput,
        renderer: &dyn PublishRenderer,
        _ux: &PublishUx,
        host: &dyn PublishHost,
        _target: Option<&PublishRecord>,
    ) -> Result<PreparedPublish, PublishError> {
        let project_dir = input.project_dir.clone();
        let ctx = github_context_for_publish(&project_dir, input.site_url.as_deref())
            .map_err(map_git_err)?;
        verify_context(&ctx, PROVIDER_NAME)?;

        if !git_user_identity_configured(&project_dir).map_err(map_git_err)? {
            return Err(unable_to_publish(
                PROVIDER_NAME,
                "git user.name and/or user.email is not configured. \
                 Run `git config user.name \"Your Name\"` and \
                 `git config user.email \"you@example.com\"`.",
            ));
        }

        // Render before touching git state, so a render failure
        // doesn't leave a half-set-up worktree behind.
        host.emit(PublishEvent::RenderStart).await;
        let files = renderer
            .render(&PublishRenderFlags {
                site_url: ctx.site_url.clone(),
            })
            .await?;
        host.emit(PublishEvent::RenderComplete).await;

        // Sync any remote gh-pages branch state we know about. If
        // origin/gh-pages exists, fetch it so we can base our
        // worktree on it.
        if ctx.gh_pages_remote {
            // `git remote set-branches --add origin gh-pages` makes
            // sure subsequent fetches pull the branch even if the
            // user's clone was made with `--single-branch`.
            run_git_allow_failure(
                &["remote", "set-branches", "--add", "origin", "gh-pages"],
                &project_dir,
            )
            .map_err(io_to_publish_err)?;
            run_git(&["fetch", "origin", "gh-pages"], &project_dir).map_err(map_git_err)?;
        }

        // Allocate a worktree under the project's `.quarto/scratch`
        // dir so it's git-ignored by default and easy to find.
        let scratch = project_dir.join(".quarto").join("scratch");
        std::fs::create_dir_all(&scratch).map_err(|e| {
            unable_to_publish(
                PROVIDER_NAME,
                format!("could not create scratch dir {}: {e}", scratch.display()),
            )
        })?;
        cleanup_stale_worktrees(&project_dir, &scratch);

        let deploy_id = uuid::Uuid::new_v4().simple().to_string()[..12].to_string();
        let worktree_path = scratch.join(format!("{WORKTREE_PREFIX}{deploy_id}"));

        // Create the worktree. Two cases:
        //   - Remote gh-pages exists → track it.
        //   - First-publish → orphan worktree.
        if ctx.gh_pages_remote {
            run_git(
                &[
                    "worktree",
                    "add",
                    "--force",
                    "--track",
                    "-B",
                    "gh-pages",
                    &worktree_path.to_string_lossy(),
                    "origin/gh-pages",
                ],
                &project_dir,
            )
            .map_err(map_git_err)?;
        } else {
            run_git(
                &[
                    "worktree",
                    "add",
                    "--force",
                    "--orphan",
                    "-b",
                    "gh-pages",
                    &worktree_path.to_string_lossy(),
                ],
                &project_dir,
            )
            .map_err(map_git_err)?;
        }

        // Now we have a state struct that owns the worktree —
        // wrap immediately so that any error from this point
        // forward triggers Drop cleanup.
        let mut state = GhPagesState {
            project_dir: project_dir.clone(),
            worktree_path: worktree_path.clone(),
            deploy_id: deploy_id.clone(),
            site_url: ctx.site_url.clone(),
            commit_sha: String::new(),
            file_count: 0,
            bytes: 0,
            is_first_publish: !ctx.gh_pages_remote,
            keep: false,
        };

        // Strip the worktree clean (only matters when we're
        // tracking origin/gh-pages — we want a clean slate).
        if ctx.gh_pages_remote {
            // `git rm -r --quiet .` from an empty index is fine; we
            // run it only when there's content to remove.
            let _ = run_git_allow_failure(&["rm", "-r", "--quiet", "."], &state.worktree_path);
        }

        // Copy render output into the worktree.
        let copy_summary = copy_render_output(&files.base_dir, &files.files, &state.worktree_path)?;
        state.file_count = copy_summary.file_count;
        state.bytes = copy_summary.bytes;

        // Write the deploy id sentinel.
        let nojekyll_path = state.worktree_path.join(".nojekyll");
        std::fs::write(&nojekyll_path, &state.deploy_id).map_err(|e| {
            unable_to_publish(PROVIDER_NAME, format!("could not write .nojekyll: {e}"))
        })?;

        // Stage and commit inside the worktree.
        run_git(&["add", "-Af", "."], &state.worktree_path).map_err(map_git_err)?;
        run_git(
            &["commit", "--allow-empty", "-m", COMMIT_MESSAGE],
            &state.worktree_path,
        )
        .map_err(map_git_err)?;

        // Capture the commit SHA we just produced.
        let sha = git_rev_parse("HEAD", &state.worktree_path)
            .map_err(map_git_err)?
            .ok_or_else(|| {
                unable_to_publish(
                    PROVIDER_NAME,
                    "could not resolve worktree HEAD after commit",
                )
            })?;
        state.commit_sha = sha.clone();

        // Build the plan exposed to dry-run / JSON consumers.
        let mut plan = Vec::new();
        plan.push(PublishAction::Render {
            project_dir: project_dir.clone(),
        });
        if state.is_first_publish {
            plan.push(PublishAction::CreateRemoteBranch {
                branch: "gh-pages".to_string(),
            });
        }
        plan.push(PublishAction::UploadFiles {
            count: state.file_count + 1, // +1 for .nojekyll
            bytes: state.bytes + state.deploy_id.len() as u64,
        });
        plan.push(PublishAction::PushBranch {
            remote: "origin".to_string(),
            branch: "gh-pages".to_string(),
            commit: sha.clone(),
        });

        let destination = PublishDestination {
            provider: PROVIDER_NAME.to_string(),
            description: format!(
                "{} (branch gh-pages)",
                ctx.repo_url
                    .as_deref()
                    .or(ctx.origin_url.as_deref())
                    .unwrap_or("origin")
            ),
            url: state.site_url.clone(),
        };

        let prepared = PreparedPublish {
            provider: PROVIDER_NAME,
            staging_dir: state.worktree_path.clone(),
            files,
            destination,
            plan,
            provider_state: Box::new(state),
        };
        Ok(prepared)
    }

    async fn commit(
        &self,
        prepared: PreparedPublish,
        host: &dyn PublishHost,
    ) -> Result<PublishOutcome, PublishError> {
        let mut state = prepared
            .provider_state
            .downcast::<GhPagesState>()
            .map_err(|_| {
                unable_to_publish(
                    PROVIDER_NAME,
                    "internal error: prepared publish state was not a GhPagesState",
                )
            })?;

        // Push.
        let push_args: Vec<&str> = if state.is_first_publish {
            vec![
                "push",
                "--force",
                "--set-upstream",
                "origin",
                "HEAD:gh-pages",
            ]
        } else {
            vec!["push", "--force", "origin", "HEAD:gh-pages"]
        };
        run_git(&push_args, &state.worktree_path).map_err(map_git_err)?;

        // After a successful push, mark the state for normal
        // (non-Drop-cleanup) cleanup, then explicitly remove the
        // worktree to surface any failure.
        state.keep = true;
        run_git(
            &[
                "worktree",
                "remove",
                "--force",
                &state.worktree_path.to_string_lossy(),
            ],
            &state.project_dir,
        )
        .map_err(map_git_err)?;

        // Don't delete the local gh-pages branch on success — it
        // mirrors origin/gh-pages and is useful for inspection.

        let url = state
            .site_url
            .as_deref()
            .and_then(|u| url::Url::parse(u).ok());
        let outcome = PublishOutcome {
            provider: PROVIDER_NAME.to_string(),
            record: Some(PublishRecord {
                id: "gh-pages".to_string(),
                url: state.site_url.clone(),
                code: None,
            }),
            url,
            admin_url: None,
            summary: PublishSummary {
                commit: Some(state.commit_sha.clone()),
                deploy_id: Some(state.deploy_id.clone()),
                file_count: state.file_count + 1, // +1 for .nojekyll
                bytes: state.bytes + state.deploy_id.len() as u64,
            },
            verified: false,
            dry_run: false,
        };

        // Default-site nudge (`<user>.github.io` first publish):
        // tell the user they need to flip the source branch.
        if state.is_first_publish {
            if let Some(site_url) = state.site_url.as_deref() {
                if let Some(user) = default_site_user(site_url) {
                    host.emit(PublishEvent::Note {
                        message: format!(
                            "First publish to a default GitHub Pages site detected. \
                             You may need to set the source branch to gh-pages at \
                             https://github.com/{user}/{user}.github.io/settings/pages"
                        ),
                    })
                    .await;
                }
            }
        }

        Ok(outcome)
    }

    async fn verify(
        &self,
        outcome: &mut PublishOutcome,
        ux: &PublishUx,
        host: &dyn PublishHost,
    ) -> Result<(), PublishError> {
        if !ux.wait {
            return Ok(());
        }
        let Some(site_url) = outcome.url.as_ref().map(|u| u.to_string()) else {
            // Nothing to poll if we don't have a site URL.
            return Ok(());
        };
        let nojekyll_url = if site_url.ends_with('/') {
            format!("{site_url}.nojekyll")
        } else {
            format!("{site_url}/.nojekyll")
        };
        let Some(deploy_id) = outcome.summary.deploy_id.clone() else {
            // Provider didn't surface a deploy id — nothing to
            // probe against.
            return Ok(());
        };

        let probe = NoJekyllProbe {
            url: nojekyll_url.clone(),
            host,
            deploy_id,
        };

        host.emit(PublishEvent::DeployWaiting {
            url: nojekyll_url.clone(),
        })
        .await;

        let result = wait_for_deploy(&probe, WaitConfig::default()).await?;

        match result {
            WaitOutcome::Verified => {
                outcome.verified = true;
                host.emit(PublishEvent::DeployVerified { url: nojekyll_url })
                    .await;
            }
            WaitOutcome::Broken => {
                host.emit(PublishEvent::Note {
                    message: format!(
                        "Deploy poll failed for {nojekyll_url} \
                         (server returned a definitive error). \
                         Visit the URL to investigate."
                    ),
                })
                .await;
            }
            WaitOutcome::TimedOut => {
                host.emit(PublishEvent::Note {
                    message: format!(
                        "Deploy poll timed out for {nojekyll_url}. \
                         GitHub Pages deploys normally take a few minutes — \
                         check back shortly."
                    ),
                })
                .await;
            }
        }
        Ok(())
    }
}

/// Detect the `<user>` part of a default GitHub Pages site URL.
///
/// Matches exactly `https://<user>.github.io/` (trailing slash
/// optional, but no further path segments). A project-pages URL
/// like `https://<user>.github.io/<repo>/` returns `None` — it
/// already has a custom landing page and doesn't need the
/// "switch source branch" nudge.
fn default_site_user(site_url: &str) -> Option<String> {
    let after_scheme = site_url
        .strip_prefix("https://")
        .or_else(|| site_url.strip_prefix("http://"))?;
    // Must be exactly "<user>.github.io" with at most a trailing
    // slash — anything else means it's a project-pages URL.
    let host = match after_scheme.split_once('/') {
        Some((host, "")) => host,
        Some(_) => return None,
        None => after_scheme,
    };
    let user = host.strip_suffix(".github.io")?;
    if user.is_empty() {
        None
    } else {
        Some(user.to_string())
    }
}

/// HTTP-based probe for the `.nojekyll` sentinel.
struct NoJekyllProbe<'h> {
    url: String,
    host: &'h dyn PublishHost,
    deploy_id: String,
}

#[async_trait]
impl<'h> DeployProbe for NoJekyllProbe<'h> {
    async fn check(&self) -> Result<DeployCheck, PublishError> {
        let resp = match self.host.http_get(&self.url).await {
            Ok(r) => r,
            // A transient network error → keep polling.
            Err(_) => return Ok(DeployCheck::NotYet),
        };
        if resp.status == 200 {
            let body = resp.body_text();
            if body.trim() == self.deploy_id {
                return Ok(DeployCheck::Ready);
            }
            // 200 but old content — still propagating.
            Ok(DeployCheck::NotYet)
        } else if resp.status == 404 {
            // Pages hasn't picked up the deploy yet.
            Ok(DeployCheck::NotYet)
        } else {
            // 5xx etc. — definitively broken from the user's POV.
            Ok(DeployCheck::Failed)
        }
    }
}

/// Result of copying render output into the worktree.
struct CopySummary {
    file_count: usize,
    bytes: u64,
}

/// Copy each entry in `files` from `src_base/<file>` to
/// `dst_base/<file>`, creating parent directories as needed.
fn copy_render_output(
    src_base: &Path,
    files: &[String],
    dst_base: &Path,
) -> Result<CopySummary, PublishError> {
    let mut count = 0usize;
    let mut bytes = 0u64;
    for f in files {
        let src = src_base.join(f);
        let dst = dst_base.join(f);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                unable_to_publish(
                    PROVIDER_NAME,
                    format!(
                        "could not create dir {} for output file {f}: {e}",
                        parent.display()
                    ),
                )
            })?;
        }
        std::fs::copy(&src, &dst).map_err(|e| {
            unable_to_publish(
                PROVIDER_NAME,
                format!(
                    "could not copy output file {} → {}: {e}",
                    src.display(),
                    dst.display()
                ),
            )
        })?;
        bytes += std::fs::metadata(&dst).map(|m| m.len()).unwrap_or(0);
        count += 1;
    }
    Ok(CopySummary {
        file_count: count,
        bytes,
    })
}

/// Best-effort cleanup of leftover worktrees from previous runs.
///
/// Picks up any directory under `scratch` whose name starts with
/// `WORKTREE_PREFIX`, runs `git worktree remove --force`, then
/// removes the directory. Errors are silently ignored — we don't
/// want a stale worktree from a crashed previous run to block a
/// fresh publish.
fn cleanup_stale_worktrees(project_dir: &Path, scratch: &Path) {
    let Ok(entries) = std::fs::read_dir(scratch) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if !name.starts_with(WORKTREE_PREFIX) {
            continue;
        }
        let _ = run_git_allow_failure(
            &["worktree", "remove", "--force", &path.to_string_lossy()],
            project_dir,
        );
        let _ = std::fs::remove_dir_all(&path);
    }
}

fn map_git_err(e: GitError) -> PublishError {
    PublishError::UnableToPublish {
        provider: PROVIDER_NAME,
        message: e.to_string(),
    }
}

fn io_to_publish_err(e: std::io::Error) -> PublishError {
    PublishError::UnableToPublish {
        provider: PROVIDER_NAME,
        message: e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_and_description() {
        let p = GhPagesProvider::new();
        assert_eq!(p.name(), "gh-pages");
        assert_eq!(p.description(), "GitHub Pages");
    }

    #[test]
    fn does_not_require_server() {
        let p = GhPagesProvider::new();
        assert!(!p.requires_server());
    }

    #[test]
    fn requires_render_by_default() {
        let p = GhPagesProvider::new();
        assert!(p.requires_render());
    }

    #[test]
    fn default_site_user_recognises_user_pages() {
        assert_eq!(
            default_site_user("https://octocat.github.io/").as_deref(),
            Some("octocat")
        );
        assert_eq!(
            default_site_user("https://octocat.github.io").as_deref(),
            Some("octocat")
        );
    }

    #[test]
    fn default_site_user_returns_none_for_project_pages() {
        assert!(
            default_site_user("https://octocat.github.io/some-repo/").is_none(),
            "host part of project-pages URL should not look like a default site"
        );
    }

    #[test]
    fn default_site_user_returns_none_for_non_github_pages() {
        assert!(default_site_user("https://example.com/").is_none());
    }
}
