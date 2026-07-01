/*
 * extension/types.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Extension data model types.
 */

//! Data types for Quarto extensions.

use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;

use quarto_error_reporting::DiagnosticMessage;
use quarto_pandoc_types::ConfigValue;

/// Identifies an extension by name and optional organization.
///
/// Examples:
/// - `ExtensionId { name: "lightbox", organization: None }`
/// - `ExtensionId { name: "acm", organization: Some("quarto-journals") }`
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ExtensionId {
    pub name: String,
    pub organization: Option<String>,
}

impl ExtensionId {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            organization: None,
        }
    }

    pub fn with_organization(name: impl Into<String>, org: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            organization: Some(org.into()),
        }
    }
}

impl fmt::Display for ExtensionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(ref org) = self.organization {
            write!(f, "{}/{}", org, self.name)
        } else {
            write!(f, "{}", self.name)
        }
    }
}

/// A parsed and resolved Quarto extension.
#[derive(Debug, Clone)]
pub struct Extension {
    pub id: ExtensionId,
    pub title: String,
    pub author: String,
    pub version: Option<String>,
    pub quarto_required: Option<String>,
    /// Absolute path to the extension directory.
    pub path: PathBuf,
    pub contributes: Contributes,
}

/// What an extension contributes.
///
/// All path fields contain absolute paths (resolved during `read_extension`).
/// Format metadata is stored as `ConfigValue` for direct use in the merge pipeline.
#[derive(Debug, Clone, Default)]
pub struct Contributes {
    /// Format-specific metadata, keyed by format name (e.g., "html", "pdf").
    /// The "common" key has already been merged into siblings and removed.
    pub formats: HashMap<String, ConfigValue>,

    /// Top-level filter contributions (absolute paths).
    pub filters: Vec<ExtensionFilter>,

    /// Top-level shortcode contributions (absolute paths).
    pub shortcodes: Vec<PathBuf>,

    /// Raw metadata contribution (merged into project config).
    pub metadata: Option<ConfigValue>,

    /// Raw project contribution.
    pub project: Option<ConfigValue>,

    /// Engine contributions: paths to TS engine modules or engine name
    /// strings for reordering.
    pub engines: Vec<EngineContribution>,
}

/// An engine contributed by an extension.
#[derive(Debug, Clone)]
pub enum EngineContribution {
    /// An external engine module (pre-built .js bundle). `path` is absolute,
    /// resolved during read_extension.
    External {
        path: PathBuf,
        /// Static declaration of the engine's runtime name (e.g. "julia").
        /// `None` = not declared (q2 must LoadEngine to learn the name).
        /// `Some(name)` = registered under `name` with no subprocess load.
        name: Option<String>,
        /// Authoritative static language claims, keyed by language
        /// (engine-resolution.md §3.3 `claims:` map). `None` = undeclared
        /// (fall back to dynamic load). `Some(map)` = resolve without loading.
        ///
        /// Each language key holds a **Vec** of claims (4c0): a YAML sequence
        /// value parses to one claim per element; a scalar/bool/int/single-object
        /// value (pre-4c0 shape) still parses to a 1-element Vec. This lets one
        /// language key carry both a `whenClass`-conditioned primary claim and an
        /// unconditional interop claim (e.g. marimo's bare-`{sql}` case). See
        /// `lookup_static_claim`/`combine_claims` for the reduction rule.
        claims: Option<HashMap<String, Vec<StaticLanguageClaim>>>,
        /// `valid_extensions` — file extensions this engine handles (e.g. [".jl"]).
        /// `None` undeclared; `Some(vec![])` handles none; `Some(...)` the set.
        file_extensions: Option<Vec<String>>,
        /// Unconditional static `claims_file` — extensions claimed WITHOUT content
        /// inspection (§3.3 `claims-files:`). `None` = fall back to dynamic claims_file.
        claims_files: Option<Vec<String>>,
    },
    /// A bare engine name string — reordering hint that moves a previously
    /// registered engine to higher priority.
    Reorder { name: String },
}

/// One authoritative static language claim (§3.3). A pure tabulation of
/// `claims_language(language, first_class)`.
#[derive(Debug, Clone)]
pub struct StaticLanguageClaim {
    pub kind: ClaimKind,       // primary | interop | fallback
    pub priority: Option<i32>, // defaults per kind (Primary 1, Interop/Fallback 0)
    /// `None`: applies for any/no first_class. `Some(c)`: applies ONLY when the
    /// cell's first class == `c` (the marimo case — `{python .marimo}`).
    pub when_class: Option<String>,
}

