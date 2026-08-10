//! Quarto's centralized `Q-*` error-code catalog.
//!
//! `quarto-error-reporting` is catalog-agnostic — it defines the catalog
//! *shape* and the [`CatalogProvider`] seam but ships no data. This crate
//! carries Quarto's *policy*: the `Q-<subsystem>-<n>` codes, their titles and
//! `quarto.org` documentation URLs (`error_catalog.json`), and a
//! [`CatalogProvider`] implementation over them.
//!
//! An embedding binary installs this catalog once, early (e.g. at the top of
//! `main`), via [`install`]:
//!
//! ```no_run
//! quarto_error_catalog::install();
//! // From here, quarto_error_reporting::get_docs_url("Q-0-1") resolves.
//! ```
//!
//! See `claude-notes/designs/cross-package-error-codes.md` for the discipline
//! this implements (this crate is the q2 *presentation*-code policy).

use once_cell::sync::Lazy;
use quarto_error_reporting::{CatalogProvider, ErrorCodeInfo, install_catalog};
use std::collections::HashMap;

/// The Quarto `Q-*` error catalog, loaded once from the embedded
/// `error_catalog.json`.
///
/// The JSON is embedded at compile time via `include_str!`, so there is no
/// runtime file I/O.
///
/// # Panics
///
/// Panics if the embedded JSON is invalid — only possible if someone edits the
/// catalog incorrectly during development.
pub static ERROR_CATALOG: Lazy<HashMap<String, ErrorCodeInfo>> = Lazy::new(|| {
    let json_data = include_str!("../error_catalog.json");
    serde_json::from_str(json_data).expect("Invalid error catalog JSON - this is a bug in Quarto")
});

/// A [`CatalogProvider`] backed by Quarto's embedded `Q-*` catalog.
pub struct QuartoCatalog;

impl CatalogProvider for QuartoCatalog {
    fn lookup(&self, code: &str) -> Option<&ErrorCodeInfo> {
        // `ERROR_CATALOG` is `'static`; the returned reference outlives `&self`.
        ERROR_CATALOG.get(code)
    }
}

