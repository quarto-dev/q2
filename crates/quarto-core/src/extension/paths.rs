/*
 * extension/paths.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Shared machinery for classifying extension-bundled file references
 * in contributed config (bd-ad7i1pc6 Phase 4, bd-of20unsb).
 */

//! Pattern-guided discovery of extension-bundled file paths.
//!
//! Extensions contribute config fragments (`contributes.project`,
//! `contributes.metadata`, `contributes.formats`) whose string values
//! may name files bundled with the extension — theme SCSS, CSS,
//! include snippets, templates. Those strings are written relative to
//! the extension directory, but the merged config is consumed relative
//! to the project root or document directory, so bundled references
//! must be recognized and rebased.
//!
//! Two ingredients are shared by every consumer and live here so the
//! key tables cannot drift between them:
//!
//! - [`walk_pattern_leaves`]: a key-path pattern walk over a
//!   [`ConfigValue`] tree. `*` matches any map key; arrays are
//!   transparent (a pattern position applies to every item). When a
//!   pattern is exhausted at a node, the action is applied to every
//!   string leaf underneath — this is what makes both the
//!   `theme: [cosmo, custom.scss]` and `theme: {light: […], dark: …}`
//!   forms work from a single `theme` pattern entry.
//! - [`bundled_file_exists`]: the *existence check* that decides
//!   whether an individual string names a bundled file. Rooted paths
//!   and URLs are never bundled; otherwise the string is a bundled
//!   file exactly when it exists under the extension dir. Builtin
//!   theme names (`cosmo`), command lines, and document-relative
//!   references simply don't exist there and pass through untouched.
//!
//! Two marking modes exist deliberately (bd-of20unsb design):
//!
//! - **Existence-driven** ([`mark_bundled_format_assets`], and the
//!   fragment rebase in `crate::project`): for keys where a string may
//!   be *either* a bundled file or something else (a builtin theme
//!   name, a project-relative path). The filesystem disambiguates.
//! - **Unconditional** (`PATH_VALUED_KEYS` in `crate::extension::read`):
//!   for keys (`template`, `template-partials`, `shortcodes`, filter
//!   entries) whose values are always paths semantically. An existence
//!   check there would convert a clear missing-file error into a
//!   silent document-dir fallback — worse, not better.
//!
//! A future manifest schema may signal path entries in-band (e.g.
//! `file:` object keys), which would obsolete the sniffing; until
//! then, keep both tables here-adjacent and documented.

use std::path::Path;

use quarto_pandoc_types::{ConfigValue, ConfigValueKind};
use quarto_system_runtime::SystemRuntime;

/// Keys in a *format* config map (`contributes.formats.<fmt>`) whose
/// string values may name files bundled with the extension. The
/// existence check decides per string; builtin theme names pass
/// through. Mirrors the `format.*` subset of the
/// `contributes.project` fragment table in `crate::project`
/// (`FRAGMENT_PATH_PATTERNS`), minus `template`/`template-partials`,
/// which are unconditionally path-valued and handled by
/// `PATH_VALUED_KEYS` in `crate::extension::read`.
pub(crate) const FORMAT_ASSET_PATTERNS: &[&[&str]] = &[
    &["theme"],
    &["css"],
    &["include-in-header"],
    &["include-before-body"],
    &["include-after-body"],
    &["format-resources"],
];

/// Walk `value` guided by key-path `patterns`, applying `action` to
/// every string-like leaf (`Scalar` string or `Path`) reachable once a
/// pattern is exhausted.
///
/// `*` matches any map key; arrays are transparent at any position.
/// Nodes matching no pattern prefix are left untouched.
pub(crate) fn walk_pattern_leaves(
    value: &mut ConfigValue,
    patterns: &[&[&str]],
    action: &mut dyn FnMut(&mut ConfigValue),
) {
    if patterns.iter().any(|p| p.is_empty()) {
        apply_to_string_leaves(value, action);
        return;
    }
    match &mut value.value {
        ConfigValueKind::Map(entries) => {
            for entry in entries {
                let next: Vec<&[&str]> = patterns
                    .iter()
                    .filter(|p| p[0] == "*" || p[0] == entry.key)
                    .map(|p| &p[1..])
                    .collect();
                if !next.is_empty() {
                    walk_pattern_leaves(&mut entry.value, &next, action);
                }
            }
        }
        // Arrays are transparent: items share the pattern position.
        ConfigValueKind::Array(items) => {
            for item in items {
                walk_pattern_leaves(item, patterns, action);
            }
        }
        _ => {}
    }
}

/// Apply `action` to every `Scalar`-string or `Path` leaf under
/// `value`, recursing through maps and arrays.
fn apply_to_string_leaves(value: &mut ConfigValue, action: &mut dyn FnMut(&mut ConfigValue)) {
    match &mut value.value {
        ConfigValueKind::Map(entries) => {
            for entry in entries {
                apply_to_string_leaves(&mut entry.value, action);
            }
        }
        ConfigValueKind::Array(items) => {
            for item in items {
                apply_to_string_leaves(item, action);
            }
        }
        ConfigValueKind::Scalar {
            yaml: yaml_rust2::Yaml::String(_),
            ..
        }
        | ConfigValueKind::Path(_) => {
            action(value);
        }
        _ => {}
    }
}

/// Does `s` name a file bundled under `ext_dir`?
///
/// Rooted paths and URLs are never bundled references; otherwise the
/// string is a bundled file exactly when it exists under the extension
/// dir. (`path_exists` with kind `None` — directories count, matching
/// the `contributes.project` fragment rebase, where e.g. resource
/// entries may be directories.)
pub(crate) fn bundled_file_exists(s: &str, ext_dir: &Path, runtime: &dyn SystemRuntime) -> bool {
    if quarto_util::is_rooted(Path::new(s)) || s.starts_with("http://") || s.starts_with("https://")
    {
        return false;
    }
    runtime.path_exists(&ext_dir.join(s), None).unwrap_or(false)
}

/// Mark bundled-file references in one format's contributed config
/// (`contributes.formats.<fmt>`) as [`ConfigValueKind::Path`], leaving
/// the string ext-dir-relative (bd-of20unsb).
///
/// Unlike the `contributes.project` fragment rebase (which rewrites to
/// project-root-relative because project config merges once), format
/// layers merge **per document**, and `MetadataMergeStage` already
/// rebases `Path`-kind values from the extension dir to the document
/// dir — so marking is the entire job here.
pub(crate) fn mark_bundled_format_assets(
    format_config: &mut ConfigValue,
    ext_dir: &Path,
    runtime: &dyn SystemRuntime,
) {
    walk_pattern_leaves(format_config, FORMAT_ASSET_PATTERNS, &mut |leaf| {
        let ConfigValueKind::Scalar {
            yaml: yaml_rust2::Yaml::String(s),
            ..
        } = &leaf.value
        else {
            return;
        };
        if bundled_file_exists(s, ext_dir, runtime) {
            leaf.value = ConfigValueKind::Path(s.clone());
        }
    });
}
