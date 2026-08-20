/*
 * project/aliases.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Resolving `aliases:` front-matter entries to redirect-stub paths.
 */

//! Pure resolution of `aliases:` entries to redirect-stub locations.
//!
//! An alias names an old URL that should redirect to the page
//! declaring it. Resolving one answers two questions — *where does the
//! stub file go* and *what does it link back to* — and needs nothing
//! but the alias string and the page's own `output_href`. No
//! filesystem, no other document, no `SystemRuntime`. That is why this
//! module is pure and lives apart from
//! [`super::website_post_render`], which owns the writing.
//!
//! Everything here operates on **output-dir-relative, forward-slash**
//! paths — the same currency as
//! [`DocumentProfile::output_href`](crate::document_profile::DocumentProfile::output_href).
//! `std::path` is deliberately avoided: these are URL paths, and on
//! Windows `Path` would introduce backslashes into strings that end up
//! in HTML `href` attributes.
//!
//! ## Resolution rules
//!
//! Ported from Quarto 1's `website-aliases.ts` (`toAnchor` /
//! `fixupHref` / `addRedirectsToMap`), whose behaviour a ported site
//! depends on:
//!
//! 1. An alias may carry a `#fragment`. It is split off first and
//!    becomes the stub's routing key; a fragment-less alias uses the
//!    empty key.
//! 2. A path ending in `/` names a directory, so the stub is its
//!    `index.html`.
//! 3. A path whose last segment has no extension is *also* a
//!    directory: `/moved` → `/moved/index.html`, not a file called
//!    `moved`. (This is the common case — 77 of the 106 aliases in the
//!    Posit Connect docs take one of these two branches.)
//! 4. A leading `/` makes the alias site-root-relative, resolved
//!    against the output directory. Anything else resolves against the
//!    directory of the declaring page's own output file.
//!
//! One rule is **not** Q1's: an alias that climbs above the output
//! directory is rejected ([`AliasError::EscapesOutputDir`]) rather
//! than writing a file outside the site. Q1 has no such guard.
//!
//! See `claude-notes/plans/2026-08-12-aliases-redirect-stubs.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

use quarto_source_map::SourceInfo;

use crate::document_profile::DocumentProfile;

/// An alias resolved against the page that declared it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAlias {
    /// Output-dir-relative path of the stub file to write, e.g.
    /// `"old-name.html"` or `"moved/index.html"`.
    pub stub_href: String,

    /// The fragment this alias routes, without the `#`. Empty for a
    /// fragment-less alias, which supplies the stub's default target.
    pub fragment: String,
}

/// Why an alias could not be resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AliasError {
    /// Normalizing the alias climbed above the output directory, so
    /// the stub would be written outside the site.
    EscapesOutputDir,
}

impl fmt::Display for AliasError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EscapesOutputDir => f.write_str("alias resolves outside the output directory"),
        }
    }
}

/// Resolve one alias against the output href of the page declaring it.
///
/// `page_output_href` is output-dir-relative and forward-slash
/// separated (e.g. `"current/index.html"`).
pub fn resolve_alias(alias: &str, page_output_href: &str) -> Result<ResolvedAlias, AliasError> {
    let (path, fragment) = split_fragment(alias);
    let path = fixup_href(path);

    let stub_href = if let Some(root_relative) = path.strip_prefix('/') {
        // Site-root-relative: resolved against the output directory,
        // so there is no base to prepend.
        normalize_segments("", root_relative)?
    } else {
        normalize_segments(parent_dir(page_output_href), &path)?
    };

    Ok(ResolvedAlias {
        stub_href,
        fragment: fragment.to_string(),
    })
}

/// The href a stub at `stub_href` should use to reach `target_href`.
///
/// Both are output-dir-relative; the result is relative to the stub's
/// own directory, so the site keeps working under any base path.
pub fn relative_href(stub_href: &str, target_href: &str) -> String {
    let from: Vec<&str> = split_nonempty(parent_dir(stub_href));
    let to: Vec<&str> = split_nonempty(target_href);

    let common = from
        .iter()
        .zip(to.iter())
        .take_while(|(a, b)| a == b)
        .count();

    let mut out = String::new();
    for _ in common..from.len() {
        out.push_str("../");
    }
    out.push_str(&to[common..].join("/"));
    out
}

/// Split an alias into its path and fragment (without the `#`).
///
/// Only the first `#` separates; a fragment may itself contain `#`,
/// matching Q1's `url.split("#")[1]` for the common case while not
/// truncating exotic ones.
fn split_fragment(alias: &str) -> (&str, &str) {
    match alias.find('#') {
        Some(i) => (&alias[..i], &alias[i + 1..]),
        None => (alias, ""),
    }
}

/// Turn a URL path that may name a directory into one that names a
/// file (Q1's `fixupHref`).
fn fixup_href(path: &str) -> String {
    if path.ends_with('/') {
        format!("{path}index.html")
    } else if has_extension(path) {
        path.to_string()
    } else {
        format!("{path}/index.html")
    }
}