/// Install Quarto's `Q-*` catalog as the process-wide
/// [`CatalogProvider`](quarto_error_reporting::CatalogProvider).
///
/// Idempotent (first install wins). Call once, as early as possible, from a
/// binary's `main` / the WASM bootstrap, before any diagnostic's docs URL is
/// resolved. Behaviour is unaffected if called multiple times.
pub fn install() {
    install_catalog(Box::new(QuartoCatalog));
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Catalog data presence (ported from quarto-error-reporting) ──────────
    //
    // These assert the *data* in `error_catalog.json` directly via the embedded
    // map, so they do not depend on the installed-global state.

    #[test]
    fn catalog_loads_and_is_nonempty() {
        assert!(!ERROR_CATALOG.is_empty());
    }

    #[test]
    fn internal_error_q_0_1_exists() {
        let info = ERROR_CATALOG.get("Q-0-1").expect("Q-0-1 must exist");
        assert_eq!(info.subsystem, "internal");
        assert_eq!(info.title, "Internal Error");
        assert!(info.docs_url.is_some());
        assert!(
            info.docs_url
                .as_deref()
                .unwrap()
                .starts_with("https://quarto.org/docs/errors/")
        );
    }

    #[test]
    fn nonexistent_code_is_absent() {
        assert!(ERROR_CATALOG.get("Q-999-999").is_none()); // quarto-error-code-audit-ignore
    }

    // bd-6d2wj4zp: Q-2-40 catalog presence (`.md` engine-ignored warning).
    #[test]
    fn error_catalog_has_q_2_40() {
        let info = ERROR_CATALOG
            .get("Q-2-40")
            .expect("Q-2-40 must be in the catalog");
        assert_eq!(info.subsystem, "markdown");
        assert_eq!(
            info.title,
            "Engine Specification Ignored for Markdown Input"
        );
        assert!(
            info.message_template.contains("never execute engines"),
            "Q-2-40 message must state the policy; got: {}",
            info.message_template
        );
    }

    // bd-cx1det1y: Q-2-36/37/38 catalog presence (backfill — these corpus
    // codes shipped without catalog entries).
    #[test]
    fn error_catalog_has_q_2_36() {
        let info = ERROR_CATALOG
            .get("Q-2-36")
            .expect("Q-2-36 must be in the catalog");
        assert_eq!(info.subsystem, "markdown");
        assert_eq!(
            info.title,
            "Old-style knitr chunk options are not supported"
        );
        assert!(
            info.message_template.contains("#| key: value"),
            "Q-2-36 message must point at the body-options syntax; got: {}",
            info.message_template
        );
    }

    #[test]
    fn error_catalog_has_q_2_37() {
        let info = ERROR_CATALOG
            .get("Q-2-37")
            .expect("Q-2-37 must be in the catalog");
        assert_eq!(info.subsystem, "markdown");
        assert_eq!(info.title, "Line break in link destination");
        assert!(
            info.message_template.contains("line break"),
            "Q-2-37 message must name the line-break restriction; got: {}",
            info.message_template
        );
    }

    #[test]
    fn error_catalog_has_q_2_38() {
        let info = ERROR_CATALOG
            .get("Q-2-38")
            .expect("Q-2-38 must be in the catalog");
        assert_eq!(info.subsystem, "markdown");
        assert_eq!(info.title, "Unclosed Attribute Specifier");
        assert!(
            info.message_template.contains("closing '}'"),
            "Q-2-38 message must mention the missing closing brace; got: {}",
            info.message_template
        );
    }

    // L8 / bd-rqgx: Q-12-14 catalog presence.
    #[test]
    fn error_catalog_has_q_12_14() {
        let info = ERROR_CATALOG
            .get("Q-12-14")
            .expect("Q-12-14 must be in the catalog");
        assert_eq!(info.subsystem, "listing");
        assert_eq!(info.title, "Listing Type custom Without template Path");
        assert!(
            info.message_template.contains("type: custom"),
            "Q-12-14 message must mention `type: custom`; got: {}",
            info.message_template
        );
        assert!(
            info.message_template.contains("default"),
            "Q-12-14 message must mention the default fallback; got: {}",
            info.message_template
        );
    }

    // bd-8d6rk: Q-13-1..7 navigation subsystem catalog presence.
    #[test]
    fn error_catalog_has_q_13_navigation_codes() {
        let cases: &[(&str, &str, &str)] = &[
            ("Q-13-1", "Sidebar", "missing document"),
            ("Q-13-2", "Navbar", "missing document"),
            ("Q-13-3", "Page footer", "missing document"),
            ("Q-13-4", "Body link", "missing document"),
            ("Q-13-5", "auto:", "project index"),
            ("Q-13-6", "auto:", "no documents"),
            ("Q-13-7", "Page navigation", "missing document"),
        ];
        for (code, title_substr, message_substr) in cases {
            let info = ERROR_CATALOG
                .get(*code)
                .unwrap_or_else(|| panic!("{} must be in the catalog", code));
            assert_eq!(
                info.subsystem, "navigation",
                "{} should be in the navigation subsystem; got: {}",
                code, info.subsystem
            );
            assert!(
                info.title.contains(title_substr),
                "{} title must mention `{}`; got: {}",
                code,
                title_substr,
                info.title
            );
            assert!(
                info.message_template.contains(message_substr),
                "{} message must mention `{}`; got: {}",
                code,
                message_substr,
                info.message_template
            );
            assert!(
                info.docs_url.as_deref().is_some_and(|u| u.ends_with(code)),
                "{} docs_url must end with {}; got: {:?}",
                code,
                code,
                info.docs_url
            );
        }
    }

    // L9 / bd-o90m: Q-12-15 + Q-12-16 catalog presence.
    #[test]
    fn error_catalog_has_q_12_15_and_q_12_16() {
        let q15 = ERROR_CATALOG
            .get("Q-12-15")
            .expect("Q-12-15 must be in the catalog");
        assert_eq!(q15.subsystem, "listing");
        assert!(
            q15.title.to_lowercase().contains("feed"),
            "Q-12-15 title must mention feed; got: {}",
            q15.title
        );
        assert!(
            q15.message_template.contains("site-url"),
            "Q-12-15 message must mention `site-url`; got: {}",
            q15.message_template
        );

        let q16 = ERROR_CATALOG
            .get("Q-12-16")
            .expect("Q-12-16 must be in the catalog");
        assert_eq!(q16.subsystem, "listing");
        assert!(
            q16.title.to_lowercase().contains("feed"),
            "Q-12-16 title must mention feed; got: {}",
            q16.title
        );
        assert!(
            q16.message_template.contains("description"),
            "Q-12-16 message must mention the empty description fallback; got: {}",
            q16.message_template
        );
    }

    // bd-bxrkxblx: Q-5-6 / Q-5-7 resource-copy diagnostics.
    #[test]
    fn error_catalog_has_q_5_6_and_q_5_7() {
        let q6 = ERROR_CATALOG
            .get("Q-5-6")
            .expect("Q-5-6 must be in the catalog");
        assert_eq!(q6.subsystem, "project");
        assert!(
            q6.title.to_lowercase().contains("resource"),
            "Q-5-6 title must mention `resource`; got: {}",
            q6.title
        );
        assert!(
            q6.message_template.to_lowercase().contains("not exist")
                || q6.message_template.to_lowercase().contains("missing")
                || q6.message_template.to_lowercase().contains("not found"),
            "Q-5-6 message must describe the missing source; got: {}",
            q6.message_template
        );
        assert!(
            q6.docs_url.as_deref().is_some_and(|u| u.ends_with("Q-5-6")),
            "Q-5-6 docs_url must end with the code; got: {:?}",
            q6.docs_url
        );

        let q7 = ERROR_CATALOG
            .get("Q-5-7")
            .expect("Q-5-7 must be in the catalog");
        assert_eq!(q7.subsystem, "project");
        assert!(
            q7.title.to_lowercase().contains("copy")
                || q7.title.to_lowercase().contains("resource"),
            "Q-5-7 title must mention copy/resource; got: {}",
            q7.title
        );
        assert!(
            q7.message_template.to_lowercase().contains("copy"),
            "Q-5-7 message must mention copying; got: {}",
            q7.message_template
        );
        assert!(
            q7.docs_url.as_deref().is_some_and(|u| u.ends_with("Q-5-7")),
            "Q-5-7 docs_url must end with the code; got: {:?}",
            q7.docs_url
        );
    }

    // bd-rr6qzcvu: Q-15-1 — the crossref subsystem's first code.
    #[test]
    fn error_catalog_has_q_15_1_crossref() {
        let info = ERROR_CATALOG
            .get("Q-15-1")
            .expect("Q-15-1 must be in the catalog");
        assert_eq!(info.subsystem, "crossref");
        assert!(
            info.title.to_lowercase().contains("duplicate")
                && info.title.to_lowercase().contains("crossref"),
            "Q-15-1 title must mention a duplicate crossref; got: {}",
            info.title
        );
        assert!(
            info.message_template.to_lowercase().contains("unique")
                || info
                    .message_template
                    .to_lowercase()
                    .contains("more than once"),
            "Q-15-1 message must explain the uniqueness requirement; got: {}",
            info.message_template
        );
        assert!(
            info.docs_url
                .as_deref()
                .is_some_and(|u| u.ends_with("Q-15-1")),
            "Q-15-1 docs_url must end with the code; got: {:?}",
            info.docs_url
        );
    }

    // ─── Integration: install() wires this catalog into quarto-error-reporting ─
    //
    // These exercise the installed-global delegation path end-to-end. Each calls
    // `install()` (idempotent); under nextest every test is its own process, and
    // they all install the *same* QuartoCatalog, so there is no conflict.

    #[test]
    fn install_makes_get_docs_url_resolve() {
        install();
        let url = quarto_error_reporting::get_docs_url("Q-0-1")
            .expect("Q-0-1 should resolve after install()");
        assert!(url.starts_with("https://quarto.org/docs/errors/"));
        assert!(url.contains("Q-0-1"));
    }

    #[test]
    fn install_makes_get_subsystem_resolve() {
        install();
        assert_eq!(
            quarto_error_reporting::get_subsystem("Q-0-1"),
            Some("internal")
        );
        assert!(quarto_error_reporting::get_subsystem("Q-999-999").is_none()); // quarto-error-code-audit-ignore
    }

    // Relocated from quarto-error-reporting's `diagnostic.rs::test_docs_url`:
    // a DiagnosticMessage's `docs_url()` resolves once the Q-* catalog is installed.
    #[test]
    fn diagnostic_docs_url_resolves_after_install() {
        install();
        let msg =
            quarto_error_reporting::DiagnosticMessage::error("Internal Error").with_code("Q-0-1");
        let url = msg
            .docs_url()
            .expect("docs_url should resolve after install()");
        assert!(url.contains("Q-0-1"));
    }
}
