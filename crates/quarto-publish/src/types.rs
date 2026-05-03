//! Public types exchanged across the `PublishProvider` boundary.
//!
//! Design constraints (see `claude-notes/plans/2026-05-03-publish-command-and-gh-pages.md`):
//!
//! - **Serde-serializable.** Future WASM-JS bridge needs to ship
//!   these across the language boundary, and `--json` output relies on
//!   them too.
//! - **No captured closures, no non-`'static` lifetimes.** Provider
//!   methods receive references, but the concrete payloads are owned
//!   plain data.
//! - **No types from `quarto-core` exposed in provider-facing
//!   shapes.** A third-party provider should not need to import
//!   `ProjectContext` to consume these.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

/// What is being published.
///
/// `kind` is *informational* — it lets the renderer tweak staging
/// (e.g. wrap a single PDF in an iframe) — and does not change the
/// trait surface. Q2 always associates a project context with the
/// input, even for a single bare `.qmd`, so providers always see
/// `project_dir` set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishInput {
    /// Absolute path to the project root (the directory containing
    /// `_quarto.yml`, or the parent of a single-file render).
    pub project_dir: PathBuf,

    /// What kind of thing is being published.
    pub kind: PublishKind,

    /// Site title (resolved from `website.title` or from the project
    /// directory name). Used as a deploy summary label.
    pub title: String,

    /// URL-safe slug derived from `title`. Used by providers that
    /// host content under a per-site path (Quarto Pub, Netlify
    /// preview URLs).
    pub slug: String,

    /// Configured `website.site-url`, if any. Providers that
    /// publish to GitHub Pages prefer this over a derived URL.
    pub site_url: Option<String>,
}

/// Document kind being published.
///
/// In Phase 1 only `Site` is exercised end-to-end; `Document` is
/// reserved for follow-up work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PublishKind {
    Site,
    Document,
}

impl PublishKind {
    pub fn as_str(self) -> &'static str {
        match self {
            PublishKind::Site => "site",
            PublishKind::Document => "document",
        }
    }
}

/// User-experience knobs (CLI-shaped). All publish flows consult
/// these.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishUx {
    /// Whether to render before publishing (Q1: `--render` /
    /// `--no-render`). Default true.
    pub render: bool,
    /// Whether to allow interactive prompts. Default true; forced
    /// false under `--json`.
    pub prompt: bool,
    /// Whether to open a browser to the published URL after success.
    /// Default true; mutually exclusive with `--no-wait`.
    pub browser: bool,
    /// Whether to wait for the deployment to be live (provider-
    /// specific verification step, e.g. polling `.nojekyll` for
    /// gh-pages). Default true.
    pub wait: bool,
    /// Run the full prepare + render path but do **not** push or
    /// upload. Final outcome is a synthesized "would-have-published"
    /// record.
    pub dry_run: bool,
    /// Emit machine-readable output. When set, the top-level driver
    /// writes a single JSON `PublishOutcome` to stdout and emits
    /// NDJSON `PublishEvent` lines to stderr.
    pub json: bool,
}

impl Default for PublishUx {
    fn default() -> Self {
        Self {
            render: true,
            prompt: true,
            browser: true,
            wait: true,
            dry_run: false,
            json: false,
        }
    }
}

/// Persisted "where did we last publish this?" record. Q1-compatible
/// shape.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PublishRecord {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub url: Option<String>,
    /// Whether the source of the publish included code (some
    /// providers — e.g. Quarto Pub — distinguish "doc with code"
    /// from "doc without code").
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub code: Option<bool>,
}

/// Authentication context for a publish. For gh-pages there's only
/// the `Anonymous` variant.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum AccountToken {
    /// No credential required (gh-pages).
    Anonymous,
    /// Token sourced from an environment variable.
    Environment {
        name: String,
        server: Option<String>,
    },
    /// Token from the user's interactive auth flow, persisted in
    /// the credential store.
    Authorized {
        name: String,
        server: Option<String>,
        // The actual token bytes never appear in serialized form.
        #[serde(skip)]
        token: String,
    },
}

impl AccountToken {
    pub fn anonymous() -> Self {
        AccountToken::Anonymous
    }

    pub fn name(&self) -> &str {
        match self {
            AccountToken::Anonymous => "anonymous",
            AccountToken::Environment { name, .. } => name,
            AccountToken::Authorized { name, .. } => name,
        }
    }
}

