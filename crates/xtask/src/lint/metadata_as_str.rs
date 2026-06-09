//! Lint rule: don't read metadata strings with `as_str()`.
//!
//! In document-metadata context a bare YAML string value is parsed as markdown
//! and stored as `ConfigValueKind::PandocInlines`, **not** `Scalar(String)`.
//! `ConfigValue::as_str()` returns `None` for `PandocInlines`, so a
//! user-authored front-matter option read with `as_str()` is **silently
//! ignored** unless the user writes the undocumented `!str` escape tag. The
//! correct accessor is `as_plain_text()`, which handles both forms.
//!
//! This bug broke `reference-location` (bd-9ez3ngt1) and a sweep
//! (bd-y89ihf0i) found the same class in `appendix-style`, `license`,
//! `copyright`, `citation.url`, `toc`/`toc-title`, `code-copy`, and `theme`.
//!
//! ## What this rule flags
//!
//! A chain that reads a metadata key and immediately calls `as_str()`:
//!
//! ```ignore
//! meta.get("toc-title").and_then(|v| v.as_str())   // flagged
//! meta.get("theme").as_str()                        // flagged
//! ast.meta.get("license").map(|v| v.as_str())       // flagged
//! ```
//!
//! The receiver of `.get(<string literal>)` must be a *metadata expression* —
//! a path or field access whose final identifier is `meta` or `metadata`
//! (the codebase convention for the merged document metadata, e.g. `meta`,
//! `ast.meta`, `doc.ast.meta`). Internal map reads (`node.plain_data.get(..)`,
//! a `serde_json::Value`, attribute maps) are NOT flagged — their receiver is
//! not a metadata expression.
//!
//! Code inside `#[cfg(test)]` modules and `#[test]` / `#[tokio::test]`
//! functions is skipped (test asserts legitimately inspect generated scalars).
//!
//! ## Suppressing a legitimate site
//!
//! Some sites deliberately read a scalar and handle `PandocInlines`
//! separately (a fast-path before an explicit `match`). Mark those with a
//! comment on the offending line or the line above:
//!
//! ```ignore
//! // lint:allow(metadata-as-str) — fast-path; PandocInlines handled below
//! if let Some(s) = meta.get("x").and_then(|v| v.as_str()) { ... }
//! ```

use std::path::Path;

use anyhow::Result;
use proc_macro2::Span;
use syn::visit::Visit;
use syn::{Expr, ExprMethodCall, File, ImplItemFn, ItemFn, ItemMod, Lit};

use super::Violation;

/// The name of this lint rule.
const RULE_NAME: &str = "metadata-as-str";

/// The marker that suppresses a violation on a line (or the line above).
const ALLOW_MARKER: &str = "lint:allow(metadata-as-str)";

/// Final-identifier names that mark an expression as "document metadata".
const METADATA_IDENTS: &[&str] = &["meta", "metadata"];

/// Check a file for metadata reads using `as_str()`.
pub fn check(path: &Path, content: &str) -> Result<Vec<Violation>> {
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

    let mut visitor = MetaVisitor {
        violations: Vec::new(),
        file_path: path.to_path_buf(),
        lines: content.lines().map(|l| l.to_string()).collect(),
    };
    visitor.visit_file(&syntax_tree);
    Ok(visitor.violations)
}

struct MetaVisitor {
    violations: Vec<Violation>,
    file_path: std::path::PathBuf,
    lines: Vec<String>,
}

impl MetaVisitor {
    fn span_to_location(&self, span: Span) -> (usize, usize) {
        let start = span.start();
        (start.line, start.column + 1)
    }

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
        let (line, column) = self.span_to_location(span);
        if self.is_allowed(line) {
            return;
        }
        self.violations.push(Violation {
            file: self.file_path.clone(),
            line,
            column,
            rule: RULE_NAME,
            message: "metadata string read with `as_str()`; a bare front-matter \
                      string is stored as `PandocInlines`, for which `as_str()` \
                      returns `None`"
                .to_string(),
            suggestion: Some(
                "Use `as_plain_text()` instead (it handles both `Scalar(String)` \
                 and `PandocInlines`). If this site deliberately reads a scalar and \
                 handles `PandocInlines` elsewhere, add `// lint:allow(metadata-as-str)` \
                 with a reason."
                    .to_string(),
            ),
        });
    }

    /// Inspect a method call for the `meta.get("k") … as_str()` shape.
    fn inspect_call(&mut self, call: &ExprMethodCall) {
        let method = call.method.to_string();
        match method.as_str() {
            // Direct chain: `<meta-get>.as_str()`
            "as_str" => {
                if is_metadata_get(&call.receiver) {
                    self.record(call.method.span());
                }
            }
            // Closure forms: `<meta-get>.and_then(|v| v.as_str())` / `.map(...)`
            "and_then" | "map" => {
                if is_metadata_get(&call.receiver) && closure_calls_as_str(call) {
                    self.record(call.method.span());
                }
            }
            _ => {}
        }
    }
}

/// True if `expr` is `<metadata>.get(<string literal>)`.
fn is_metadata_get(expr: &Expr) -> bool {
    let Expr::MethodCall(call) = expr else {
        return false;
    };
    if call.method != "get" {
        return false;
    }
    // Exactly one argument, a string literal.
    if call.args.len() != 1 {
        return false;
    }
    let is_str_lit = matches!(
        call.args.first(),
        Some(Expr::Lit(lit)) if matches!(&lit.lit, Lit::Str(_))
    );
    is_str_lit && is_metadata_expr(&call.receiver)
}

