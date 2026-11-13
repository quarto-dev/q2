/*
 * presenter/visibility.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Which parts of a cell reach the output (echo, output, warning, include).
 */

//! Cell-option resolution and cell visibility (bd-nn2fou8h).

use quarto_pandoc_types::config_value::ConfigValue;

use crate::cell_options::{merge_cell_over_scope, options_to_config};

/// One cell's options resolved against the document: the cell's `#|`
/// map merged *over* the document's `execute` scope, so a cell option
/// wins wherever both set the same key (Q1's `shouldInclude`, and the
/// scoped resolution bd-ohvl879u introduced for `error:`).
///
/// `None` when neither scope supplied anything — callers then fall
/// back to per-option defaults.
pub fn resolve_cell_options(
    doc_scope: Option<&ConfigValue>,
    options: Option<quarto_yaml::YamlWithSourceInfo>,
) -> Option<ConfigValue> {
    let cell_config = cell_config(options);
    merge_cell_over_scope(doc_scope, cell_config.as_ref())
}

/// The cell\'s own `#|` options as a config, without the document scope under them. Options
/// that nothing handles are forwarded from this, not from the merged scope (Phase 7g).
pub fn cell_config(options: Option<quarto_yaml::YamlWithSourceInfo>) -> Option<ConfigValue> {
    options.map(|o| {
        let (config, diagnostics) = options_to_config(o);
        for d in diagnostics {
            tracing::warn!("cell option conversion diagnostic: {d:?}");
        }
        config
    })
}

/// Read a boolean option out of resolved cell options, falling back to
/// `default` when the key is absent or not a boolean.
pub fn resolved_flag(resolved: Option<&ConfigValue>, key: &str, default: bool) -> bool {
    resolved
        .and_then(|merged| merged.get(key))
        .and_then(|v| v.as_bool())
        .unwrap_or(default)
}

/// Which parts of a cell reach the output (bd-nn2fou8h).
///
/// Mirrors Q1's `shouldInclude` family (`src/core/jupyter/tags.ts`):
/// each key is resolved cell-over-document and defaults to `true`, so
/// a document that says nothing keeps today's fully-visible behaviour.
///
/// `include: false` is the master switch — Q1 bails on the whole cell
/// before consulting anything else (`mdFromCodeCell`'s
/// `if (!includeCell(...)) return []`), which is why [`Self::resolve`]
/// collapses the other flags rather than leaving callers to remember
/// the precedence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellVisibility {
    /// Echo the cell's source.
    pub echo: bool,
    /// Emit the cell's outputs.
    pub output: bool,
    /// Keep stderr-stream outputs (Python warnings land there).
    pub warning: bool,
}

impl CellVisibility {
    /// Everything visible — the default for a cell with no options and
    /// a document with no `execute:` scope.
    pub const VISIBLE: Self = Self {
        echo: true,
        output: true,
        warning: true,
    };

    /// Nothing at all reaches the output (`include: false`).
    pub const HIDDEN: Self = Self {
        echo: false,
        output: false,
        warning: false,
    };

    /// The one rule (decision D2, OPR-2), over any source of boolean options. `echo`,
    /// `output` and `include` are always presenter-enforced: hiding what capture already hid
    /// is harmless, and knitr records outputs the presenter must hide. `warning` is read only
    /// when `presenter_enforces_warning`; otherwise capture applied it and the stderr that is
    /// left is the author\'s to see. (`message` has no switch: nothing declares it display-only,
    /// and the presenter cannot tell a message from a warning on a bare stderr stream.)
    pub fn from_flags(
        flag: impl Fn(&str) -> Option<bool>,
        presenter_enforces_warning: bool,
    ) -> Self {
        if flag("include") == Some(false) {
            return Self::HIDDEN;
        }
        let output = flag("output") != Some(false);
        Self {
            echo: flag("echo") != Some(false),
            output,
            // Q1 filters stderr under `output: false` too: the option suppresses every
            // output, warnings included.
            warning: output && !(presenter_enforces_warning && flag("warning") == Some(false)),
        }
    }

