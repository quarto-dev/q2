//! Lint: restrict `SourceContext::add_file_with_id` to blessed modules.
//!
//! `add_file_with_id(id, path, content)` lets a caller pair an arbitrary
//! FileId with arbitrary path/content. When the id was resolved from a
//! diagnostic's `SourceInfo` and the path is merely *assumed* (e.g.
//! "config errors come from `_quarto.yml`"), the assumption can be wrong —
//! merged config values keep pointing into the file they were written in
//! (`_extension.yml`, `_metadata.yml`, an included file). Binding the wrong
//! file renders the right offsets against the wrong text: a confidently
//! wrong ariadne span when the offsets fit, a silently dropped snippet when
//! they don't. This caused bd-m6wmztln (PR #478); bd-nv4p0eb1 is the
//! tree-wide audit; bd-jrq4hroi added this lint.
//!
//! The safe pattern is `quarto_core::config_sources::bind_config_source`,
//! which re-derives each candidate file's id from its path and registers
//! only a match — the wrong pairing is unrepresentable through it.
//!
//! Allowed without a marker:
//! - Blessed modules (see `BLESSED_SUFFIXES`) whose bindings derive id,
//!   path, and content from a single path, or that implement the candidate
//!   matching itself.
//! - Test code (`#[cfg(test)]` modules, `#[test]`/`#[tokio::test]` fns) —
//!   test contexts are self-consistent by construction.
//!
//! Everywhere else, suppress a deliberate use with
//! `// lint:allow(add-file-with-id)` on the line or the line above, with a
//! reason.

use std::path::Path;

use anyhow::Result;
use proc_macro2::Span;
use syn::visit::Visit;
use syn::{ExprMethodCall, File, ImplItemFn, ItemFn, ItemMod};

use super::Violation;

const RULE_NAME: &str = "add-file-with-id";

/// The marker that suppresses a violation on a line (or the line above).
const ALLOW_MARKER: &str = "lint:allow(add-file-with-id)";

/// Files (matched by path suffix) where `add_file_with_id` is allowed.
const BLESSED_SUFFIXES: &[&str] = &[
    // Implements the candidate-matching helper itself.
    "quarto-core/src/config_sources.rs",
    // Derives id, path, and content from one path (the safe shape).
    "quarto-core/src/stage/stages/metadata_merge.rs",
    // Test-only span-assertion helpers; self-consistent by construction.
    "quarto-config/src/span_assert.rs",
    // TEMPORARY: rewritten by PR #478 (bd-m6wmztln) to use
    // bind_config_source; blessed here to avoid inline-comment conflicts
    // with the in-flight branch. Remove after #478 merges.
    "quarto-core/src/project/render_scripts.rs",
    // TEMPORARY: doc-level path is bd-x113wg9v; project-level path is
    // rewritten by PR #478. Remove once both land.
    "quarto-core/src/project_resources.rs",
];

/// Check a file for `add_file_with_id` calls outside blessed locations.
pub fn check(path: &Path, content: &str) -> Result<Vec<Violation>> {
    // Cheap pre-filter before parsing.
    if !content.contains("add_file_with_id") {
        return Ok(Vec::new());
    }

    let path_str = path.to_string_lossy().replace('\\', "/");
    if BLESSED_SUFFIXES.iter().any(|s| path_str.ends_with(s)) {
        return Ok(Vec::new());
    }

    let syntax_tree: File = match syn::parse_file(content) {
        Ok(tree) => tree,
        Err(e) => {
            eprintln!(
                "Warning: Could not parse {}: {} (skipping)",
                path.display(),
                e
            );
            return Ok(Vec::new());
        }
    };

    let mut visitor = AddFileVisitor {
        violations: Vec::new(),
        file_path: path.to_path_buf(),
        lines: content.lines().map(|l| l.to_string()).collect(),
    };
    visitor.visit_file(&syntax_tree);
    Ok(visitor.violations)
}

struct AddFileVisitor {
    violations: Vec<Violation>,
    file_path: std::path::PathBuf,
    lines: Vec<String>,
}

impl AddFileVisitor {
    /// True if the line at `line` (1-indexed) or the line above carries the
    /// allow marker.
    fn is_allowed(&self, line: usize) -> bool {
        let on = line
            .checked_sub(1)
            .and_then(|i| self.lines.get(i))
            .is_some_and(|l| l.contains(ALLOW_MARKER));
        let above = line
            .checked_sub(2)
            .and_then(|i| self.lines.get(i))
            .is_some_and(|l| l.contains(ALLOW_MARKER));
        on || above
    }