/// Whether the last segment of `path` carries a file extension.
///
/// Mirrors Node's `extname`, which the Q1 implementation used: only
/// the final segment is considered, and a leading dot does not count
/// (`.gitignore` has no extension).
fn has_extension(path: &str) -> bool {
    let last = path.rsplit('/').next().unwrap_or("");
    match last.rfind('.') {
        Some(0) | None => false,
        Some(_) => true,
    }
}

/// The directory part of an output-dir-relative href, without a
/// trailing slash. `"a/b/c.html"` → `"a/b"`; `"c.html"` → `""`.
fn parent_dir(href: &str) -> &str {
    match href.rfind('/') {
        Some(i) => &href[..i],
        None => "",
    }
}

fn split_nonempty(path: &str) -> Vec<&str> {
    path.split('/').filter(|s| !s.is_empty()).collect()
}

/// Join `rel` onto `base` and resolve `.` / `..`, rejecting anything
/// that climbs above the output directory.
fn normalize_segments(base: &str, rel: &str) -> Result<String, AliasError> {
    let mut out: Vec<&str> = split_nonempty(base);

    for segment in rel.split('/') {
        match segment {
            // Empty segments come from `//` or a trailing slash that
            // `fixup_href` already accounted for.
            "" | "." => {}
            ".." => {
                if out.pop().is_none() {
                    return Err(AliasError::EscapesOutputDir);
                }
            }
            other => out.push(other),
        }
    }

    Ok(out.join("/"))
}

// ═══════════════════════════════════════════════════════════════════
// Planning: folding every page's aliases into a set of stubs
// ═══════════════════════════════════════════════════════════════════

/// One alias as the author wrote it, with enough provenance for a
/// diagnostic to point at it.
#[derive(Debug, Clone, PartialEq)]
pub struct AliasRef {
    /// The alias verbatim, e.g. `"/cookbook/runtime-caches/#delete"`.
    pub alias: String,
    /// Project-relative source path of the page that declared it.
    pub source_path: PathBuf,
    /// Span of the YAML scalar the alias came from. May be
    /// [`SourceInfo::default`] for synthetic profiles (tests), in
    /// which case the diagnostic degrades to a span-less message.
    pub source_info: SourceInfo,
}

/// A redirect stub the alias pass intends to write.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedStub {
    /// Output-dir-relative path of the stub file.
    pub stub_href: String,
    /// Fragment → href from this stub to the target page. Sorted by
    /// fragment with the default (`""`) first, so the rendered stub is
    /// byte-stable across runs.
    pub redirects: Vec<(String, String)>,
}

/// A reason the alias pass refuses to render.
///
/// Every variant is fatal. Quarto 1 tolerated the first two (warning
/// on one, silent on the other); we do not, because the failure they
/// produce is a redirect that points at the wrong page while the
/// author believes it points at the right one. See the plan's
/// §"Design decisions" 2.
#[derive(Debug, Clone, PartialEq)]
pub enum AliasConflict {
    /// The stub would be written over a page the project renders.
    OverwritesPage {
        alias: AliasRef,
        stub_href: String,
        /// Source path of the page that occupies it.
        page_source: PathBuf,
    },

    /// Two pages claim the same stub under the same fragment key.
    DuplicateClaim {
        first: AliasRef,
        second: AliasRef,
        stub_href: String,
        /// Empty for the default (fragment-less) route.
        fragment: String,
    },

    /// Several pages route fragments through one stub, but none
    /// claims the fragment-less URL, so there is no defensible answer
    /// for a visitor who arrives without a fragment.
    NoDefaultOwner {
        stub_href: String,
        /// Every page contributing a fragment, in declaration order.
        contributors: Vec<AliasRef>,
    },

    /// Two aliases resolve to paths that differ only by case.
    CaseOnlyAliasCollision { first: AliasRef, second: AliasRef },

    /// An alias resolves to a path that differs only by case from a
    /// rendered page's output path.
    CaseOnlyPageCollision {
        alias: AliasRef,
        stub_href: String,
        page_href: String,
        page_source: PathBuf,
    },

    /// The alias climbs above the output directory.
    EscapesOutputDir { alias: AliasRef },
}

/// The result of folding every page's aliases together.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AliasPlan {
    /// Stubs to write, sorted by `stub_href`.
    pub stubs: Vec<PlannedStub>,
    /// Everything wrong with the project's aliases. Non-empty means
    /// the render must fail — and it is deliberately *all* of them,
    /// not the first: a site with 69 aliasing files should not learn
    /// about its mistakes one render at a time.
    pub conflicts: Vec<AliasConflict>,
}

/// One page's claim on one (stub, fragment) route.
struct Claim {
    /// The stub path as *this* alias spelled it, case preserved. Two
    /// claims in one bucket whose spellings differ are a case-only
    /// collision, so the raw casing has to survive bucketing.
    stub_spelling: String,
    /// Fragment routed by this claim; empty for the default route.
    fragment: String,
    /// Href from the stub back to the claiming page.
    target_href: String,
    /// Who claimed it, and where they wrote it.
    who: AliasRef,
}

