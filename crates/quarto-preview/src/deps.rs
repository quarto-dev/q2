//! `GET /api/preview/deps?page=<rel>` endpoint (Phase D.6, bd-kw93.12).
//!
//! Returns the set of project-relative file paths that the page at
//! `<rel>` depends on — i.e. the files whose edits should cause the
//! active page to re-render. The SPA fetches this on boot and on
//! active-file change, caches it locally, and uses it to filter
//! incoming `onFileContent` callbacks so unrelated sibling edits no
//! longer trigger a re-render.
//!
//! ## How "dependency" is decided for the MVP
//!
//! The handler parses the page's qmd source into a Pandoc AST and
//! walks the *block-level* shortcodes for the same shape
//! [`IncludeExpansionStage`] uses:
//!
//!   - a `Block::Paragraph` or `Block::Plain` containing exactly one
//!     inline (`Plain` covers tight list items and table cells),
//!   - that inline is an `Inline::Shortcode`,
//!   - the shortcode's `name` is `"include"`,
//!   - the first positional argument is a string,
//!   - at **any block-list position** — top level or nested inside
//!     divs, blockquotes, list items, tables, … (bd-1fz3vh99).
//!
//! Reusing [`collect_include_paths`](quarto_core::stage::stages::collect_include_paths)
//! keeps both the recognition rules and the traversal in lock-step
//! with what the renderer actually treats as an include — no drift
//! between "this file is a dep" and "this include actually expands
//! at render time." Going
//! through the AST (rather than regex over raw text) is also what
//! the Q1→Q2 migration is about: operate on the parsed syntax, not
//! on substrings.
//!
//! ## Deliberately out of scope for this MVP
//!
//! - **Transitive includes.** If `a.qmd` includes `b.qmd` and
//!   `b.qmd` includes `c.qmd`, this endpoint returns `[b.qmd]` for
//!   `a.qmd`. A regression test pins single-hop semantics so a
//!   future transitive expansion is a deliberate change.
//! - **Image references.** `![](foo.png)` doesn't show up in the
//!   text-doc dep set. The SPA keeps `onBinaryContent` callbacks
//!   unfiltered for now — every binary edit bumps `contentTick`.
//! - **Bibliography / CSL paths.** Same reasoning as images.
//! - **The full [`ProjectDependencyGraph`].** That graph requires
//!   running per-page render pipelines to populate
//!   `DocumentProfile.body_link_targets` etc.; way more expensive
//!   than D.6 needs.
//!
//! Parse / IO errors get logged and downgraded to "no deps" — a
//! filter that misses a real dep is bad (stale render); a filter
//! that includes too many is just the pre-D.6 behaviour, so we'd
//! rather over-broadcast than fail closed.

use std::path::Path;

use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use quarto_core::stage::stages::collect_include_paths;
use quarto_error_reporting::DiagnosticMessageBuilder;
use quarto_hub::context::SharedContext;
use serde::{Deserialize, Serialize};

use crate::diagnostics;

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
    let source = match std::fs::read(&abs_path) {
        Ok(s) => s,
        Err(e) => {
            // bd-b9kzg: surface IO failures via the per-page sink
            // in addition to (instead of) the existing tracing
            // line. `emit` calls `tracing::warn!` itself, so this
            // remains a clean replacement when the sink is set,
            // and falls back to the original log line when it
            // isn't (e.g. unit tests that call the handler
            // without booting a server).
            if let Some(sink) = diagnostics::current_sink() {
                sink.emit(
                    &rel_path,
                    DiagnosticMessageBuilder::warning("Could not analyze includes")
                        .with_code("Q-PREVIEW-DEPS-1")
                        .problem(format!("Failed to read source for dep extraction: {e}"))
                        .build(),
                );
            } else {
                tracing::warn!(
                    rel_path = %rel_path,
                    error = %e,
                    "could not read source for dep extraction; returning empty",
                );
            }
            return Json(DepsResponse { deps: vec![] }).into_response();
        }
    };

    let deps = extract_include_deps(&source, &rel_path);
    Json(DepsResponse { deps }).into_response()
}