/// The kind half of `LanguageClaim`, without the priority payload, so a static
/// claim can carry kind + priority separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimKind {
    Primary,
    Interop,
    Fallback,
}

impl ClaimKind {
    /// Explicit combine-rule rank for the 4c0 Vec-per-language reducer:
    /// Primary > Interop > Fallback. This is **deliberately new, bespoke
    /// code** — it is NOT a reuse of the cross-engine resolution-tier
    /// ordering documented at `resolution.rs:321` ("kind dominates priority").
    /// That note describes *emergent* behavior of running the T1→T4
    /// cross-engine tiers in sequence; it is a doc comment, not a reusable
    /// function. This method is the per-Vec (single-language-key) reducer's
    /// own, independent comparator.
    fn combine_rank(self) -> u8 {
        match self {
            ClaimKind::Primary => 2,
            ClaimKind::Interop => 1,
            ClaimKind::Fallback => 0,
        }
    }
}

/// Convert a single static claim to a `LanguageClaim`, honoring `when_class`.
/// Returns `LanguageClaim::None` when `when_class` is `Some(c)` and `c != first_class`.
pub fn static_claim_to_language_claim(
    claim: &StaticLanguageClaim,
    first_class: Option<&str>,
) -> crate::engine::LanguageClaim {
    use crate::engine::LanguageClaim;
    // Guard: if when_class is declared, it must match first_class exactly.
    if let Some(ref required) = claim.when_class
        && first_class != Some(required.as_str())
    {
        return LanguageClaim::None;
    }
    match claim.kind {
        ClaimKind::Primary => LanguageClaim::Primary(claim.priority.unwrap_or(1)),
        ClaimKind::Interop => LanguageClaim::Interop(claim.priority.unwrap_or(0)),
        ClaimKind::Fallback => LanguageClaim::Fallback(claim.priority.unwrap_or(0)),
    }
}

/// Look up `language` in a static `claims` map and combine its Vec of claims,
/// honoring `first_class`. Returns `LanguageClaim::None` if the language has
/// no entry, or none of its entries' `when_class` match.
///
/// See `combine_claims` for the reduction rule applied to a language's Vec.
pub fn lookup_static_claim<S: ::std::hash::BuildHasher>(
    claims: &HashMap<String, Vec<StaticLanguageClaim>, S>,
    language: &str,
    first_class: Option<&str>,
) -> crate::engine::LanguageClaim {
    match claims.get(language) {
        None => crate::engine::LanguageClaim::None,
        Some(claim_vec) => combine_claims(claim_vec, first_class),
    }
}

/// Extract the `(kind_rank, priority)` pair used to rank a non-`None`
/// `LanguageClaim` for the combine-rule reducer. Panics if given `None` —
/// callers must filter those out first (a `None` claim carries no kind to
/// rank).
fn claim_rank(claim: &crate::engine::LanguageClaim) -> (u8, i32) {
    use crate::engine::LanguageClaim;
    match claim {
        LanguageClaim::Primary(p) => (ClaimKind::Primary.combine_rank(), *p),
        LanguageClaim::Interop(p) => (ClaimKind::Interop.combine_rank(), *p),
        LanguageClaim::Fallback(p) => (ClaimKind::Fallback.combine_rank(), *p),
        LanguageClaim::None => {
            unreachable!("claim_rank must only be called on non-None claims")
        }
    }
}

/// Combine a Vec of static claims for one language key into the single
/// strongest applicable `LanguageClaim` (the 4c0 combine rule).
///
/// Maps each claim through `static_claim_to_language_claim` (which returns
/// `LanguageClaim::None` on a `whenClass` mismatch), drops the `None`s, and
/// reduces the survivors by `ClaimKind::combine_rank` (kind dominates
/// priority: Primary > Interop > Fallback, `priority` breaking ties within a
/// kind). Returns `LanguageClaim::None` if every claim mismatches or the Vec
/// is empty.
///
/// Order-independent by construction: the reducer never looks at Vec
/// position, only at each element's converted kind/priority.
pub fn combine_claims(
    claims: &[StaticLanguageClaim],
    first_class: Option<&str>,
) -> crate::engine::LanguageClaim {
    use crate::engine::LanguageClaim;
    claims
        .iter()
        .map(|c| static_claim_to_language_claim(c, first_class))
        .filter(|c| *c != LanguageClaim::None)
        .max_by_key(claim_rank)
        .unwrap_or(LanguageClaim::None)
}

