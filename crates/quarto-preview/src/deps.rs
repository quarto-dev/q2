//! `GET /api/preview/deps?page=<rel>` endpoint (Phase D.6, bd-kw93.12).
//!
//! Returns the set of project-relative file paths that the page at
//! `<rel>` depends on — i.e. the files whose edits should cause the
//! active page to re-render. The SPA fetches this on boot and on
//! active-file change, caches it locally, and uses it to filter
//! incoming `onFileContent` callbacks so unrelated sibling edits no
//! longer trigger a re-render.
//!
//! ## Scope of "dependency" for the MVP
//!
//! Extracts the `{{< include foo.qmd >}}` shortcode references that
//! appear in the page's raw qmd source. Includes are emitted as
//! project-relative paths (the regex result is relative to the
//! page's directory; we normalize against the project root before
//! returning).
//!
//! Deliberately out of scope for this MVP:
//!
//! - **Transitive includes.** If `a.qmd` includes `b.qmd` and
//!   `b.qmd` includes `c.qmd`, this endpoint returns `[b.qmd]` for
//!   `a.qmd`. A regression test in §D.6 pins single-hop semantics so
//!   a future transitive expansion is a deliberate change.
//! - **Image references.** `![](foo.png)` doesn't show up in the
//!   text-doc dep set. The SPA keeps `onBinaryContent` callbacks
//!   unfiltered for now — every binary edit bumps `contentTick`.
//! - **Bibliography / CSL paths.** Same reasoning as images.
//! - **The full [`ProjectDependencyGraph`].** That graph requires
//!   running per-page render pipelines to populate
//!   `DocumentProfile.body_link_targets` etc.; way more expensive
//!   than D.6 needs.
//!
//! Errors get logged + downgraded to "no deps" rather than a 5xx.
//! A filter that misses a real dep is bad (stale render); a filter
//! that includes too many is just the pre-D.6 behaviour. So we'd
//! rather over-broadcast than fail closed.

use std::path::Path;
use std::sync::LazyLock;

use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use quarto_hub::context::SharedContext;
use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct DepsQuery {
    pub page: String,
}

#[derive(Debug, Serialize)]
pub struct DepsResponse {
    /// Project-relative paths (forward slashes) the requested page
    /// directly includes. Single-hop; non-include channels (images,
    /// bibliography, etc.) are out of scope per the module doc.
    pub deps: Vec<String>,
}

/// Axum handler for `GET /api/preview/deps?page=<rel>`.
pub async fn deps_handler(
    State(ctx): State<SharedContext>,
    Query(query): Query<DepsQuery>,
) -> Response {
    let rel_path = query.page;

    // Path validation: must be tracked in the project index. Files
    // outside the project (or unknown paths) → 400.
    if !ctx.index().has_file(&rel_path) {
        return (
            StatusCode::BAD_REQUEST,
            format!("Path '{rel_path}' is not in the project index"),
        )
            .into_response();
    }

    let Some(project_root) = ctx.storage().project_root().map(|p| p.to_path_buf()) else {
        // Standalone mode (no project) — return empty deps.
        return Json(DepsResponse { deps: vec![] }).into_response();
    };
    let project_root = project_root.canonicalize().unwrap_or(project_root);

    let abs_path = project_root.join(&rel_path);
    let source = match std::fs::read_to_string(&abs_path) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(
                rel_path = %rel_path,
                error = %e,
                "could not read source for dep extraction; returning empty",
            );
            return Json(DepsResponse { deps: vec![] }).into_response();
        }
    };

    let deps = extract_include_deps(&source, &rel_path);
    Json(DepsResponse { deps }).into_response()
}

/// Extract include-shortcode dependencies from a qmd source string.
///
/// Returns the deduplicated, sorted list of project-relative paths
/// the source references via `{{< include ... >}}`. Paths are
/// resolved relative to the source page's directory and emitted as
/// forward-slash project-relative strings.
///
/// Public so unit tests + a potential future debugging surface can
/// reach it without spinning up an axum router.
pub fn extract_include_deps(source: &str, page_rel: &str) -> Vec<String> {
    // The shortcode accepts the included path with or without
    // quotes (single or double). The path itself can be any
    // non-quote, non-whitespace character. We anchor on the
    // opening `{{<` + `include` so we don't false-match other
    // shortcodes that mention "include" in body text.
    //
    // Examples matched:
    //   {{< include foo.qmd >}}
    //   {{< include "posts/intro.qmd" >}}
    //   {{< include 'data/setup.qmd' >}}
    //   {{< include shortcode=foo.qmd >}}   (named-arg form)
    static INCLUDE_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r#"\{\{<\s*include\s+(?:shortcode\s*=\s*)?(?:"([^"]+)"|'([^']+)'|(\S+))\s*>\}\}"#,
        )
        .expect("D.6 include regex compiles")
    });

    let page_dir = Path::new(page_rel)
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();

    let mut deps: Vec<String> = INCLUDE_RE
        .captures_iter(source)
        .filter_map(|cap| {
            // Three optional groups for the three quoting forms.
            cap.get(1)
                .or_else(|| cap.get(2))
                .or_else(|| cap.get(3))
                .map(|m| m.as_str().to_string())
        })
        .map(|raw| {
            // Resolve relative to the page's directory and emit
            // forward-slash relative path. We don't canonicalize
            // against the filesystem — the SPA matches against
            // paths it gets from `onFileContent`, which arrive in
            // the same forward-slash project-relative form.
            let joined = page_dir.join(&raw);
            normalize_forward_slash(&joined)
        })
        .collect();
    deps.sort();
    deps.dedup();
    deps
}