/// Extract include-shortcode dependencies from a qmd source.
///
/// Parses `source` into a Pandoc AST via `pampa::readers::qmd::read`,
/// then walks every block-list position for the same "is this an
/// include shortcode?" shape [`IncludeExpansionStage`] uses (via the
/// shared [`collect_include_paths`] walker, which mirrors the
/// expander's traversal exactly). Paths are resolved relative to
/// `page_rel`'s directory and emitted as forward-slash
/// project-relative strings, deduplicated and sorted.
///
/// On parse failure, returns an empty list — see the module-level
/// fail-open rationale.
///
/// Public so unit tests + a potential future debugging surface can
/// reach it without spinning up an axum router.
pub fn extract_include_deps(source: &[u8], page_rel: &str) -> Vec<String> {
    // Pampa's reader writes commentary to an output stream we don't
    // care about here; sink it. Source-location tracking is off
    // (`false` for the `track_source_locations` argument) since we
    // don't need spans for include-name extraction.
    let mut sink = std::io::sink();
    let parse_result = pampa::readers::qmd::read(
        source, false,    // loose
        page_rel, // filename, for any error messages
        &mut sink, false, // track_source_locations
        None,  // parent SourceInfo
    );

    let Ok((mut pandoc, _ast_context, _warnings)) = parse_result else {
        // bd-b9kzg: parse failures surface to the SPA via the
        // per-page sink in addition to the existing tracing line.
        // Pampa's parser is robust enough that this branch is
        // rarely hit in practice (the unit test
        // `pathological_input_does_not_panic` covers garbage
        // input), but when it IS hit, the user benefits from
        // seeing "include analysis failed" in the overlay rather
        // than the silent empty-deps fallback. As elsewhere,
        // `emit` calls `tracing::warn!` itself, so this is a
        // clean replacement when the sink is set.
        if let Some(diag_sink) = diagnostics::current_sink() {
            diag_sink.emit(
                page_rel,
                DiagnosticMessageBuilder::warning("Could not analyze includes")
                    .with_code("Q-PREVIEW-DEPS-2")
                    .problem(
                        "Document failed to parse during include extraction; \
                         dep-graph filter will fall back to broadcasting edits.",
                    )
                    .build(),
            );
        } else {
            tracing::warn!(
                page_rel = %page_rel,
                "pampa parse failed during dep extraction; returning no deps",
            );
        }
        return Vec::new();
    };

    let page_dir = Path::new(page_rel)
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();

    // `collect_include_paths` takes `&mut` so it can share the
    // expander's container-position accessor (see its docs); we own
    // this freshly-parsed AST, so that costs nothing.
    let mut deps: Vec<String> = collect_include_paths(&mut pandoc.blocks)
        .into_iter()
        .map(|raw| {
            // Resolve relative to the page's directory and emit a
            // forward-slash project-relative path. We don't
            // canonicalize against the filesystem — the SPA matches
            // these strings against paths from `onFileContent`,
            // which arrive in the same forward-slash form.
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

    fn deps_for(src: &str, page: &str) -> Vec<String> {
        extract_include_deps(src.as_bytes(), page)
    }

    #[test]
    fn returns_empty_for_no_includes() {
        let src = "# Hello\n\nNo shortcodes here.\n";
        assert!(deps_for(src, "index.qmd").is_empty());
    }

    #[test]
    fn unquoted_include() {
        let src = "# A\n\n{{< include x.qmd >}}\n";
        assert_eq!(deps_for(src, "index.qmd"), vec!["x.qmd"]);
    }

    #[test]
    fn double_quoted_include() {
        let src = "# A\n\n{{< include \"posts/intro.qmd\" >}}\n";
        assert_eq!(deps_for(src, "index.qmd"), vec!["posts/intro.qmd"]);
    }

    #[test]
    fn single_quoted_include() {
        let src = "# A\n\n{{< include 'data/setup.qmd' >}}\n";
        assert_eq!(deps_for(src, "index.qmd"), vec!["data/setup.qmd"]);
    }

    #[test]
    fn include_resolves_relative_to_page_dir() {
        // page = posts/post1.qmd; include = setup.qmd → posts/setup.qmd
        let src = "# Post\n\n{{< include setup.qmd >}}\n";
        assert_eq!(deps_for(src, "posts/post1.qmd"), vec!["posts/setup.qmd"]);
    }

    #[test]
    fn include_resolves_parent_dir() {
        // page = posts/post1.qmd; include = ../shared.qmd → shared.qmd
        let src = "# Post\n\n{{< include ../shared.qmd >}}\n";
        assert_eq!(deps_for(src, "posts/post1.qmd"), vec!["shared.qmd"]);
    }

    #[test]
    fn multiple_includes_deduped_and_sorted() {
        let src = "# Mix\n\n{{< include z.qmd >}}\n\n{{< include a.qmd >}}\n\n{{< include z.qmd >}}\n\n{{< include \"m.qmd\" >}}\n";
        assert_eq!(deps_for(src, "index.qmd"), vec!["a.qmd", "m.qmd", "z.qmd"]);
    }

    #[test]
    fn unrelated_shortcodes_are_ignored() {
        // Non-`include` shortcodes don't contribute deps. The AST
        // walker filters by `shortcode.name == "include"`, so
        // `{{< meta title >}}` etc. don't show up.
        let src = "# Other\n\n{{< meta title >}}\n";
        assert!(deps_for(src, "index.qmd").is_empty());
    }

    #[test]
    fn inline_include_mixed_with_text_does_not_count() {
        // `IncludeExpansionStage`'s rule: an include shortcode must
        // be the *only* inline in its paragraph. Mixed-with-text
        // doesn't qualify as a true include. This is the
        // canonical Quarto-renderer behaviour — pinning it here
        // keeps the dep filter in lock-step with the renderer.
        let src = "# A\n\nSome prose then {{< include foo.qmd >}} inline.\n";
        assert!(
            deps_for(src, "index.qmd").is_empty(),
            "an inline include doesn't expand at render time, so it isn't a dep"
        );
    }

    #[test]
    fn includes_nested_in_containers_are_deps() {
        // bd-1fz3vh99: the renderer expands includes at every
        // block-list position (divs, blockquotes, list items), so the
        // dep filter must see them too — otherwise nested includes
        // mean stale previews, the exact bug class this module
        // exists to prevent.
        let src = "# A\n\n\
            ::: {.callout-note}\n{{< include in-div.qmd >}}\n:::\n\n\
            > {{< include in-quote.qmd >}}\n\n\
            - {{< include in-list.qmd >}}\n- other\n";
        assert_eq!(
            deps_for(src, "index.qmd"),
            vec!["in-div.qmd", "in-list.qmd", "in-quote.qmd"]
        );
    }

    #[test]
    fn include_inside_code_block_is_not_a_dep() {
        // Shortcode syntax inside a fenced code block is just text;
        // the AST walker only sees a `Block::CodeBlock`, no
        // shortcode inlines. (The regex implementation we replaced
        // would have incorrectly matched this — that's the bug the
        // AST-based approach fixes.)
        let src = "# A\n\n```\n{{< include foo.qmd >}}\n```\n";
        assert!(deps_for(src, "index.qmd").is_empty());
    }

    #[test]
    fn single_hop_is_documented_limitation() {
        // a.qmd directly includes b.qmd. The extractor does NOT
        // recursively open b.qmd to look at its includes; single-hop
        // is the v1 contract. Future transitive expansion would be
        // a deliberate behaviour change.
        let src = "# A\n\n{{< include b.qmd >}}\n";
        assert_eq!(deps_for(src, "a.qmd"), vec!["b.qmd"]);
    }

    #[test]
    fn pathological_input_does_not_panic() {
        // The pampa parser is robust enough to handle invalid UTF-8
        // and arbitrary byte garbage without panicking, but the
        // contract this test pins is "never panics, never returns
        // an Err to the caller, always returns a Vec." If a future
        // pampa change makes some input genuinely return Err, the
        // fail-open `Ok(_) else return Vec::new()` branch keeps
        // this contract intact.
        let garbage = b"\xff\xfe\xfd not valid utf-8 at all";
        let _result: Vec<String> = extract_include_deps(garbage, "index.qmd");
        // No assertion needed beyond "the call returned." The type
        // system guarantees a Vec<String>; the contract is no panic.
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