    /// [`Self::from_flags`] over resolved cell options (`None`: neither the cell nor the
    /// document said anything, everything visible).
    pub fn resolve(resolved: Option<&ConfigValue>, presenter_enforces_warning: bool) -> Self {
        Self::from_flags(
            |key| resolved.and_then(|r| r.get(key)).and_then(|v| v.as_bool()),
            presenter_enforces_warning,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Jupyter case: the presenter enforces `warning`.
    fn resolve(resolved: Option<&ConfigValue>) -> CellVisibility {
        CellVisibility::resolve(resolved, true)
    }

    // === bd-nn2fou8h: execute-visibility resolution ===
    //
    // Resolution and emission are unit-testable without a kernel; the
    // end-to-end matrix (both engines, doc and cell scope) lives in
    // `tests/integration/engine_visibility.rs`.

    /// Build resolved cell options from YAML source, as
    /// `resolve_cell_options` would after merging.
    fn resolved(yaml: &str) -> Option<ConfigValue> {
        let parsed = quarto_yaml::parse(yaml).expect("test YAML parses");
        let (config, _) = options_to_config(parsed);
        Some(config)
    }

    #[test]
    fn test_visibility_defaults_to_fully_visible() {
        assert_eq!(resolve(None), CellVisibility::VISIBLE);
        assert_eq!(
            resolve(resolved("error: true").as_ref()),
            CellVisibility::VISIBLE,
            "an unrelated option must not change visibility"
        );
    }

    #[test]
    fn test_visibility_reads_each_key() {
        assert!(!resolve(resolved("echo: false").as_ref()).echo);
        assert!(!resolve(resolved("output: false").as_ref()).output);
        assert!(!resolve(resolved("warning: false").as_ref()).warning);
        // Independence: hiding one must not hide the others.
        let v = resolve(resolved("echo: false").as_ref());
        assert!(v.output && v.warning);
    }

    #[test]
    fn test_include_false_hides_everything() {
        assert_eq!(
            resolve(resolved("include: false").as_ref()),
            CellVisibility::HIDDEN
        );
        // include: false wins even when another key says "show me".
        assert_eq!(
            resolve(resolved("include: false\necho: true").as_ref()),
            CellVisibility::HIDDEN
        );
    }

    /// `output: false` suppresses every output, warnings included — so
    /// a cell must not leak stderr through the warning channel after
    /// its outputs were switched off.
    #[test]
    fn test_output_false_implies_warnings_hidden() {
        let v = resolve(resolved("output: false\nwarning: true").as_ref());
        assert!(!v.warning, "output: false must also silence warnings");
    }

    /// EXR-4 Phase 0 audit: Jupyter's presenter reads only `include`,
    /// `echo`, `output` and `warning` for visibility (`error` is read
    /// separately by the executor, `engine/jupyter/text_execute.rs`, and
    /// the stored-replay path, `stored.rs`). Every other option, including
    /// `eval`, `execute-dir` and `params`, leaves visibility untouched, so
    /// the Jupyter hash declaration's closed presentation list is the
    /// visibility set plus display-only options, and `execute-dir`,
    /// `params` and `fig-width` over-invalidate Jupyter harmlessly.
    #[test]
    fn exr4_audit_jupyter_visibility_ignores_every_other_key() {
        let ignored = [
            "eval: false",
            "error: true",
            "execute-dir: project",
            "params: {a: 1}",
            "fig-width: 3",
            "fig-height: 3",
            "fig-asp: 0.5",
            "fig-dpi: 50",
            "fig-format: jpeg",
            "df-print: paged",
            "enabled: false",
            "debug: true",
            "cache: true",
            "freeze: true",
            "label: fig-x",
            "fig-cap: A caption",
        ];
        for yaml in ignored {
            assert_eq!(
                resolve(resolved(yaml).as_ref()),
                CellVisibility::VISIBLE,
                "`{yaml}` must not change Jupyter visibility"
            );
        }
        for yaml in [
            "include: false",
            "echo: false",
            "output: false",
            "warning: false",
        ] {
            assert_ne!(
                resolve(resolved(yaml).as_ref()),
                CellVisibility::VISIBLE,
                "`{yaml}` is one of the four keys Jupyter reads"
            );
        }
    }
}
