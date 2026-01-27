//! Lint rule: No external-sources references in compile-time macros.
//!
//! This rule detects references to `external-sources/` in macros like `include_dir!`
//! that embed files at compile time. Such references break builds because:
//!
//! 1. `external-sources/` is not version-controlled
//! 2. CI environments don't have it checked out
//! 3. Different developers may have different versions or none at all
//!
//! Resources needed at compile time must be copied to a local directory
//! (like `resources/`) that IS version-controlled.

use std::path::Path;

use anyhow::Result;
use proc_macro2::Span;
use syn::visit::Visit;
use syn::{File, Lit, Macro};

use super::Violation;

/// The name of this lint rule.
const RULE_NAME: &str = "external-sources-in-macro";

/// Macros that embed files at compile time and should not reference external-sources.
const COMPILE_TIME_MACROS: &[&str] = &["include_dir", "include_str", "include_bytes", "include"];

/// Check a file for external-sources references in compile-time macros.
pub fn check(path: &Path, content: &str) -> Result<Vec<Violation>> {
    // Parse the file with syn
    let syntax_tree: File = match syn::parse_file(content) {
        Ok(tree) => tree,
        Err(e) => {
            // If the file doesn't parse, we can't check it.
            // This shouldn't happen for committed code, but we handle it gracefully.
            eprintln!(
                "Warning: Could not parse {}: {} (skipping)",
                path.display(),
                e
            );
            return Ok(Vec::new());
        }
    };

    // Visit the AST to find macro invocations
    let mut visitor = MacroVisitor {
        violations: Vec::new(),
        file_path: path.to_path_buf(),
    };

    visitor.visit_file(&syntax_tree);

    Ok(visitor.violations)
}

/// AST visitor that finds macro invocations and checks for violations.
struct MacroVisitor {
    violations: Vec<Violation>,
    file_path: std::path::PathBuf,
}

impl MacroVisitor {
    /// Convert a proc_macro2::Span to line and column numbers.
    fn span_to_location(&self, span: Span) -> (usize, usize) {
        let start = span.start();
        (start.line, start.column + 1) // Column is 0-indexed in proc_macro2
    }

    /// Check if a macro path matches one of the compile-time macros we care about.
    fn is_compile_time_macro(&self, mac: &Macro) -> bool {
        // Get the last segment of the path (e.g., "include_dir" from "include_dir!" or "foo::include_dir!")
        if let Some(segment) = mac.path.segments.last() {
            let name = segment.ident.to_string();
            return COMPILE_TIME_MACROS.contains(&name.as_str());
        }
        false
    }

    /// Extract string literals from macro tokens and check for violations.
    fn check_macro_tokens(&mut self, mac: &Macro) {
        // Parse the macro tokens to find string literals
        // The tokens inside include_dir!("path/to/dir") are a TokenStream
        let tokens = &mac.tokens;

        // Try to parse as a literal expression
        if let Ok(lit) = syn::parse2::<Lit>(tokens.clone()) {
            self.check_literal(&lit);
            return;
        }

        // If that didn't work, iterate through the tokens looking for string literals
        // This handles cases like include_dir!($crate_root, "path")
        for token in tokens.clone() {
            if let proc_macro2::TokenTree::Literal(lit) = token {
                // Try to parse as a syn::Lit to get the string value
                let lit_str = lit.to_string();
                // String literals in TokenStream are quoted, so check for external-sources
                if lit_str.contains("external-sources") {
                    let (line, column) = self.span_to_location(lit.span());
                    self.violations.push(Violation {
                        file: self.file_path.clone(),
                        line,
                        column,
                        rule: RULE_NAME,
                        message: format!(
                            "compile-time macro contains reference to 'external-sources/': {}",
                            lit_str
                        ),
                        suggestion: Some(
                            "Copy the required files to a local directory (e.g., resources/) \
                             and reference that instead. See resources/scss/README.md for an example."
                                .to_string(),
                        ),
                    });
                }
            } else if let proc_macro2::TokenTree::Group(group) = token {
                // Recursively check groups (parentheses, brackets, braces)
                for inner_token in group.stream() {
                    if let proc_macro2::TokenTree::Literal(lit) = inner_token {
                        let lit_str = lit.to_string();
                        if lit_str.contains("external-sources") {
                            let (line, column) = self.span_to_location(lit.span());
                            self.violations.push(Violation {
                                file: self.file_path.clone(),
                                line,
                                column,
                                rule: RULE_NAME,
                                message: format!(
                                    "compile-time macro contains reference to 'external-sources/': {}",
                                    lit_str
                                ),
                                suggestion: Some(
                                    "Copy the required files to a local directory (e.g., resources/) \
                                     and reference that instead. See resources/scss/README.md for an example."
                                        .to_string(),
                                ),
                            });
                        }
                    }
                }
            }
        }
    }