/// Files that the renderer produced and the provider should publish.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishFiles {
    /// Directory the file paths are relative to.
    pub base_dir: PathBuf,
    /// "Front-page" file (typically `index.html`). Providers that
    /// serve a directory use this to set up redirects or default
    /// routing.
    pub root_file: String,
    /// Every file to be uploaded, relative to `base_dir`.
    pub files: Vec<String>,
}

/// Where a publish is heading. Exposed in the dry-run plan so the
/// user can see the destination before any side effects happen.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishDestination {
    /// Provider name (`"gh-pages"`, etc.).
    pub provider: String,
    /// Human-readable destination ("github.com/user/repo branch
    /// gh-pages", "netlify site SITE_ID", etc.).
    pub description: String,
    /// Production URL the deploy will be live at, if known
    /// pre-publish.
    pub url: Option<String>,
}

/// One step in a planned publish. Emitted by `prepare()` so a
/// `--dry-run` invocation can show the user exactly what would
/// happen.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum PublishAction {
    /// "Render the project."
    Render { project_dir: PathBuf },
    /// "Create the gh-pages branch (it does not exist on origin)."
    CreateRemoteBranch { branch: String },
    /// "Force-push commit X to origin/<branch>."
    PushBranch {
        remote: String,
        branch: String,
        commit: String,
    },
    /// "Upload N files (S bytes) to <destination>."
    UploadFiles { count: usize, bytes: u64 },
    /// "Wait for deployment to be live at <url>."
    WaitForDeploy { url: String, timeout_secs: u64 },
    /// Provider-specific note, free-form.
    Note { message: String },
}

/// Aggregated post-publish summary. Emitted in `PublishOutcome`,
/// surfaced both in human output and in the `--json` payload.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PublishSummary {
    /// Commit SHA of the published bytes (gh-pages, anything
    /// git-backed).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub commit: Option<String>,
    /// Total number of files uploaded.
    pub file_count: usize,
    /// Total byte count of files uploaded.
    pub bytes: u64,
}

/// Result of a successful publish.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishOutcome {
    /// Provider name, included for `--json` consumers that don't
    /// otherwise know the provider.
    pub provider: String,
    /// Persisted publish record (e.g. for `_publish.yml`). For
    /// gh-pages this is `None` — the gh-pages branch on origin is
    /// itself the persistent record.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub record: Option<PublishRecord>,
    /// Production URL (if known).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub url: Option<Url>,
    /// Admin/dashboard URL (Q1 parity; e.g. Netlify's admin URL).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub admin_url: Option<Url>,
    /// Aggregated summary (commit SHA, file count, bytes).
    pub summary: PublishSummary,
    /// True if `verify()` confirmed the deployment is live.
    pub verified: bool,
    /// True if the outcome is from a `--dry-run` invocation.
    pub dry_run: bool,
}

impl PublishOutcome {
    /// Synthesize an outcome for a `--dry-run` invocation, given the
    /// `PreparedPublish` that *would* have been committed.
    pub fn dry_run(provider: &str, summary: PublishSummary, url: Option<Url>) -> Self {
        Self {
            provider: provider.to_string(),
            record: None,
            url,
            admin_url: None,
            summary,
            verified: false,
            dry_run: true,
        }
    }
}

/// One progress event. Emitted by providers to the host (which
/// renders human-readable output, or NDJSON under `--json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum PublishEvent {
    /// "Starting render."
    RenderStart,
    /// "Rendered N of M."
    RenderProgress { rendered: usize, total: usize },
    /// "Render complete."
    RenderComplete,
    /// "Preparing to publish (provider-specific staging)."
    PrepareStart { provider: String },
    /// "Plan computed." (the actual plan rides in `actions`)
    Plan {
        provider: String,
        actions: Vec<PublishAction>,
    },
    /// "Pushing/uploading."
    CommitStart { provider: String },
    /// "Push/upload complete."
    CommitComplete { provider: String },
    /// "Polling for deploy."
    DeployWaiting { url: String },
    /// "Deploy verified live."
    DeployVerified { url: String },
    /// Free-form provider message, useful for things like the gh-
    /// pages "switch source branch" nudge.
    Note { message: String },
}