    fn record(&mut self, span: Span) {
        let start = span.start();
        let (line, column) = (start.line, start.column + 1);
        if self.is_allowed(line) {
            return;
        }
        self.violations.push(Violation {
            file: self.file_path.clone(),
            line,
            column,
            rule: RULE_NAME,
            message: "`add_file_with_id` pairs an arbitrary FileId with arbitrary \
                      content; binding an assumed file to a diagnostic's resolved \
                      id renders byte offsets against the wrong text (bd-m6wmztln)"
                .to_string(),
            suggestion: Some(
                "Use `quarto_core::config_sources::bind_config_source` with the \
                 candidate files this value can originate from — it re-derives \
                 each candidate's FileId and registers only a match. If this \
                 site provably derives id, path, and content from one path, add \
                 `// lint:allow(add-file-with-id)` with a reason."
                    .to_string(),
            ),
        });
    }
}

/// True if the attribute list marks test-only code we should skip.
fn is_test_attrs(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        let path = attr.path();
        if path.is_ident("test") {
            return true;
        }
        if path.segments.last().is_some_and(|s| s.ident == "test") {
            return true;
        }
        if path.is_ident("cfg") {
            let mut is_cfg_test = false;
            let _ = attr.parse_nested_meta(|nested| {
                if nested.path.is_ident("test") {
                    is_cfg_test = true;
                }
                Ok(())
            });
            return is_cfg_test;
        }
        false
    })
}

impl<'ast> Visit<'ast> for AddFileVisitor {
    fn visit_item_mod(&mut self, node: &'ast ItemMod) {
        if is_test_attrs(&node.attrs) {
            return; // skip `#[cfg(test)] mod tests { ... }`
        }
        syn::visit::visit_item_mod(self, node);
    }

    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        if is_test_attrs(&node.attrs) {
            return;
        }
        syn::visit::visit_item_fn(self, node);
    }

    fn visit_impl_item_fn(&mut self, node: &'ast ImplItemFn) {
        if is_test_attrs(&node.attrs) {
            return;
        }
        syn::visit::visit_impl_item_fn(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        if node.method == "add_file_with_id" {
            self.record(node.method.span());
        }
        syn::visit::visit_expr_method_call(self, node);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn check_str(path: &str, content: &str) -> Vec<Violation> {
        check(&PathBuf::from(path), content).unwrap()
    }

    #[test]
    fn flags_bare_call_in_production_code() {
        let src = r#"
            fn diag(ctx: &mut SourceContext) {
                ctx.add_file_with_id(id, path, content);
            }
        "#;
        let v = check_str("crates/quarto-core/src/somewhere.rs", src);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "add-file-with-id");
    }

    #[test]
    fn allow_marker_on_line_suppresses() {
        let src = r#"
            fn diag(ctx: &mut SourceContext) {
                ctx.add_file_with_id(id, path, content); // lint:allow(add-file-with-id) — derived from one path
            }
        "#;
        assert!(check_str("crates/quarto-core/src/somewhere.rs", src).is_empty());
    }

    #[test]
    fn allow_marker_on_line_above_suppresses() {
        let src = r#"
            fn diag(ctx: &mut SourceContext) {
                // lint:allow(add-file-with-id) — candidate-matched above
                ctx.add_file_with_id(id, path, content);
            }
        "#;
        assert!(check_str("crates/quarto-core/src/somewhere.rs", src).is_empty());
    }

    #[test]
    fn blessed_file_is_skipped() {
        let src = r#"
            fn bind(ctx: &mut SourceContext) {
                ctx.add_file_with_id(id, path, content);
            }
        "#;
        assert!(check_str("crates/quarto-core/src/config_sources.rs", src).is_empty());
        assert!(check_str("crates/quarto-core/src/stage/stages/metadata_merge.rs", src).is_empty());
    }

    #[test]
    fn test_code_is_skipped() {
        let src = r#"
            #[cfg(test)]
            mod tests {
                fn helper(ctx: &mut SourceContext) {
                    ctx.add_file_with_id(id, path, content);
                }
            }

            #[test]
            fn direct_test() {
                ctx.add_file_with_id(id, path, content);
            }
        "#;
        assert!(check_str("crates/quarto-core/src/somewhere.rs", src).is_empty());
    }

    #[test]
    fn windows_paths_match_blessed_suffixes() {
        let src = r#"
            fn bind(ctx: &mut SourceContext) {
                ctx.add_file_with_id(id, path, content);
            }
        "#;
        assert!(check_str("crates\\quarto-core\\src\\config_sources.rs", src).is_empty());
    }
}