/// Returns a warning if an External engine omits any static-declaration field
/// (`name`/`claims`/`file-extensions`/`claims-files`), naming exactly which are
/// missing. Returns `None` for `Reorder`, or for an `External` that declares all
/// four (a field present-but-empty — `Some({})`/`Some([])` — counts as DECLARED,
/// not missing). The caller (Task 7, registry construction) pushes the message
/// into `registry.diagnostics`; this fn does not emit anywhere itself.
pub fn engine_contribution_missing_fields_warning(
    extension_label: &str,
    contribution: &EngineContribution,
) -> Option<DiagnosticMessage> {
    let EngineContribution::External {
        path,
        name,
        claims,
        file_extensions,
        claims_files,
    } = contribution
    else {
        return None;
    };

    let mut missing: Vec<&str> = Vec::new();
    if name.is_none() {
        missing.push("name");
    }
    if claims.is_none() {
        missing.push("claims");
    }
    if file_extensions.is_none() {
        missing.push("file-extensions");
    }
    if claims_files.is_none() {
        missing.push("claims-files");
    }

    if missing.is_empty() {
        return None;
    }

    let missing_list = missing.join(", ");
    let msg = format!(
        "Engine extension '{}' (path: {}) is missing static declarations: {}. \
        An engine missing name/claims/claims-files is treated as a legacy Q1 engine \
        and LoadEngined on every render (slow dynamic path) vs. zero-load when declared. \
        Declare [] or {{}} to silence a field the engine genuinely has none of.",
        extension_label,
        path.display(),
        missing_list,
    );

    Some(DiagnosticMessage::warning(msg))
}

/// A filter contributed by an extension.
#[derive(Debug, Clone)]
pub struct ExtensionFilter {
    pub path: PathBuf,
    pub at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extension_id_display_no_org() {
        let id = ExtensionId::new("lightbox");
        assert_eq!(id.to_string(), "lightbox");
    }

    #[test]
    fn test_extension_id_display_with_org() {
        let id = ExtensionId::with_organization("acm", "quarto-journals");
        assert_eq!(id.to_string(), "quarto-journals/acm");
    }