/// Fold every profile's `aliases:` into the stubs they imply.
///
/// Draft pages contribute nothing — leaking a draft's existence
/// through a live redirect is worse than over-eagerly hiding it — but
/// they still *occupy* their output path, so a stub may not be written
/// over one.
///
/// Case-folding is ASCII-only and applies on **every platform**, not
/// just case-insensitive ones: a Linux CI build that let a case-only
/// collision through would ship a site that breaks the moment it is
/// served from macOS or Windows. Non-ASCII characters compare exactly,
/// which avoids the surprises of full Unicode case folding (Turkish
/// dotless ı, ligatures, normalization forms) at the cost of missing
/// collisions no real filesystem is likely to conflate.
pub fn plan_alias_stubs(profiles: &[DocumentProfile]) -> AliasPlan {
    let mut plan = AliasPlan::default();

    // Every rendered page's output href, keyed by its folded form.
    // Drafts are included: they render, so they occupy a path.
    let mut pages: BTreeMap<String, (&str, &Path)> = BTreeMap::new();
    for profile in profiles {
        pages.insert(
            fold(&profile.output_href),
            (profile.output_href.as_str(), profile.source_path.as_path()),
        );
    }

    // Claims grouped by folded stub path, so a case-only collision
    // lands in the same bucket as what it collides with.
    let mut buckets: BTreeMap<String, Vec<Claim>> = BTreeMap::new();

    for profile in profiles {
        if profile.draft {
            continue;
        }
        for (i, alias) in profile.aliases.iter().enumerate() {
            let who = AliasRef {
                alias: alias.clone(),
                source_path: profile.source_path.clone(),
                // `alias_sources` is index-aligned with `aliases` by
                // contract, but a profile hand-built in a test may
                // omit it; degrade to a span-less diagnostic rather
                // than panicking.
                source_info: profile.alias_sources.get(i).cloned().unwrap_or_default(),
            };

            match resolve_alias(alias, &profile.output_href) {
                Ok(resolved) => {
                    let target_href = relative_href(&resolved.stub_href, &profile.output_href);
                    buckets
                        .entry(fold(&resolved.stub_href))
                        .or_default()
                        .push(Claim {
                            stub_spelling: resolved.stub_href,
                            fragment: resolved.fragment,
                            target_href,
                            who,
                        });
                }
                Err(AliasError::EscapesOutputDir) => {
                    plan.conflicts
                        .push(AliasConflict::EscapesOutputDir { alias: who });
                }
            }
        }
    }

    for (folded, claims) in buckets {
        if let Some(stub) = plan_one_bucket(&folded, claims, &pages, &mut plan.conflicts) {
            plan.stubs.push(stub);
        }
    }

    plan
}

/// Turn one folded bucket's claims into a stub, or into conflicts.
///
/// Returns `None` when the bucket cannot produce a defensible stub.
/// Every problem found is pushed to `conflicts`; the checks do not
/// short-circuit each other, so one render reports everything wrong
/// with the bucket.
fn plan_one_bucket(
    folded: &str,
    claims: Vec<Claim>,
    pages: &BTreeMap<String, (&str, &Path)>,
    conflicts: &mut Vec<AliasConflict>,
) -> Option<PlannedStub> {
    // The spelling the stub would actually be written under. All
    // claims in a bucket agree unless there is a case-only collision,
    // which the next check reports.
    let stub_spelling = claims[0].stub_spelling.clone();

    let mut fatal = false;

    // ── A stub may not displace a page the project renders ───────
    if let Some((page_href, page_source)) = pages.get(folded) {
        fatal = true;
        if *page_href == stub_spelling {
            conflicts.push(AliasConflict::OverwritesPage {
                alias: claims[0].who.clone(),
                stub_href: stub_spelling.clone(),
                page_source: page_source.to_path_buf(),
            });
        } else {
            conflicts.push(AliasConflict::CaseOnlyPageCollision {
                alias: claims[0].who.clone(),
                stub_href: stub_spelling.clone(),
                page_href: (*page_href).to_string(),
                page_source: page_source.to_path_buf(),
            });
        }
    }

    // ── Two aliases that differ only by case ─────────────────────
    if let Some(other) = claims
        .iter()
        .find(|c| c.stub_spelling != claims[0].stub_spelling)
    {
        fatal = true;
        conflicts.push(AliasConflict::CaseOnlyAliasCollision {
            first: claims[0].who.clone(),
            second: other.who.clone(),
        });
    }

    // ── Two pages claiming one route ─────────────────────────────
    // `BTreeMap` keeps the report order stable across runs.
    let mut by_fragment: BTreeMap<&str, &Claim> = BTreeMap::new();
    for claim in &claims {
        match by_fragment.get(claim.fragment.as_str()) {
            Some(first) if first.target_href != claim.target_href => {
                fatal = true;
                conflicts.push(AliasConflict::DuplicateClaim {
                    first: first.who.clone(),
                    second: claim.who.clone(),
                    stub_href: stub_spelling.clone(),
                    fragment: claim.fragment.clone(),
                });
            }
            // A page repeating its own alias is redundant, not
            // contradictory — the route it asks for is the one it
            // would get anyway.
            Some(_) => {}
            None => {
                by_fragment.insert(claim.fragment.as_str(), claim);
            }
        }
    }

    // ── Someone must own the fragment-less URL ───────────────────
    let default_target = match by_fragment.get("") {
        Some(claim) => claim.target_href.clone(),
        None => {
            // Every fragment here belongs to one page, so that page
            // is the unambiguous owner of the bare URL too. Q1 would
            // have sent this visitor to the site root.
            let targets: BTreeSet<&str> = by_fragment
                .values()
                .map(|c| c.target_href.as_str())
                .collect();
            if targets.len() == 1 {
                claims[0].target_href.clone()
            } else {
                // Several pages route fragments through this stub and
                // none claims the bare URL. Picking one would be an
                // arbitrary guess about author intent — the exact
                // class of silent wrongness this feature exists to
                // prevent — so ask instead. (No `fatal` flag: this
                // branch returns directly, and the other checks have
                // already recorded whatever else was wrong.)
                conflicts.push(AliasConflict::NoDefaultOwner {
                    stub_href: stub_spelling.clone(),
                    contributors: claims.iter().map(|c| c.who.clone()).collect(),
                });
                return None;
            }
        }
    };

    if fatal {
        return None;
    }

    let mut redirects: Vec<(String, String)> = by_fragment
        .iter()
        .filter(|(fragment, _)| !fragment.is_empty())
        .map(|(fragment, claim)| ((*fragment).to_string(), claim.target_href.clone()))
        .collect();
    // The default route leads, then fragments in sorted order — a
    // total order, so the rendered stub is byte-stable run to run.
    redirects.insert(0, (String::new(), default_target));

    Some(PlannedStub {
        stub_href: stub_spelling,
        redirects,
    })
}

