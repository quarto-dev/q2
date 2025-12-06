/*
 * template/builtin.rs
 * Copyright (c) 2025 Posit, PBC
 */

//! Built-in templates shipped with pampa.
//!
//! These templates are embedded in the binary and can be used without
//! filesystem access. They are based on Pandoc's default templates.

use crate::template::bundle::TemplateBundle;

/// List of available built-in template names.
pub const BUILTIN_TEMPLATE_NAMES: &[&str] = &["html", "plain"];

/// Get a built-in template bundle by name.
///
/// Returns `None` if the name is not a recognized built-in template.
pub fn get_builtin_template(name: &str) -> Option<TemplateBundle> {
    match name {
        "html" => Some(html_bundle()),
        "plain" => Some(plain_bundle()),
        _ => None,
    }
}

/// Check if a name refers to a built-in template.
pub fn is_builtin_template(name: &str) -> bool {
    BUILTIN_TEMPLATE_NAMES.contains(&name)
}

/// Create the html template bundle.
///
/// Based on Pandoc's default.html5 template.
fn html_bundle() -> TemplateBundle {
    TemplateBundle::new(HTML_TEMPLATE)
        .with_partial("styles.html", STYLES_HTML)
        .with_partial("styles.citations.html", STYLES_CITATIONS_HTML)
}

/// Create the plain template bundle.
///
/// A minimal template that just outputs the body.
fn plain_bundle() -> TemplateBundle {
    TemplateBundle::new(PLAIN_TEMPLATE)
}

// =============================================================================
// Template content
// =============================================================================

/// HTML template based on Pandoc's default.html5.
/// Loaded from resources/templates/html/main.html
const HTML_TEMPLATE: &str = include_str!("../../resources/templates/html/main.html");

/// Pandoc's styles.html partial.
/// Loaded from resources/templates/html/styles.html
const STYLES_HTML: &str = include_str!("../../resources/templates/html/styles.html");

/// Pandoc's styles.citations.html partial.
/// Loaded from resources/templates/html/styles.citations.html
const STYLES_CITATIONS_HTML: &str =
    include_str!("../../resources/templates/html/styles.citations.html");

/// A minimal plain template that just outputs the body.
const PLAIN_TEMPLATE: &str = "$body$\n";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_builtin_template_html() {
        let bundle = get_builtin_template("html").unwrap();
        assert!(bundle.main.contains("<!DOCTYPE html>"));
        assert!(bundle.partials.contains_key("styles.html"));
        assert!(bundle.partials.contains_key("styles.citations.html"));
    }

    #[test]
    fn test_get_builtin_template_plain() {
        let bundle = get_builtin_template("plain").unwrap();
        assert_eq!(bundle.main, "$body$\n");
        assert!(bundle.partials.is_empty());
    }

    #[test]
    fn test_get_builtin_template_unknown() {
        assert!(get_builtin_template("unknown").is_none());
    }

    #[test]
    fn test_is_builtin_template() {
        assert!(is_builtin_template("html"));
        assert!(is_builtin_template("plain"));
        assert!(!is_builtin_template("unknown"));
    }

    #[test]
    fn test_html_template_compiles() {
        let bundle = html_bundle();
        // Should compile without error
        let result = bundle.compile("html.html");
        assert!(
            result.is_ok(),
            "html template should compile: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_plain_template_compiles() {
        let bundle = plain_bundle();
        let result = bundle.compile("plain.txt");
        assert!(
            result.is_ok(),
            "plain template should compile: {:?}",
            result.err()
        );
    }
}