fn normalize_forward_slash(p: &Path) -> String {
    // Use components() to collapse `./` and resolve `..` against
    // earlier components. Doesn't touch the filesystem.
    let mut parts: Vec<String> = Vec::new();
    for c in p.components() {
        use std::path::Component;
        match c {
            Component::Normal(s) => parts.push(s.to_string_lossy().to_string()),
            Component::ParentDir => {
                parts.pop();
            }
            Component::CurDir => {}
            Component::Prefix(_) | Component::RootDir => {
                // Absolute path — shouldn't happen for include
                // references; fall back to the raw form so callers
                // notice the surprise.
                return p.to_string_lossy().replace('\\', "/");
            }
        }
    }
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_returns_empty_for_no_includes() {
        let src = "# Hello\n\nNo shortcodes here.\n";
        assert!(extract_include_deps(src, "index.qmd").is_empty());
    }

    #[test]
    fn extract_unquoted_include() {
        let src = "# A\n\n{{< include x.qmd >}}\n";
        assert_eq!(extract_include_deps(src, "index.qmd"), vec!["x.qmd"]);
    }

    #[test]
    fn extract_double_quoted_include() {
        let src = "# A\n\n{{< include \"posts/intro.qmd\" >}}\n";
        assert_eq!(
            extract_include_deps(src, "index.qmd"),
            vec!["posts/intro.qmd"]
        );
    }

    #[test]
    fn extract_single_quoted_include() {
        let src = "# A\n\n{{< include 'data/setup.qmd' >}}\n";
        assert_eq!(
            extract_include_deps(src, "index.qmd"),
            vec!["data/setup.qmd"]
        );
    }

    #[test]
    fn extract_includes_relative_to_page_dir() {
        // page = posts/post1.qmd; include = setup.qmd → posts/setup.qmd
        let src = "# Post\n\n{{< include setup.qmd >}}\n";
        assert_eq!(
            extract_include_deps(src, "posts/post1.qmd"),
            vec!["posts/setup.qmd"]
        );
    }

    #[test]
    fn extract_resolves_parent_dir_includes() {
        // page = posts/post1.qmd; include = ../shared.qmd → shared.qmd
        let src = "# Post\n\n{{< include ../shared.qmd >}}\n";
        assert_eq!(
            extract_include_deps(src, "posts/post1.qmd"),
            vec!["shared.qmd"]
        );
    }

    #[test]
    fn extract_multiple_includes_deduped_and_sorted() {
        let src = r#"# Mix

{{< include z.qmd >}}
{{< include a.qmd >}}
{{< include z.qmd >}}
{{< include "m.qmd" >}}
"#;
        assert_eq!(
            extract_include_deps(src, "index.qmd"),
            vec!["a.qmd", "m.qmd", "z.qmd"]
        );
    }

    #[test]
    fn extract_ignores_unrelated_shortcodes() {
        let src = r#"# Other

{{< meta title >}}
{{< embed video.mp4 >}}
Some inline ``{{ }}`` thing.
"#;
        assert!(extract_include_deps(src, "index.qmd").is_empty());
    }

    #[test]
    fn extract_handles_shortcode_named_arg_form() {
        // Hand-crafted edge: some authoring tools emit the named-arg
        // form `shortcode=path`. Accept it as a path.
        let src = "# A\n\n{{< include shortcode=foo.qmd >}}\n";
        assert_eq!(extract_include_deps(src, "index.qmd"), vec!["foo.qmd"]);
    }

    #[test]
    fn extract_is_single_hop_documented_limitation() {
        // a.qmd includes b.qmd; the function does NOT recursively
        // open b.qmd to find its includes. Single-hop is the v1
        // contract. Future transitive expansion is a deliberate
        // change (different method or option).
        let src_a = "# A\n\n{{< include b.qmd >}}\n";
        let deps_a = extract_include_deps(src_a, "a.qmd");
        assert_eq!(deps_a, vec!["b.qmd"]);
        // (We don't open b.qmd. If we did, we'd return [b.qmd, c.qmd].)
    }

    #[test]
    fn normalize_forward_slash_round_trips() {
        assert_eq!(
            normalize_forward_slash(Path::new("posts/intro.qmd")),
            "posts/intro.qmd"
        );
        assert_eq!(
            normalize_forward_slash(Path::new("a/b/../c.qmd")),
            "a/c.qmd"
        );
        assert_eq!(normalize_forward_slash(Path::new("./a.qmd")), "a.qmd");
    }
}