/// The case-insensitive key two paths collide under.
///
/// ASCII-only by design — see [`plan_alias_stubs`].
fn fold(href: &str) -> String {
    href.to_ascii_lowercase()
}

/// Warn that `aliases:` has no effect in this project type.
///
/// Only website projects write redirect stubs. Saying so is the whole
/// point of the strand this feature closes: the original report was
/// not "the file is missing" but "the key vanished with no signal",
/// which cost a porting project 99 redirects before anyone noticed.
///
/// One diagnostic per declaring page, not per alias — a page with six
/// aliases has one problem, not six.
pub fn warn_aliases_ignored(
    index: &crate::project::index::ProjectIndex,
    diagnostics: &mut Vec<quarto_error_reporting::DiagnosticMessage>,
) {
    for profile in index.profiles() {
        if profile.aliases.is_empty() {
            continue;
        }
        diagnostics.push(
            quarto_error_reporting::DiagnosticMessageBuilder::warning(format!(
                "`aliases:` in `{}` has no effect in this project type",
                profile.source_path.display()
            ))
            .problem(
                "Redirect stubs are written only for `website` projects, so these aliases \
                 produce no redirects.",
            )
            .add_hint("Set `project: type: website` in `_quarto.yml`?")
            .build(),
        );
    }
}

// ═══════════════════════════════════════════════════════════════════
// Rendering the stub
// ═══════════════════════════════════════════════════════════════════

/// Render the redirect stub for a planned stub.
///
/// The shape deliberately improves on Quarto 1's, whose stub is a
/// JavaScript-only `window.location.replace` with no DOCTYPE, no
/// charset, and nothing at all for a client that does not run scripts.
/// Three additions:
///
/// - **`<noscript><meta http-equiv="refresh">`** so the redirect still
///   happens without JavaScript. It must stay *inside* `<noscript>`: a
///   bare meta refresh races the script and can win, sending a
///   fragment-carrying URL to the default target instead of that
///   fragment's own page — and those are frequently different pages.
/// - **`<link rel="canonical">`** so a crawler knows which page this
///   stands in for, rather than guessing at a JS soft-redirect.
/// - **A visible body link**, for the client that follows neither. It
///   is never seen by a scripted client: `location.replace` runs from
///   `<head>`, before the body is parsed.
///
/// The fragment map keeps Q1's routing semantics exactly, including
/// dropping the fragment when it has its own entry (the destination
/// page may not have that anchor) and preserving `location.search`.
pub fn render_stub(stub: &PlannedStub) -> String {
    // The planner guarantees a leading default entry.
    let default_target = stub
        .redirects
        .first()
        .map_or("/", |(_, target)| target.as_str());
    let escaped = html_escape(default_target);

    let map = stub
        .redirects
        .iter()
        .map(|(fragment, target)| format!("{}:{}", json_string(fragment), json_string(target)))
        .collect::<Vec<_>>()
        .join(",");

    format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<title>Redirect</title>
<link rel="canonical" href="{escaped}">
<noscript><meta http-equiv="refresh" content="0; url={escaped}"></noscript>
<script type="text/javascript">
  var redirects = {{{map}}};
  var hash = window.location.hash.replace(/^#/, '');
  var target = redirects[hash];
  if (!target) {{
    target = redirects[""] + window.location.hash;
  }}
  target = target + window.location.search;
  document.title = 'Redirect to ' + target;
  window.location.replace(target);
</script>
</head>
<body>
<p>This page has moved to <a href="{escaped}">{escaped}</a>.</p>
</body>
</html>
"#
    )
}

/// Escape a string for use in an HTML attribute or text node.
///
/// Q1's stub needed none of this — it put the href in exactly one
/// place, a JS string literal. Candidate B's three attribute contexts
/// make it necessary: an unescaped `&` in a path silently truncates
/// the href at the entity boundary.
fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            other => out.push(other),
        }
    }
    out
}