/// True if `expr`'s final identifier is a metadata name (`meta`/`metadata`).
/// Covers `meta`, `ast.meta`, `doc.ast.meta`, `self.metadata`, etc.
fn is_metadata_expr(expr: &Expr) -> bool {
    match expr {
        Expr::Path(p) => p
            .path
            .segments
            .last()
            .is_some_and(|s| METADATA_IDENTS.contains(&s.ident.to_string().as_str())),
        Expr::Field(f) => match &f.member {
            syn::Member::Named(ident) => METADATA_IDENTS.contains(&ident.to_string().as_str()),
            syn::Member::Unnamed(_) => false,
        },
        // `(&meta).get(..)` / `meta.clone().get(..)` are not the convention.
        _ => false,
    }
}

/// True if `call` (an `and_then`/`map`) has a closure argument whose body
/// calls `.as_str()` somewhere.
fn closure_calls_as_str(call: &ExprMethodCall) -> bool {
    call.args.iter().any(|arg| {
        if let Expr::Closure(closure) = arg {
            let mut finder = AsStrFinder { found: false };
            finder.visit_expr(&closure.body);
            finder.found
        } else {
            false
        }
    })
}

/// Finds any `.as_str()` call within an expression subtree.
struct AsStrFinder {
    found: bool,
}

impl<'ast> Visit<'ast> for AsStrFinder {
    fn visit_expr_method_call(&mut self, call: &'ast ExprMethodCall) {
        if call.method == "as_str" {
            self.found = true;
        }
        syn::visit::visit_expr_method_call(self, call);
    }
}

/// True if the attribute list marks test-only code we should skip.
fn is_test_attrs(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        let path = attr.path();
        // `#[test]`
        if path.is_ident("test") {
            return true;
        }
        // `#[tokio::test]`, `#[async_std::test]`, etc.
        if path.segments.last().is_some_and(|s| s.ident == "test") {
            return true;
        }
        // `#[cfg(test)]`
        if path.is_ident("cfg") {
            let mut is_cfg_test = false;
            // Best-effort: parse the nested meta list looking for `test`.
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

impl<'ast> Visit<'ast> for MetaVisitor {
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
        self.inspect_call(node);
        syn::visit::visit_expr_method_call(self, node);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(code: &str) -> Vec<Violation> {
        check(Path::new("test.rs"), code).unwrap()
    }

    #[test]
    fn flags_and_then_as_str_on_meta() {
        let code = r#"
            fn f(meta: &ConfigValue) {
                let _ = meta.get("toc-title").and_then(|v| v.as_str());
            }
        "#;
        let v = run(code);
        assert_eq!(v.len(), 1, "expected one violation, got {v:?}");
        assert_eq!(v[0].rule, "metadata-as-str");
    }

    #[test]
    fn flags_direct_as_str_on_meta() {
        let code = r#"
            fn f(meta: &ConfigValue) {
                let _ = meta.get("theme").as_str();
            }
        "#;
        assert_eq!(run(code).len(), 1);
    }

    #[test]
    fn flags_map_as_str_on_nested_meta() {
        let code = r#"
            fn f(ast: &Pandoc) {
                let _ = ast.meta.get("license").map(|v| v.as_str());
            }
        "#;
        assert_eq!(run(code).len(), 1);
    }

    #[test]
    fn flags_deeply_nested_meta_receiver() {
        let code = r#"
            fn f(doc: &Doc) {
                let _ = doc.ast.meta.get("copyright").and_then(|v| v.as_str());
            }
        "#;
        assert_eq!(run(code).len(), 1);
    }

    #[test]
    fn ignores_as_plain_text() {
        let code = r#"
            fn f(meta: &ConfigValue) {
                let _ = meta.get("toc-title").and_then(|v| v.as_plain_text());
            }
        "#;
        assert!(run(code).is_empty());
    }

    #[test]
    fn ignores_non_metadata_receiver() {
        // Internal map / plain_data reads must not be flagged.
        let code = r#"
            fn f(node: &CustomNode) {
                let _ = node.plain_data.get("ref_type").and_then(|v| v.as_str());
                let _ = json.get("$schema").and_then(|v| v.as_str());
            }
        "#;
        assert!(run(code).is_empty());
    }

    #[test]
    fn ignores_meta_as_str_without_get() {
        // The fast-path `meta.as_str()` (value is itself the metadata scalar)
        // is a different shape and not flagged — those sites handle
        // PandocInlines via an explicit match.
        let code = r#"
            fn f(meta: &ConfigValue) {
                if let Some(s) = meta.as_str() { let _ = s; }
            }
        "#;
        assert!(run(code).is_empty());
    }

    #[test]
    fn skips_test_modules() {
        let code = r#"
            #[cfg(test)]
            mod tests {
                fn helper(meta: &ConfigValue) {
                    let _ = meta.get("canonical-url").and_then(|v| v.as_str());
                }
            }
        "#;
        assert!(run(code).is_empty());
    }

    #[test]
    fn skips_test_functions() {
        let code = r#"
            #[test]
            fn a_test(meta: &ConfigValue) {
                let _ = meta.get("title").and_then(|v| v.as_str());
            }
        "#;
        assert!(run(code).is_empty());
    }

    #[test]
    fn respects_allow_marker_on_line() {
        let code = r#"
            fn f(meta: &ConfigValue) {
                let _ = meta.get("x").and_then(|v| v.as_str()); // lint:allow(metadata-as-str)
            }
        "#;
        assert!(run(code).is_empty());
    }

    #[test]
    fn respects_allow_marker_on_line_above() {
        let code = r#"
            fn f(meta: &ConfigValue) {
                // lint:allow(metadata-as-str) — intentional scalar fast-path
                let _ = meta.get("x").and_then(|v| v.as_str());
            }
        "#;
        assert!(run(code).is_empty());
    }

    #[test]
    fn ignores_unparseable_file() {
        assert!(run("this is not valid rust {{{{").is_empty());
    }
}