/// Errors a provider can surface.
///
/// Variants are stable: third-party tooling and `--json` consumers
/// pin on the variant name (mapped to a `code` string by the host).
#[derive(Debug, Error)]
pub enum PublishError {
    #[error("authorization failed for {provider}: {source}")]
    Unauthorized {
        provider: &'static str,
        source: anyhow::Error,
    },

    #[error("publish target not found for {provider}: {source}")]
    NotFound {
        provider: &'static str,
        source: anyhow::Error,
    },

    /// User-fixable precondition that prevents publishing (no git
    /// installed, no origin remote, etc.). The message is
    /// user-facing.
    #[error("unable to publish via {provider}: {message}")]
    UnableToPublish {
        provider: &'static str,
        message: String,
    },

    /// Wraps any other error. Non-stable variant — consumers should
    /// not pin on the inner type.
    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

impl PublishError {
    /// Stable error code suitable for machine-readable output.
    pub fn code(&self) -> &'static str {
        match self {
            PublishError::Unauthorized { .. } => "Q-PUBLISH-UNAUTHORIZED",
            PublishError::NotFound { .. } => "Q-PUBLISH-NOT-FOUND",
            PublishError::UnableToPublish { .. } => "Q-PUBLISH-UNABLE",
            PublishError::Other(_) => "Q-PUBLISH-OTHER",
        }
    }

    /// Provider name, when known.
    pub fn provider(&self) -> Option<&'static str> {
        match self {
            PublishError::Unauthorized { provider, .. }
            | PublishError::NotFound { provider, .. }
            | PublishError::UnableToPublish { provider, .. } => Some(provider),
            PublishError::Other(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publish_ux_default_is_interactive_with_browser_and_wait() {
        let ux = PublishUx::default();
        assert!(ux.render);
        assert!(ux.prompt);
        assert!(ux.browser);
        assert!(ux.wait);
        assert!(!ux.dry_run);
        assert!(!ux.json);
    }

    #[test]
    fn publish_record_round_trips_through_serde() {
        let r = PublishRecord {
            id: "gh-pages".into(),
            url: Some("https://example.com/".into()),
            code: None,
        };
        let json = serde_json::to_string(&r).unwrap();
        // `code: None` should be omitted from the JSON form.
        assert!(!json.contains("\"code\""), "got {json}");
        let parsed: PublishRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, r);
    }

    #[test]
    fn account_token_authorized_does_not_serialize_token_bytes() {
        let t = AccountToken::Authorized {
            name: "alice".into(),
            server: None,
            token: "super-secret".into(),
        };
        let json = serde_json::to_string(&t).unwrap();
        assert!(!json.contains("super-secret"), "got {json}");
    }

    #[test]
    fn publish_action_round_trips_through_serde() {
        let actions = vec![
            PublishAction::Render {
                project_dir: PathBuf::from("/tmp/site"),
            },
            PublishAction::CreateRemoteBranch {
                branch: "gh-pages".into(),
            },
            PublishAction::PushBranch {
                remote: "origin".into(),
                branch: "gh-pages".into(),
                commit: "deadbeef".into(),
            },
            PublishAction::UploadFiles {
                count: 12,
                bytes: 4096,
            },
        ];
        let json = serde_json::to_string(&actions).unwrap();
        let parsed: Vec<PublishAction> = serde_json::from_str(&json).unwrap();
        // Reserialize to compare canonically.
        let json2 = serde_json::to_string(&parsed).unwrap();
        assert_eq!(json, json2);
    }

    #[test]
    fn publish_error_codes_are_stable() {
        let e = PublishError::Unauthorized {
            provider: "gh-pages",
            source: anyhow::anyhow!("nope"),
        };
        assert_eq!(e.code(), "Q-PUBLISH-UNAUTHORIZED");
        assert_eq!(e.provider(), Some("gh-pages"));

        let e = PublishError::UnableToPublish {
            provider: "gh-pages",
            message: "no origin".into(),
        };
        assert_eq!(e.code(), "Q-PUBLISH-UNABLE");

        let e = PublishError::Other(anyhow::anyhow!("io error"));
        assert_eq!(e.code(), "Q-PUBLISH-OTHER");
        assert_eq!(e.provider(), None);
    }

    #[test]
    fn publish_kind_serializes_kebab_case() {
        let s = serde_json::to_string(&PublishKind::Site).unwrap();
        assert_eq!(s, "\"site\"");
        let s = serde_json::to_string(&PublishKind::Document).unwrap();
        assert_eq!(s, "\"document\"");
    }
}