/// Encode a string as a JSON string literal, safe to embed in a
/// `<script>` element.
///
/// `serde_json` handles quoting and control characters; `<` is
/// additionally escaped so that no path — however strange — can close
/// the script element early with `</script>` or open a comment with
/// `<!--`. HTML does not decode entities inside `<script>`, so this
/// numeric escape is the only form that works here.
fn json_string(s: &str) -> String {
    serde_json::Value::String(s.to_string())
        .to_string()
        .replace('<', "\\u003c")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolved(alias: &str, page: &str) -> (String, String) {
        let r = resolve_alias(alias, page).expect("alias should resolve");
        (r.stub_href, r.fragment)
    }

    // ── fixup: which aliases name a directory ────────────────────

    #[test]
    fn trailing_slash_becomes_index_html() {
        assert_eq!(resolved("/moved/", "index.html").0, "moved/index.html");
    }

    #[test]
    fn extensionless_becomes_index_html() {
        // The single most common shape in the Connect docs corpus.
        assert_eq!(resolved("/moved", "index.html").0, "moved/index.html");
    }

    #[test]
    fn explicit_html_file_is_used_verbatim() {
        assert_eq!(resolved("/moved.html", "index.html").0, "moved.html");
    }

    #[test]
    fn extension_is_read_from_the_last_segment_only() {
        // `a.b` is a directory here, not an extension on the path.
        assert_eq!(resolved("/a.b/c", "index.html").0, "a.b/c/index.html");
    }

    #[test]
    fn leading_dot_is_not_an_extension() {
        // Node's `extname(".foo")` is `""`, so this names a directory.
        assert_eq!(resolved("/.foo", "index.html").0, ".foo/index.html");
    }

    #[test]
    fn a_non_html_extension_is_still_a_file() {
        // Q1 uses whatever extension is present rather than insisting
        // on `.html`, so a `.htm` alias from an older site round-trips.
        assert_eq!(resolved("/legacy.htm", "index.html").0, "legacy.htm");
    }

    // ── base: site-root-relative vs page-relative ────────────────

    #[test]
    fn absolute_alias_ignores_the_declaring_page() {
        assert_eq!(resolved("/old.html", "deep/nested/page.html").0, "old.html");
    }

    #[test]
    fn relative_alias_resolves_against_the_page_output_dir() {
        assert_eq!(
            resolved("../previous/index.html", "current/index.html").0,
            "previous/index.html"
        );
    }

    #[test]
    fn dot_slash_alias_stays_beside_the_page() {
        assert_eq!(
            resolved("./old.html", "current/index.html").0,
            "current/old.html"
        );
    }

    #[test]
    fn bare_relative_alias_stays_beside_the_page() {
        assert_eq!(
            resolved("old.html", "current/index.html").0,
            "current/old.html"
        );
    }

    #[test]
    fn redundant_separators_are_collapsed() {
        assert_eq!(resolved("/a//b.html", "index.html").0, "a/b.html");
    }

    // ── fragments ────────────────────────────────────────────────

    #[test]
    fn fragment_is_split_from_the_path() {
        assert_eq!(
            resolved("/hub/#deploy", "deploy/index.html"),
            ("hub/index.html".to_string(), "deploy".to_string())
        );
    }

    #[test]
    fn absent_fragment_is_the_empty_key() {
        assert_eq!(resolved("/hub.html", "p.html").1, "");
    }

    #[test]
    fn only_the_first_hash_separates() {
        // A second `#` belongs to the fragment; truncating it would
        // silently route a valid anchor to the wrong key.
        assert_eq!(resolved("/a.html#x#y", "p.html").1, "x#y");
    }

    #[test]
    fn empty_fragment_is_the_default_key() {
        // `/a.html#` carries no anchor, so it is the default target
        // rather than a distinct route.
        assert_eq!(resolved("/a.html#", "p.html").1, "");
    }

    // ── escapes ──────────────────────────────────────────────────

    #[test]
    fn relative_alias_climbing_past_the_root_is_rejected() {
        assert_eq!(
            resolve_alias("../escaped.html", "index.html"),
            Err(AliasError::EscapesOutputDir)
        );
    }

    #[test]
    fn absolute_alias_climbing_past_the_root_is_rejected() {
        assert_eq!(
            resolve_alias("/../escaped.html", "index.html"),
            Err(AliasError::EscapesOutputDir)
        );
    }

    #[test]
    fn climbing_within_the_output_dir_is_allowed() {
        // Two levels up from `a/b/page.html` lands back at the root,
        // which is inside the site.
        assert_eq!(
            resolved("../../top.html", "a/b/page.html").0,
            "top.html",
            "climbing to the output root is legal; only past it is not"
        );
    }

    // ── relative_href: what the stub links back to ───────────────

    #[test]
    fn root_stub_links_directly_to_a_nested_page() {
        assert_eq!(
            relative_href("old-name.html", "current/index.html"),
            "current/index.html"
        );
    }

    #[test]
    fn nested_stub_climbs_back_up() {
        assert_eq!(
            relative_href("previous/index.html", "current/index.html"),
            "../current/index.html"
        );
    }

    #[test]
    fn stub_and_page_in_the_same_directory() {
        assert_eq!(relative_href("docs/old.html", "docs/new.html"), "new.html");
    }

    #[test]
    fn deeply_nested_stub_shares_a_prefix_with_the_page() {
        assert_eq!(
            relative_href("a/b/c/old.html", "a/b/d/new.html"),
            "../d/new.html"
        );
    }

    #[test]
    fn nested_stub_links_to_a_root_page() {
        assert_eq!(
            relative_href("moved/index.html", "index.html"),
            "../index.html"
        );
    }

    #[test]
    fn relative_href_never_uses_backslashes() {
        // These strings land in HTML `href` attributes; a Windows
        // separator here would produce a broken link on the one
        // platform the tests are least likely to run on.
        let href = relative_href("a/b/old.html", "c/d/new.html");
        assert!(!href.contains('\\'), "got {href}");
        assert_eq!(href, "../../c/d/new.html");
    }

    // ── planning ─────────────────────────────────────────────────

    /// A profile with just the fields the planner reads.
    fn page(source: &str, output_href: &str, aliases: &[&str]) -> DocumentProfile {
        DocumentProfile {
            source_path: PathBuf::from(source),
            output_href: output_href.to_string(),
            aliases: aliases.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    fn draft(source: &str, output_href: &str, aliases: &[&str]) -> DocumentProfile {
        DocumentProfile {
            draft: true,
            ..page(source, output_href, aliases)
        }
    }

    fn stub_paths(plan: &AliasPlan) -> Vec<&str> {
        plan.stubs.iter().map(|s| s.stub_href.as_str()).collect()
    }

    #[test]
    fn plan_is_empty_without_aliases() {
        let plan = plan_alias_stubs(&[page("index.qmd", "index.html", &[])]);
        assert_eq!(plan, AliasPlan::default());
    }

    #[test]
    fn plan_maps_each_alias_to_its_own_stub() {
        let plan = plan_alias_stubs(&[
            page("index.qmd", "index.html", &[]),
            page(
                "current/index.qmd",
                "current/index.html",
                &["/old-name.html", "../previous/index.html"],
            ),
        ]);
        assert!(plan.conflicts.is_empty(), "{:?}", plan.conflicts);
        assert_eq!(stub_paths(&plan), ["old-name.html", "previous/index.html"]);
        assert_eq!(
            plan.stubs[0].redirects,
            [(String::new(), "current/index.html".to_string())]
        );
        assert_eq!(
            plan.stubs[1].redirects,
            [(String::new(), "../current/index.html".to_string())]
        );
    }

    #[test]
    fn plan_merges_fragments_from_two_pages_into_one_stub() {
        // The Connect-docs shape: two pages route different fragments
        // through one old URL, and one of them owns the bare URL.
        let plan = plan_alias_stubs(&[
            page(
                "build/index.qmd",
                "build/index.html",
                &["/hub", "/hub/#image"],
            ),
            page("deploy/index.qmd", "deploy/index.html", &["/hub/#deploy"]),
        ]);
        assert!(plan.conflicts.is_empty(), "{:?}", plan.conflicts);
        assert_eq!(stub_paths(&plan), ["hub/index.html"]);
        assert_eq!(
            plan.stubs[0].redirects,
            [
                (String::new(), "../build/index.html".to_string()),
                ("deploy".to_string(), "../deploy/index.html".to_string()),
                ("image".to_string(), "../build/index.html".to_string()),
            ],
            "default first, then fragments sorted"
        );
    }

    #[test]
    fn plan_gives_a_lone_fragment_stub_a_default_route() {
        // One page, one fragment, no bare alias: that page is the
        // unambiguous owner of the bare URL. Q1 would have sent a
        // fragment-less visitor to the site root.
        let plan = plan_alias_stubs(&[page("p/index.qmd", "p/index.html", &["/old.html#sec"])]);
        assert!(plan.conflicts.is_empty(), "{:?}", plan.conflicts);
        assert_eq!(
            plan.stubs[0].redirects,
            [
                (String::new(), "p/index.html".to_string()),
                ("sec".to_string(), "p/index.html".to_string()),
            ]
        );
    }

    #[test]
    fn plan_refuses_when_no_page_owns_the_bare_url() {
        // Two pages, fragments only, no bare alias. Choosing an owner
        // would be a guess about intent.
        let plan = plan_alias_stubs(&[
            page("a/index.qmd", "a/index.html", &["/hub/#one"]),
            page("b/index.qmd", "b/index.html", &["/hub/#two"]),
        ]);
        assert!(plan.stubs.is_empty(), "no stub without an owner");
        match plan.conflicts.as_slice() {
            [
                AliasConflict::NoDefaultOwner {
                    stub_href,
                    contributors,
                },
            ] => {
                assert_eq!(stub_href, "hub/index.html");
                assert_eq!(contributors.len(), 2);
            }
            other => panic!("expected NoDefaultOwner, got {other:?}"),
        }
    }

    #[test]
    fn plan_rejects_a_stub_over_a_rendered_page() {
        let plan = plan_alias_stubs(&[
            page("index.qmd", "index.html", &[]),
            page("p/index.qmd", "p/index.html", &["/index.html"]),
        ]);
        assert!(plan.stubs.is_empty());
        match plan.conflicts.as_slice() {
            [
                AliasConflict::OverwritesPage {
                    stub_href,
                    page_source,
                    ..
                },
            ] => {
                assert_eq!(stub_href, "index.html");
                assert_eq!(page_source, Path::new("index.qmd"));
            }
            other => panic!("expected OverwritesPage, got {other:?}"),
        }
    }

    #[test]
    fn plan_rejects_two_pages_claiming_one_route() {
        let plan = plan_alias_stubs(&[
            page("one/index.qmd", "one/index.html", &["/shared.html"]),
            page("two/index.qmd", "two/index.html", &["/shared.html"]),
        ]);
        assert!(plan.stubs.is_empty());
        match plan.conflicts.as_slice() {
            [
                AliasConflict::DuplicateClaim {
                    first,
                    second,
                    fragment,
                    ..
                },
            ] => {
                assert_eq!(first.source_path, Path::new("one/index.qmd"));
                assert_eq!(second.source_path, Path::new("two/index.qmd"));
                assert_eq!(fragment, "", "the default route is the contested one");
            }
            other => panic!("expected DuplicateClaim, got {other:?}"),
        }
    }

    #[test]
    fn plan_tolerates_a_page_repeating_its_own_alias() {
        // Redundant, not contradictory: the route asked for twice is
        // the one the page would get anyway.
        let plan = plan_alias_stubs(&[page(
            "p/index.qmd",
            "p/index.html",
            &["/old.html", "/old.html"],
        )]);
        assert!(plan.conflicts.is_empty(), "{:?}", plan.conflicts);
        assert_eq!(stub_paths(&plan), ["old.html"]);
    }

    #[test]
    fn plan_rejects_a_case_only_collision_between_aliases() {
        let plan = plan_alias_stubs(&[
            page("one/index.qmd", "one/index.html", &["/Shared.html"]),
            page("two/index.qmd", "two/index.html", &["/shared.html"]),
        ]);
        assert!(plan.stubs.is_empty());
        assert!(
            plan.conflicts
                .iter()
                .any(|c| matches!(c, AliasConflict::CaseOnlyAliasCollision { .. })),
            "expected CaseOnlyAliasCollision, got {:?}",
            plan.conflicts
        );
    }

    #[test]
    fn plan_rejects_a_case_only_collision_with_a_page() {
        let plan = plan_alias_stubs(&[
            page("readme.qmd", "README.html", &[]),
            page("p/index.qmd", "p/index.html", &["/readme.html"]),
        ]);
        assert!(plan.stubs.is_empty());
        match plan.conflicts.as_slice() {
            [
                AliasConflict::CaseOnlyPageCollision {
                    stub_href,
                    page_href,
                    ..
                },
            ] => {
                assert_eq!(stub_href, "readme.html");
                assert_eq!(page_href, "README.html");
            }
            other => panic!("expected CaseOnlyPageCollision, got {other:?}"),
        }
    }

    #[test]
    fn plan_case_folding_is_platform_independent() {
        // The collision above must be found on Linux too, where the
        // filesystem would happily keep both files. Shipping such a
        // site breaks it the moment it is served from macOS.
        let plan = plan_alias_stubs(&[
            page("one/index.qmd", "one/index.html", &["/A/B.html"]),
            page("two/index.qmd", "two/index.html", &["/a/b.html"]),
        ]);
        assert!(
            !plan.conflicts.is_empty(),
            "case-only collision must be detected regardless of host filesystem"
        );
    }

    #[test]
    fn plan_rejects_an_alias_escaping_the_output_dir() {
        let plan = plan_alias_stubs(&[page("index.qmd", "index.html", &["../escaped.html"])]);
        assert!(plan.stubs.is_empty());
        match plan.conflicts.as_slice() {
            [AliasConflict::EscapesOutputDir { alias }] => {
                assert_eq!(alias.alias, "../escaped.html");
                assert_eq!(alias.source_path, Path::new("index.qmd"));
            }
            other => panic!("expected EscapesOutputDir, got {other:?}"),
        }
    }

    #[test]
    fn plan_skips_draft_pages_but_still_guards_their_output() {
        // A draft contributes no redirect...
        let plan = plan_alias_stubs(&[draft("wip.qmd", "wip.html", &["/old.html"])]);
        assert!(plan.stubs.is_empty(), "a draft's alias must not go live");
        assert!(plan.conflicts.is_empty());

        // ...but it still renders, so its output path is occupied.
        let plan = plan_alias_stubs(&[
            draft("wip.qmd", "wip.html", &[]),
            page("p/index.qmd", "p/index.html", &["/wip.html"]),
        ]);
        assert!(
            matches!(
                plan.conflicts.as_slice(),
                [AliasConflict::OverwritesPage { .. }]
            ),
            "got {:?}",
            plan.conflicts
        );
    }

    #[test]
    fn plan_reports_every_conflict_in_one_pass() {
        // A project with several mistakes should learn about all of
        // them from a single render.
        let plan = plan_alias_stubs(&[
            page("index.qmd", "index.html", &[]),
            page(
                "one/index.qmd",
                "one/index.html",
                &["/shared.html", "/index.html", "../../escaped.html"],
            ),
            page("two/index.qmd", "two/index.html", &["/shared.html"]),
        ]);
        assert!(plan.stubs.is_empty());
        assert!(
            plan.conflicts
                .iter()
                .any(|c| matches!(c, AliasConflict::EscapesOutputDir { .. }))
                && plan
                    .conflicts
                    .iter()
                    .any(|c| matches!(c, AliasConflict::OverwritesPage { .. }))
                && plan
                    .conflicts
                    .iter()
                    .any(|c| matches!(c, AliasConflict::DuplicateClaim { .. })),
            "expected all three kinds, got {:?}",
            plan.conflicts
        );
    }

    // ── rendering ────────────────────────────────────────────────

    fn render(redirects: &[(&str, &str)]) -> String {
        render_stub(&PlannedStub {
            stub_href: "old.html".to_string(),
            redirects: redirects
                .iter()
                .map(|(f, t)| (f.to_string(), t.to_string()))
                .collect(),
        })
    }

    #[test]
    fn stub_works_without_javascript() {
        let html = render(&[("", "page/index.html")]);
        assert!(
            html.contains(
                r#"<noscript><meta http-equiv="refresh" content="0; url=page/index.html"></noscript>"#
            ),
            "got:\n{html}"
        );
        assert!(
            html.contains(r#"<a href="page/index.html">page/index.html</a>"#),
            "got:\n{html}"
        );
    }

    #[test]
    fn stub_names_the_canonical_page() {
        let html = render(&[("", "page/index.html")]);
        assert!(
            html.contains(r#"<link rel="canonical" href="page/index.html">"#),
            "got:\n{html}"
        );
        assert!(html.starts_with("<!DOCTYPE html>"), "got:\n{html}");
        assert!(html.contains(r#"<meta charset="utf-8">"#), "got:\n{html}");
    }

    #[test]
    fn stub_renders_the_fragment_map_in_order() {
        let html = render(&[
            ("", "../build/index.html"),
            ("deploy", "../deploy/index.html"),
            ("image", "../build/index.html"),
        ]);
        assert!(
            html.contains(
                r#"var redirects = {"":"../build/index.html","deploy":"../deploy/index.html","image":"../build/index.html"};"#
            ),
            "got:\n{html}"
        );
    }

    #[test]
    fn stub_escapes_ampersands_in_attributes_but_not_in_json() {
        let html = render(&[("", "a&b/index.html")]);
        assert!(
            html.contains(r#"<link rel="canonical" href="a&amp;b/index.html">"#),
            "unescaped `&` truncates the href at the entity boundary; got:\n{html}"
        );
        assert!(
            html.contains(r#"{"":"a&b/index.html"}"#),
            "the JSON literal keeps the raw character; got:\n{html}"
        );
    }

    #[test]
    fn stub_cannot_be_escaped_by_a_hostile_path() {
        // HTML does not decode entities inside <script>, so a literal
        // `</script>` in a path would end the element and spill the
        // rest of the redirect map into the document as markup.
        let html = render(&[("", "</script><img src=x>.html")]);
        assert!(
            !html.contains("</script><img"),
            "path must not be able to close the script element; got:\n{html}"
        );
        assert!(
            html.contains(r#"{"":"\u003c/script>\u003cimg src=x>.html"}"#),
            "expected `<` numerically escaped in the JSON; got:\n{html}"
        );
        // The attribute contexts are covered by entity escaping, which
        // *is* decoded there — so the same path is neutralised twice,
        // by the mechanism appropriate to each context.
        assert!(
            html.contains(r#"href="&lt;/script&gt;&lt;img src=x&gt;.html""#),
            "expected entity escaping in attribute context; got:\n{html}"
        );
    }

    #[test]
    fn stub_is_byte_stable_for_the_same_plan() {
        // Sites are diffed and cached; a stub that shuffles between
        // runs would churn every deploy.
        assert_eq!(
            render(&[("", "a.html"), ("x", "b.html")]),
            render(&[("", "a.html"), ("x", "b.html")])
        );
    }

    #[test]
    fn plan_output_is_ordered_deterministically() {
        // Stub order comes from a BTreeMap, not profile order, so two
        // projects that differ only in file-discovery order produce
        // byte-identical sites.
        // Stub names deliberately unrelated to the page names, so no
        // alias lands on a rendered page.
        let forward = plan_alias_stubs(&[
            page("a.qmd", "a.html", &["/old-z.html"]),
            page("b.qmd", "b.html", &["/old-m.html"]),
            page("c.qmd", "c.html", &["/old-a.html"]),
        ]);
        let reversed = plan_alias_stubs(&[
            page("c.qmd", "c.html", &["/old-a.html"]),
            page("b.qmd", "b.html", &["/old-m.html"]),
            page("a.qmd", "a.html", &["/old-z.html"]),
        ]);
        assert!(forward.conflicts.is_empty(), "{:?}", forward.conflicts);
        assert_eq!(
            stub_paths(&forward),
            ["old-a.html", "old-m.html", "old-z.html"]
        );
        assert_eq!(forward, reversed);
    }
}