    #[test]
    fn test_extension_id_equality() {
        let a = ExtensionId::new("lightbox");
        let b = ExtensionId::new("lightbox");
        let c = ExtensionId::new("other");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn test_extension_id_with_org_equality() {
        let a = ExtensionId::with_organization("acm", "quarto-journals");
        let b = ExtensionId::with_organization("acm", "quarto-journals");
        let c = ExtensionId::with_organization("acm", "other-org");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn test_extension_id_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(ExtensionId::new("a"));
        set.insert(ExtensionId::new("b"));
        set.insert(ExtensionId::new("a")); // duplicate
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn test_contributes_default() {
        let c = Contributes::default();
        assert!(c.formats.is_empty());
        assert!(c.filters.is_empty());
        assert!(c.shortcodes.is_empty());
        assert!(c.metadata.is_none());
        assert!(c.project.is_none());
        assert!(c.engines.is_empty());
    }

    // --- static_claim_to_language_claim tests (seam P1-14) ---

    fn make_claim(
        kind: ClaimKind,
        priority: Option<i32>,
        when_class: Option<&str>,
    ) -> StaticLanguageClaim {
        StaticLanguageClaim {
            kind,
            priority,
            when_class: when_class.map(|s| s.to_string()),
        }
    }

    #[test]
    fn static_claim_primary_no_when_class_default_priority() {
        // when_class: None, kind: Primary, priority: None → Primary(1)
        let claim = make_claim(ClaimKind::Primary, None, None);
        assert_eq!(
            static_claim_to_language_claim(&claim, None),
            crate::engine::LanguageClaim::Primary(1)
        );
    }

    #[test]
    fn static_claim_primary_no_when_class_explicit_priority() {
        // when_class: None, kind: Primary, priority: Some(5) → Primary(5)
        let claim = make_claim(ClaimKind::Primary, Some(5), None);
        assert_eq!(
            static_claim_to_language_claim(&claim, None),
            crate::engine::LanguageClaim::Primary(5)
        );
    }

    #[test]
    fn static_claim_interop_and_fallback_default_priority() {
        // kind: Interop, priority: None → Interop(0)
        let interop = make_claim(ClaimKind::Interop, None, None);
        assert_eq!(
            static_claim_to_language_claim(&interop, None),
            crate::engine::LanguageClaim::Interop(0)
        );
        // kind: Fallback, priority: None → Fallback(0)
        let fallback = make_claim(ClaimKind::Fallback, None, None);
        assert_eq!(
            static_claim_to_language_claim(&fallback, None),
            crate::engine::LanguageClaim::Fallback(0)
        );
    }

    #[test]
    fn static_claim_when_class_match_converts() {
        // when_class: Some("marimo"), first_class: Some("marimo") → Primary(1)
        let claim = make_claim(ClaimKind::Primary, None, Some("marimo"));
        assert_eq!(
            static_claim_to_language_claim(&claim, Some("marimo")),
            crate::engine::LanguageClaim::Primary(1)
        );
    }

    #[test]
    fn static_claim_when_class_mismatch_returns_none() {
        // when_class: Some("marimo"), first_class: Some("python") → None
        let claim = make_claim(ClaimKind::Primary, None, Some("marimo"));
        assert_eq!(
            static_claim_to_language_claim(&claim, Some("python")),
            crate::engine::LanguageClaim::None
        );
    }

    #[test]
    fn static_claim_when_class_mismatch_no_first_class_returns_none() {
        // when_class: Some("marimo"), first_class: None → None
        let claim = make_claim(ClaimKind::Primary, None, Some("marimo"));
        assert_eq!(
            static_claim_to_language_claim(&claim, None),
            crate::engine::LanguageClaim::None
        );
    }

    // --- lookup_static_claim tests (migrated to Vec-per-language, 4c0) ---
    //
    // Migration note: each map value became `Vec<StaticLanguageClaim>`. These
    // three tests keep their original 1-element-Vec discriminators verbatim
    // (absent key / matching whenClass / mismatched whenClass) so the
    // pre-4c0 single-claim behavior is proven to survive the widening.

    #[test]
    fn lookup_absent_language_returns_none() {
        let claims: HashMap<String, Vec<StaticLanguageClaim>> = HashMap::new();
        assert_eq!(
            lookup_static_claim(&claims, "python", None),
            crate::engine::LanguageClaim::None
        );
    }

    #[test]
    fn lookup_present_matching_when_class_converts() {
        let mut claims = HashMap::new();
        claims.insert(
            "python".to_string(),
            vec![make_claim(ClaimKind::Primary, None, Some("marimo"))],
        );
        assert_eq!(
            lookup_static_claim(&claims, "python", Some("marimo")),
            crate::engine::LanguageClaim::Primary(1)
        );
    }

    #[test]
    fn lookup_present_mismatched_when_class_returns_none() {
        let mut claims = HashMap::new();
        claims.insert(
            "python".to_string(),
            vec![make_claim(ClaimKind::Primary, None, Some("marimo"))],
        );
        assert_eq!(
            lookup_static_claim(&claims, "python", Some("python")),
            crate::engine::LanguageClaim::None
        );
    }

    // --- SC1: Vec combine rule — order-independent, kind dominates priority ---
    //
    // The discriminator: `sql`'s Vec lists Interop FIRST, then the
    // whenClass-conditioned Primary. A naive "return the first non-None
    // claim" reducer would answer `lookup(sql, Some("marimo"))` with
    // `Interop(0)` (the first element always converts, since Interop has no
    // whenClass guard); the correct strongest-kind reducer must instead
    // combine to `Primary(2)`. This is why Interop is listed first here, not
    // as an arbitrary ordering choice.
    #[test]
    fn sc1_lookup_static_claim_combines_by_strongest_kind_not_vec_order() {
        let mut claims: HashMap<String, Vec<StaticLanguageClaim>> = HashMap::new();
        claims.insert(
            "sql".to_string(),
            vec![
                make_claim(ClaimKind::Interop, None, None),
                make_claim(ClaimKind::Primary, Some(2), Some("marimo")),
            ],
        );
        // All-claims-mismatch case: two Primary claims, both whenClass-gated,
        // neither matching a `None` first_class.
        claims.insert(
            "python".to_string(),
            vec![
                make_claim(ClaimKind::Primary, None, Some("marimo")),
                make_claim(ClaimKind::Primary, None, Some("other")),
            ],
        );

        assert_eq!(
            lookup_static_claim(&claims, "sql", Some("marimo")),
            crate::engine::LanguageClaim::Primary(2),
            "tagged sql: the whenClass-matching Primary claim must win over \
             the always-applicable Interop claim, even though Interop is \
             listed first in the Vec"
        );
        assert_eq!(
            lookup_static_claim(&claims, "sql", None),
            crate::engine::LanguageClaim::Interop(0),
            "bare sql: only the Interop claim applies (the Primary claim's \
             whenClass mismatches a None first_class)"
        );
        assert_eq!(
            lookup_static_claim(&claims, "python", None),
            crate::engine::LanguageClaim::None,
            "every claim in the Vec mismatches whenClass → None"
        );
    }

    // --- P1-11: engine_contribution_missing_fields_warning matrix ---

    fn make_full_external() -> EngineContribution {
        EngineContribution::External {
            path: std::path::PathBuf::from("/path/to/engine.js"),
            name: Some("myengine".to_string()),
            claims: Some(HashMap::new()),
            file_extensions: Some(vec![]),
            claims_files: Some(vec![]),
        }
    }

    fn make_external_without(missing_field: &str) -> EngineContribution {
        EngineContribution::External {
            path: std::path::PathBuf::from("/path/to/engine.js"),
            name: if missing_field == "name" {
                None
            } else {
                Some("myengine".to_string())
            },
            claims: if missing_field == "claims" {
                None
            } else {
                Some(HashMap::new())
            },
            file_extensions: if missing_field == "file-extensions" {
                None
            } else {
                Some(vec![])
            },
            claims_files: if missing_field == "claims-files" {
                None
            } else {
                Some(vec![])
            },
        }
    }

    /// Missing `name` alone → Some(warning) that names "name". (RED: make emitter return None.)
    #[test]
    fn warning_names_missing_name_field() {
        let contrib = make_external_without("name");
        let w = engine_contribution_missing_fields_warning("my-ext", &contrib)
            .expect("should produce a warning when name is absent");
        assert!(
            w.title.contains("name"),
            "warning message should mention 'name': {}",
            w.title
        );
    }

    /// Missing `claims` alone → Some(warning) that names "claims". (RED: make emitter return None.)
    #[test]
    fn warning_names_missing_claims_field() {
        let contrib = make_external_without("claims");
        let w = engine_contribution_missing_fields_warning("my-ext", &contrib)
            .expect("should produce a warning when claims is absent");
        assert!(
            w.title.contains("claims"),
            "warning message should mention 'claims': {}",
            w.title
        );
    }

    /// Missing `file-extensions` alone → Some(warning) that names "file-extensions".
    #[test]
    fn warning_names_missing_file_extensions_field() {
        let contrib = make_external_without("file-extensions");
        let w = engine_contribution_missing_fields_warning("my-ext", &contrib)
            .expect("should produce a warning when file-extensions is absent");
        assert!(
            w.title.contains("file-extensions"),
            "warning message should mention 'file-extensions': {}",
            w.title
        );
    }

    /// Missing `claims-files` alone → Some(warning) that names "claims-files".
    #[test]
    fn warning_names_missing_claims_files_field() {
        let contrib = make_external_without("claims-files");
        let w = engine_contribution_missing_fields_warning("my-ext", &contrib)
            .expect("should produce a warning when claims-files is absent");
        assert!(
            w.title.contains("claims-files"),
            "warning message should mention 'claims-files': {}",
            w.title
        );
    }

    /// All four fields present (some as Some(empty)) → None (no warning).
    /// (RED: treat Some(empty) as missing → this assertion fails.)
    #[test]
    fn no_warning_when_all_fields_present_even_empty() {
        let contrib = make_full_external();
        assert!(
            engine_contribution_missing_fields_warning("my-ext", &contrib).is_none(),
            "Some(empty) fields count as declared — no warning expected"
        );
    }

    /// Reorder variant → None.
    #[test]
    fn no_warning_for_reorder_variant() {
        let contrib = EngineContribution::Reorder {
            name: "jupyter".to_string(),
        };
        assert!(
            engine_contribution_missing_fields_warning("my-ext", &contrib).is_none(),
            "Reorder should never produce a warning"
        );
    }
}