    /// Check a literal for external-sources references.
    fn check_literal(&mut self, lit: &Lit) {
        if let Lit::Str(lit_str) = lit {
            let value = lit_str.value();
            if value.contains("external-sources") {
                let (line, column) = self.span_to_location(lit_str.span());
                self.violations.push(Violation {
                    file: self.file_path.clone(),
                    line,
                    column,
                    rule: RULE_NAME,
                    message: format!(
                        "compile-time macro contains reference to 'external-sources/': \"{}\"",
                        value
                    ),
                    suggestion: Some(
                        "Copy the required files to a local directory (e.g., resources/) \
                         and reference that instead. See resources/scss/README.md for an example."
                            .to_string(),
                    ),
                });
            }
        }
    }
}

impl<'ast> Visit<'ast> for MacroVisitor {
    fn visit_macro(&mut self, mac: &'ast Macro) {
        if self.is_compile_time_macro(mac) {
            self.check_macro_tokens(mac);
        }

        // Continue visiting nested items
        syn::visit::visit_macro(self, mac);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detects_include_dir_with_external_sources() {
        let code = r#"
            static DIR: Dir<'static> = include_dir!(
                "$CARGO_MANIFEST_DIR/../../external-sources/quarto-cli/src/resources"
            );
        "#;

        let violations = check(Path::new("test.rs"), code).unwrap();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule, "external-sources-in-macro");
        assert!(violations[0].message.contains("external-sources"));
    }

    #[test]
    fn test_ignores_include_dir_with_local_path() {
        let code = r#"
            static DIR: Dir<'static> = include_dir!(
                "$CARGO_MANIFEST_DIR/../../resources/scss/bootstrap"
            );
        "#;

        let violations = check(Path::new("test.rs"), code).unwrap();
        assert!(violations.is_empty());
    }

    #[test]
    fn test_detects_include_str_with_external_sources() {
        let code = r#"
            const CONTENT: &str = include_str!("../../external-sources/file.txt");
        "#;

        let violations = check(Path::new("test.rs"), code).unwrap();
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn test_detects_include_bytes_with_external_sources() {
        let code = r#"
            const DATA: &[u8] = include_bytes!("external-sources/data.bin");
        "#;

        let violations = check(Path::new("test.rs"), code).unwrap();
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn test_ignores_non_compile_time_macros() {
        let code = r#"
            // This is not a compile-time macro, so we don't flag it
            let path = format!("external-sources/some/path");
            println!("external-sources is mentioned but not embedded");
        "#;

        let violations = check(Path::new("test.rs"), code).unwrap();
        assert!(violations.is_empty());
    }

    #[test]
    fn test_handles_unparseable_files() {
        let code = "this is not valid rust {{{{";

        // Should not panic, just return empty
        let violations = check(Path::new("invalid.rs"), code).unwrap();
        assert!(violations.is_empty());
    }

    #[test]
    fn test_multiple_violations_in_one_file() {
        let code = r#"
            static A: Dir<'static> = include_dir!("external-sources/a");
            static B: Dir<'static> = include_dir!("external-sources/b");
            const C: &str = include_str!("external-sources/c.txt");
        "#;

        let violations = check(Path::new("test.rs"), code).unwrap();
        assert_eq!(violations.len(), 3);
    }
}
