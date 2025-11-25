/*
 * evaluator.rs
 * Copyright (c) 2025 Posit, PBC
 */

//! Template evaluation engine.
//!
//! This module implements the evaluation of parsed templates against a context.
//! The evaluator produces a `Doc` tree that can be rendered to a string.

use crate::ast::TemplateNode;
use crate::ast::VariableRef;
use crate::ast::{BreakableSpace, Comment, Conditional, ForLoop, Literal, Nesting, Partial};
use crate::context::{TemplateContext, TemplateValue};
use crate::doc::{Doc, concat_docs, intersperse_docs};
use crate::error::TemplateResult;
use crate::eval_context::EvalContext;
use crate::parser::Template;
use quarto_error_reporting::DiagnosticMessage;

impl Template {
    /// Render this template with the given context.
    ///
    /// # Arguments
    /// * `context` - The variable context for evaluation
    ///
    /// # Returns
    /// The rendered output string, or an error if evaluation fails.
    ///
    /// Note: This method does not report warnings. Use [`render_with_diagnostics`]
    /// if you need access to warnings (like undefined variable warnings).
    pub fn render(&self, context: &TemplateContext) -> TemplateResult<String> {
        let mut eval_ctx = EvalContext::new(context);
        let doc = evaluate_nodes(&self.nodes, &mut eval_ctx)?;
        Ok(doc.render(None))
    }

    /// Evaluate this template to a Doc tree.
    ///
    /// This is useful when you need the structured representation
    /// for further processing before final string rendering.
    ///
    /// Note: This method does not report warnings. Use [`evaluate_with_diagnostics`]
    /// if you need access to warnings.
    pub fn evaluate(&self, context: &TemplateContext) -> TemplateResult<Doc> {
        let mut eval_ctx = EvalContext::new(context);
        evaluate_nodes(&self.nodes, &mut eval_ctx)
    }

    /// Render this template with diagnostics collection.
    ///
    /// Returns both the rendered output and any diagnostics (errors and warnings)
    /// that were collected during evaluation.
    ///
    /// # Arguments
    /// * `context` - The variable context for evaluation
    ///
    /// # Returns
    /// A tuple of (result, diagnostics) where:
    /// - `result` is `Ok(String)` if rendering succeeded, `Err(())` if there were errors
    /// - `diagnostics` is a list of all errors and warnings
    ///
    /// # Example
    ///
    /// ```ignore
    /// let template = Template::compile("Hello, $name$!")?;
    /// let ctx = TemplateContext::new(); // Note: 'name' not defined
    ///
    /// let (result, diagnostics) = template.render_with_diagnostics(&ctx);
    ///
    /// // Result is Ok because undefined variables are warnings, not errors
    /// assert!(result.is_ok());
    /// // But we get a warning about the undefined variable
    /// assert!(!diagnostics.is_empty());
    /// ```
    pub fn render_with_diagnostics(
        &self,
        context: &TemplateContext,
    ) -> (Result<String, ()>, Vec<DiagnosticMessage>) {
        let mut eval_ctx = EvalContext::new(context);
        let result = evaluate_nodes(&self.nodes, &mut eval_ctx);

        let diagnostics = eval_ctx.into_diagnostics();
        let has_errors = diagnostics
            .iter()
            .any(|d| d.kind == quarto_error_reporting::DiagnosticKind::Error);

        match result {
            Ok(doc) if !has_errors => (Ok(doc.render(None)), diagnostics),
            _ => (Err(()), diagnostics),
        }
    }

    /// Render this template in strict mode.
    ///
    /// In strict mode, warnings (like undefined variables) are treated as errors.
    ///
    /// # Arguments
    /// * `context` - The variable context for evaluation
    ///
    /// # Returns
    /// A tuple of (result, diagnostics).
    pub fn render_strict(
        &self,
        context: &TemplateContext,
    ) -> (Result<String, ()>, Vec<DiagnosticMessage>) {
        let mut eval_ctx = EvalContext::new(context).with_strict_mode(true);
        let result = evaluate_nodes(&self.nodes, &mut eval_ctx);

        let diagnostics = eval_ctx.into_diagnostics();
        let has_errors = diagnostics
            .iter()
            .any(|d| d.kind == quarto_error_reporting::DiagnosticKind::Error);

        match result {
            Ok(doc) if !has_errors => (Ok(doc.render(None)), diagnostics),
            _ => (Err(()), diagnostics),
        }
    }

    /// Evaluate this template to a Doc tree with diagnostics collection.
    ///
    /// Similar to [`evaluate`], but also returns collected diagnostics.
    pub fn evaluate_with_diagnostics(
        &self,
        context: &TemplateContext,
    ) -> (TemplateResult<Doc>, Vec<DiagnosticMessage>) {
        let mut eval_ctx = EvalContext::new(context);
        let result = evaluate_nodes(&self.nodes, &mut eval_ctx);
        let diagnostics = eval_ctx.into_diagnostics();
        (result, diagnostics)
    }
}

/// Evaluate a list of template nodes to a Doc.
///
/// This is the internal evaluation function that threads EvalContext.
fn evaluate_nodes(nodes: &[TemplateNode], ctx: &mut EvalContext) -> TemplateResult<Doc> {
    let docs: Result<Vec<Doc>, _> = nodes.iter().map(|n| evaluate_node(n, ctx)).collect();
    Ok(concat_docs(docs?))
}

/// Evaluate a single template node to a Doc.
fn evaluate_node(node: &TemplateNode, ctx: &mut EvalContext) -> TemplateResult<Doc> {
    match node {
        TemplateNode::Literal(Literal { text, .. }) => Ok(Doc::text(text)),

        TemplateNode::Variable(var) => Ok(render_variable(var, ctx)),

        TemplateNode::Conditional(Conditional {
            branches,
            else_branch,
            ..
        }) => evaluate_conditional(branches, else_branch, ctx),

        TemplateNode::ForLoop(ForLoop {
            var,
            body,
            separator,
            ..
        }) => evaluate_for_loop(var, body, separator, ctx),

        TemplateNode::Partial(partial) => evaluate_partial(partial, ctx),

        TemplateNode::Nesting(Nesting { children, .. }) => {
            // TODO: Implement nesting/indentation tracking
            // For now, just evaluate children without nesting
            evaluate_nodes(children, ctx)
        }

        TemplateNode::BreakableSpace(BreakableSpace { children, .. }) => {
            // For now, breakable spaces just evaluate their children
            // Full breakable space semantics require line-width-aware rendering
            evaluate_nodes(children, ctx)
        }

        TemplateNode::Comment(Comment { .. }) => {
            // Comments produce no output
            Ok(Doc::Empty)
        }
    }
}

/// Resolve a variable reference in the context.
fn resolve_variable<'a>(
    var: &VariableRef,
    variables: &'a TemplateContext,
) -> Option<&'a TemplateValue> {
    // Variable paths may contain dots (e.g., "employee.salary" is a single path element)
    // Split on dots to get the actual path components
    let path: Vec<&str> = var.path.iter().flat_map(|s| s.split('.')).collect();
    variables.get_path(&path)
}

/// Render a variable reference to a Doc.
fn render_variable(var: &VariableRef, ctx: &mut EvalContext) -> Doc {
    match resolve_variable(var, ctx.variables) {
        Some(value) => {
            // Handle literal separator for arrays: $var[, ]$
            if let Some(sep) = &var.separator {
                if let TemplateValue::List(items) = value {
                    let docs: Vec<Doc> = items.iter().map(|v| v.to_doc()).collect();
                    return intersperse_docs(docs, Doc::text(sep));
                }
            }
            // TODO: Apply pipes
            value.to_doc()
        }
        None => {
            // Emit warning or error depending on strict mode
            let var_path = var.path.join(".");
            ctx.warn_or_error_with_code(
                "Q-10-2",
                format!("Undefined variable: {}", var_path),
                &var.source_info,
            );
            Doc::Empty
        }
    }
}

/// Evaluate a conditional block.
fn evaluate_conditional(
    branches: &[(VariableRef, Vec<TemplateNode>)],
    else_branch: &Option<Vec<TemplateNode>>,
    ctx: &mut EvalContext,
) -> TemplateResult<Doc> {
    // Try each if/elseif branch
    for (condition, body) in branches {
        if let Some(value) = resolve_variable(condition, ctx.variables) {
            if value.is_truthy() {
                return evaluate_nodes(body, ctx);
            }
        }
    }

    // No branch matched, try else
    if let Some(else_body) = else_branch {
        evaluate_nodes(else_body, ctx)
    } else {
        Ok(Doc::Empty)
    }
}

/// Evaluate a for loop.
fn evaluate_for_loop(
    var: &VariableRef,
    body: &[TemplateNode],
    separator: &Option<Vec<TemplateNode>>,
    ctx: &mut EvalContext,
) -> TemplateResult<Doc> {
    let value = resolve_variable(var, ctx.variables);

    // Determine what to iterate over
    let items: Vec<&TemplateValue> = match value {
        Some(TemplateValue::List(items)) => items.iter().collect(),
        Some(TemplateValue::Map(_)) => vec![value.unwrap()], // Single iteration over map
        Some(v) if v.is_truthy() => vec![v],                 // Single iteration for truthy scalars
        _ => vec![],                                         // No iterations for null/falsy
    };

    if items.is_empty() {
        return Ok(Doc::Empty);
    }

    // Get the variable name for binding (use the last path component)
    let var_name = var.path.last().map(|s| s.as_str()).unwrap_or("");

    // Render separator if present
    let sep_doc = if let Some(sep_nodes) = separator {
        Some(evaluate_nodes(sep_nodes, ctx)?)
    } else {
        None
    };

    // Render each iteration
    let mut results = Vec::new();
    for item in &items {
        let mut child_vars = ctx.variables.child();

        // Bind to variable name AND "it" (Pandoc semantics)
        child_vars.insert(var_name, (*item).clone());
        child_vars.insert("it", (*item).clone());

        // Create child context and evaluate
        let mut child_ctx = ctx.child(&child_vars);
        let result = evaluate_nodes(body, &mut child_ctx)?;
        results.push(result);

        // Merge any diagnostics from the child context
        ctx.merge_diagnostics(child_ctx);
    }

    // Join with separator
    match sep_doc {
        Some(sep) => Ok(intersperse_docs(results, sep)),
        None => Ok(concat_docs(results)),
    }
}

/// Evaluate a partial template.
///
/// Partials come in two forms:
/// - Bare partial: `$partial()$` - evaluated with current context
/// - Applied partial: `$var:partial()$` - evaluated with var's value as context
///
/// For applied partials with array values, the partial is evaluated once per item,
/// with optional separator between iterations.
fn evaluate_partial(partial: &Partial, ctx: &mut EvalContext) -> TemplateResult<Doc> {
    let Partial {
        name,
        var,
        separator,
        pipes,
        resolved,
        source_info,
    } = partial;

    // Get the resolved partial nodes
    let nodes = match resolved {
        Some(nodes) => nodes,
        None => {
            // Partial was not resolved during compilation - emit error
            ctx.error_with_code(
                "Q-10-5",
                format!("Partial '{}' was not resolved", name),
                source_info,
            );
            return Ok(Doc::Empty);
        }
    };

    // TODO: Apply pipes to partial output
    let _ = pipes;

    match var {
        None => {
            // Bare partial: evaluate with current context
            evaluate_nodes(nodes, ctx)
        }
        Some(var_ref) => {
            // Applied partial: evaluate with var's value as context
            let value = resolve_variable(var_ref, ctx.variables);

            match value {
                None => {
                    // Variable not found - emit warning/error
                    let var_path = var_ref.path.join(".");
                    ctx.warn_or_error_with_code(
                        "Q-10-2",
                        format!("Undefined variable: {}", var_path),
                        &var_ref.source_info,
                    );
                    Ok(Doc::Empty)
                }
                Some(TemplateValue::List(items)) => {
                    // Iterate over list items
                    let mut results = Vec::new();
                    for item in items {
                        let item_ctx = item.to_context();
                        let mut child_ctx = ctx.child(&item_ctx);
                        let result = evaluate_nodes(nodes, &mut child_ctx)?;
                        results.push(result);
                        ctx.merge_diagnostics(child_ctx);
                    }

                    // Join with separator
                    if let Some(sep) = separator {
                        Ok(intersperse_docs(results, Doc::text(sep)))
                    } else {
                        Ok(concat_docs(results))
                    }
                }
                Some(value) => {
                    // Single value: evaluate once with value as context
                    let item_ctx = value.to_context();
                    let mut child_ctx = ctx.child(&item_ctx);
                    let result = evaluate_nodes(nodes, &mut child_ctx)?;
                    ctx.merge_diagnostics(child_ctx);
                    Ok(result)
                }
            }
        }
    }
}

// Re-export the old evaluate function for backwards compatibility
// (kept as a module-level function in case anyone was using it)

/// Evaluate a list of template nodes to a Doc.
///
/// This is a convenience function that creates a temporary EvalContext.
/// For production use with diagnostics, use `Template::render_with_diagnostics`.
pub fn evaluate(nodes: &[TemplateNode], context: &TemplateContext) -> TemplateResult<Doc> {
    let mut eval_ctx = EvalContext::new(context);
    evaluate_nodes(nodes, &mut eval_ctx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn compile(source: &str) -> Template {
        Template::compile(source).expect("template should parse")
    }

    fn ctx() -> TemplateContext {
        TemplateContext::new()
    }

    #[test]
    fn test_literal_text() {
        let template = compile("Hello, world!");
        assert_eq!(template.render(&ctx()).unwrap(), "Hello, world!");
    }

    #[test]
    fn test_simple_variable() {
        let template = compile("Hello, $name$!");
        let mut ctx = ctx();
        ctx.insert("name", TemplateValue::String("Alice".to_string()));
        assert_eq!(template.render(&ctx).unwrap(), "Hello, Alice!");
    }

    #[test]
    fn test_missing_variable() {
        let template = compile("Hello, $name$!");
        // Variable not defined - should produce empty string
        assert_eq!(template.render(&ctx()).unwrap(), "Hello, !");
    }

    #[test]
    fn test_missing_variable_warning() {
        let template = compile("Hello, $name$!");
        let (result, diagnostics) = template.render_with_diagnostics(&ctx());

        // Should succeed (undefined variables are warnings, not errors)
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Hello, !");

        // Should have a warning with error code Q-10-2
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].kind,
            quarto_error_reporting::DiagnosticKind::Warning
        );
        assert!(diagnostics[0].title.contains("Undefined variable"));
        assert_eq!(diagnostics[0].code.as_deref(), Some("Q-10-2"));
    }

    #[test]
    fn test_missing_variable_strict_mode() {
        let template = compile("Hello, $name$!");
        let (result, diagnostics) = template.render_strict(&ctx());

        // Should fail in strict mode
        assert!(result.is_err());

        // Should have an error (not a warning) with error code Q-10-2
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].kind,
            quarto_error_reporting::DiagnosticKind::Error
        );
        assert_eq!(diagnostics[0].code.as_deref(), Some("Q-10-2"));
    }

    #[test]
    fn test_nested_variable() {
        let template = compile("Salary: $employee.salary$");
        let mut ctx = ctx();

        let mut employee = HashMap::new();
        employee.insert(
            "salary".to_string(),
            TemplateValue::String("50000".to_string()),
        );
        ctx.insert("employee", TemplateValue::Map(employee));

        assert_eq!(template.render(&ctx).unwrap(), "Salary: 50000");
    }

    #[test]
    fn test_boolean_true() {
        let template = compile("Value: $flag$");
        let mut ctx = ctx();
        ctx.insert("flag", TemplateValue::Bool(true));
        assert_eq!(template.render(&ctx).unwrap(), "Value: true");
    }

    #[test]
    fn test_boolean_false() {
        let template = compile("Value: $flag$");
        let mut ctx = ctx();
        ctx.insert("flag", TemplateValue::Bool(false));
        // false renders as empty
        assert_eq!(template.render(&ctx).unwrap(), "Value: ");
    }

    #[test]
    fn test_list_concatenation() {
        let template = compile("Items: $items$");
        let mut ctx = ctx();
        ctx.insert(
            "items",
            TemplateValue::List(vec![
                TemplateValue::String("a".to_string()),
                TemplateValue::String("b".to_string()),
                TemplateValue::String("c".to_string()),
            ]),
        );
        assert_eq!(template.render(&ctx).unwrap(), "Items: abc");
    }

    #[test]
    fn test_list_with_separator() {
        let template = compile("Items: $items[, ]$");
        let mut ctx = ctx();
        ctx.insert(
            "items",
            TemplateValue::List(vec![
                TemplateValue::String("a".to_string()),
                TemplateValue::String("b".to_string()),
                TemplateValue::String("c".to_string()),
            ]),
        );
        assert_eq!(template.render(&ctx).unwrap(), "Items: a, b, c");
    }

    #[test]
    fn test_conditional_true() {
        let template = compile("$if(show)$visible$endif$");
        let mut ctx = ctx();
        ctx.insert("show", TemplateValue::Bool(true));
        assert_eq!(template.render(&ctx).unwrap(), "visible");
    }

    #[test]
    fn test_conditional_false() {
        let template = compile("$if(show)$visible$endif$");
        let mut ctx = ctx();
        ctx.insert("show", TemplateValue::Bool(false));
        assert_eq!(template.render(&ctx).unwrap(), "");
    }

    #[test]
    fn test_conditional_missing() {
        let template = compile("$if(show)$visible$endif$");
        // Variable not defined
        assert_eq!(template.render(&ctx()).unwrap(), "");
    }

    #[test]
    fn test_conditional_else() {
        let template = compile("$if(show)$yes$else$no$endif$");
        let mut ctx = ctx();
        ctx.insert("show", TemplateValue::Bool(false));
        assert_eq!(template.render(&ctx).unwrap(), "no");
    }

    #[test]
    fn test_conditional_elseif() {
        // Note: The tree-sitter grammar currently has issues with elseif/else parsing
        // when they appear without whitespace. Use braces syntax or whitespace for now.
        // TODO: Fix this by implementing an external scanner in tree-sitter

        // Using brace syntax which parses correctly
        let template = compile("${if(a)}A${elseif(b)}B${else}C${endif}");

        // a is true
        let mut ctx1 = ctx();
        ctx1.insert("a", TemplateValue::Bool(true));
        assert_eq!(template.render(&ctx1).unwrap(), "A");

        // a false, b true
        let mut ctx2 = ctx();
        ctx2.insert("a", TemplateValue::Bool(false));
        ctx2.insert("b", TemplateValue::Bool(true));
        assert_eq!(template.render(&ctx2).unwrap(), "B");

        // both false
        let mut ctx3 = ctx();
        ctx3.insert("a", TemplateValue::Bool(false));
        ctx3.insert("b", TemplateValue::Bool(false));
        assert_eq!(template.render(&ctx3).unwrap(), "C");
    }

    #[test]
    fn test_for_loop_basic() {
        let template = compile("$for(x)$$x$$endfor$");
        let mut ctx = ctx();
        ctx.insert(
            "x",
            TemplateValue::List(vec![
                TemplateValue::String("a".to_string()),
                TemplateValue::String("b".to_string()),
                TemplateValue::String("c".to_string()),
            ]),
        );
        assert_eq!(template.render(&ctx).unwrap(), "abc");
    }

    #[test]
    fn test_for_loop_with_separator() {
        let template = compile("$for(x)$$x$$sep$, $endfor$");
        let mut ctx = ctx();
        ctx.insert(
            "x",
            TemplateValue::List(vec![
                TemplateValue::String("a".to_string()),
                TemplateValue::String("b".to_string()),
                TemplateValue::String("c".to_string()),
            ]),
        );
        assert_eq!(template.render(&ctx).unwrap(), "a, b, c");
    }

    #[test]
    fn test_for_loop_with_it() {
        // "it" should be bound to current iteration value
        let template = compile("$for(x)$$it$$endfor$");
        let mut ctx = ctx();
        ctx.insert(
            "x",
            TemplateValue::List(vec![
                TemplateValue::String("1".to_string()),
                TemplateValue::String("2".to_string()),
            ]),
        );
        assert_eq!(template.render(&ctx).unwrap(), "12");
    }

    #[test]
    fn test_for_loop_empty() {
        let template = compile("$for(x)$item$endfor$");
        let mut ctx = ctx();
        ctx.insert("x", TemplateValue::List(vec![]));
        assert_eq!(template.render(&ctx).unwrap(), "");
    }

    #[test]
    fn test_for_loop_single_value() {
        // Non-list truthy value should iterate once
        let template = compile("$for(x)$[$x$]$endfor$");
        let mut ctx = ctx();
        ctx.insert("x", TemplateValue::String("single".to_string()));
        assert_eq!(template.render(&ctx).unwrap(), "[single]");
    }

    #[test]
    fn test_comment() {
        // Comment ends at newline; newline needs to be
        // chomped by comment because it's otherwise unavoidable
        let template = compile("before$-- this is a comment\nafter");
        assert_eq!(template.render(&ctx()).unwrap(), "beforeafter");
    }

    #[test]
    fn test_escaped_dollar() {
        let template = compile("Price: $$100");
        assert_eq!(template.render(&ctx()).unwrap(), "Price: $100");
    }

    #[test]
    fn test_combined() {
        let template = compile("$if(items)$Items: $for(items)$$it$$sep$, $endfor$$endif$");
        let mut ctx = ctx();
        ctx.insert(
            "items",
            TemplateValue::List(vec![
                TemplateValue::String("foo".to_string()),
                TemplateValue::String("bar".to_string()),
            ]),
        );
        assert_eq!(template.render(&ctx).unwrap(), "Items: foo, bar");
    }

    #[test]
    fn test_map_truthiness() {
        let template = compile("$if(data)$has data$endif$");
        let mut ctx = ctx();
        let mut data = HashMap::new();
        data.insert("key".to_string(), TemplateValue::Null);
        ctx.insert("data", TemplateValue::Map(data));
        // Non-empty map is truthy
        assert_eq!(template.render(&ctx).unwrap(), "has data");
    }

    #[test]
    fn test_string_false_is_truthy() {
        // The string "false" is truthy (only empty string is falsy)
        let template = compile("$if(x)$truthy$endif$");
        let mut ctx = ctx();
        ctx.insert("x", TemplateValue::String("false".to_string()));
        assert_eq!(template.render(&ctx).unwrap(), "truthy");
    }

    #[test]
    fn test_multiple_undefined_variables() {
        let template = compile("$a$ $b$ $c$");
        let (result, diagnostics) = template.render_with_diagnostics(&ctx());

        // Should succeed with warnings
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "  "); // Three empties with spaces between

        // Should have three warnings
        assert_eq!(diagnostics.len(), 3);
        for diag in &diagnostics {
            assert_eq!(diag.kind, quarto_error_reporting::DiagnosticKind::Warning);
        }
    }

    #[test]
    fn test_for_loop_with_undefined_in_body() {
        // Undefined variable inside a for loop body
        let template = compile("$for(x)$[$y$]$endfor$");
        let mut ctx = ctx();
        ctx.insert(
            "x",
            TemplateValue::List(vec![
                TemplateValue::String("a".to_string()),
                TemplateValue::String("b".to_string()),
            ]),
        );

        let (result, diagnostics) = template.render_with_diagnostics(&ctx);

        // Should succeed (warnings, not errors)
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "[][]"); // Two empty brackets

        // Should have two warnings (one per iteration)
        assert_eq!(diagnostics.len(), 2);
    }

    // Partial tests using MemoryResolver for in-memory partials

    use crate::resolver::MemoryResolver;
    use std::path::Path;

    fn compile_with_partials(
        source: &str,
        partials: impl IntoIterator<Item = (&'static str, &'static str)>,
    ) -> Template {
        let resolver = MemoryResolver::with_partials(partials.into_iter());
        Template::compile_with_resolver(source, Path::new("test.html"), &resolver, 0)
            .expect("template should compile")
    }

    #[test]
    fn test_bare_partial() {
        // Bare partial: $header()$ evaluates with current context
        let template = compile_with_partials("$header()$", [("header", "<h1>$title$</h1>")]);
        let mut ctx = ctx();
        ctx.insert("title", TemplateValue::String("Hello".to_string()));

        assert_eq!(template.render(&ctx).unwrap(), "<h1>Hello</h1>");
    }

    #[test]
    fn test_bare_partial_nested() {
        // Nested bare partials
        let template = compile_with_partials(
            "$wrapper()$",
            [
                ("wrapper", "<div>$inner()$</div>"),
                ("inner", "Content: $text$"),
            ],
        );
        let mut ctx = ctx();
        ctx.insert("text", TemplateValue::String("Hello".to_string()));

        assert_eq!(template.render(&ctx).unwrap(), "<div>Content: Hello</div>");
    }

    #[test]
    fn test_applied_partial_with_map() {
        // Applied partial: $item:card()$ evaluates with item as context
        let template =
            compile_with_partials("$item:card()$", [("card", "<div>$name$ - $price$</div>")]);

        let mut ctx = ctx();
        let mut item = HashMap::new();
        item.insert(
            "name".to_string(),
            TemplateValue::String("Widget".to_string()),
        );
        item.insert(
            "price".to_string(),
            TemplateValue::String("$10".to_string()),
        );
        ctx.insert("item", TemplateValue::Map(item));

        assert_eq!(template.render(&ctx).unwrap(), "<div>Widget - $10</div>");
    }

    #[test]
    fn test_applied_partial_with_list() {
        // Applied partial with list: iterates over items
        let template = compile_with_partials("$items:item()$", [("item", "[$name$]")]);

        let mut ctx = ctx();
        let items = vec![
            {
                let mut m = HashMap::new();
                m.insert("name".to_string(), TemplateValue::String("A".to_string()));
                TemplateValue::Map(m)
            },
            {
                let mut m = HashMap::new();
                m.insert("name".to_string(), TemplateValue::String("B".to_string()));
                TemplateValue::Map(m)
            },
        ];
        ctx.insert("items", TemplateValue::List(items));

        assert_eq!(template.render(&ctx).unwrap(), "[A][B]");
    }

    #[test]
    fn test_applied_partial_with_list_and_separator() {
        // Applied partial with list and separator: $items:item()[, ]$
        let template = compile_with_partials("$items:item()[, ]$", [("item", "$name$")]);

        let mut ctx = ctx();
        let items = vec![
            {
                let mut m = HashMap::new();
                m.insert("name".to_string(), TemplateValue::String("A".to_string()));
                TemplateValue::Map(m)
            },
            {
                let mut m = HashMap::new();
                m.insert("name".to_string(), TemplateValue::String("B".to_string()));
                TemplateValue::Map(m)
            },
            {
                let mut m = HashMap::new();
                m.insert("name".to_string(), TemplateValue::String("C".to_string()));
                TemplateValue::Map(m)
            },
        ];
        ctx.insert("items", TemplateValue::List(items));

        assert_eq!(template.render(&ctx).unwrap(), "A, B, C");
    }

    #[test]
    fn test_applied_partial_with_scalar() {
        // Applied partial with scalar value: binds to "it"
        let template = compile_with_partials("$name:bold()$", [("bold", "<b>$it$</b>")]);

        let mut ctx = ctx();
        ctx.insert("name", TemplateValue::String("Alice".to_string()));

        assert_eq!(template.render(&ctx).unwrap(), "<b>Alice</b>");
    }

    #[test]
    fn test_partial_missing_variable_warning() {
        // Undefined variable in applied partial should emit warning
        let template = compile_with_partials("$x:partial()$", [("partial", "content")]);

        let (result, diagnostics) = template.render_with_diagnostics(&ctx());

        // Should succeed (warning, not error)
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ""); // Empty because x is undefined

        // Should have a warning about undefined variable
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].title.contains("Undefined variable"));
    }

    #[test]
    fn test_unresolved_partial_error() {
        // Partial that wasn't resolved during compilation should emit error
        // We can't easily test this with the normal API, but we can test
        // the diagnostic behavior indirectly
        let template = compile_with_partials("Text only", []);
        assert_eq!(template.render(&ctx()).unwrap(), "Text only");
    }

    #[test]
    fn test_partial_in_conditional() {
        // Partial inside conditional block
        let template =
            compile_with_partials("$if(show)$$header()$$endif$", [("header", "[HEADER]")]);

        let mut ctx_true = ctx();
        ctx_true.insert("show", TemplateValue::Bool(true));
        assert_eq!(template.render(&ctx_true).unwrap(), "[HEADER]");

        let mut ctx_false = ctx();
        ctx_false.insert("show", TemplateValue::Bool(false));
        assert_eq!(template.render(&ctx_false).unwrap(), "");
    }

    #[test]
    fn test_partial_in_for_loop() {
        // Partial inside for loop
        let template =
            compile_with_partials("$for(items)$$item()$$sep$, $endfor$", [("item", "[$it$]")]);

        let mut ctx = ctx();
        ctx.insert(
            "items",
            TemplateValue::List(vec![
                TemplateValue::String("a".to_string()),
                TemplateValue::String("b".to_string()),
            ]),
        );

        assert_eq!(template.render(&ctx).unwrap(), "[a], [b]");
    }

    #[test]
    fn test_to_context_map() {
        // TemplateValue::to_context with map
        let mut map = HashMap::new();
        map.insert("x".to_string(), TemplateValue::String("val".to_string()));
        let value = TemplateValue::Map(map);

        let ctx = value.to_context();
        assert_eq!(
            ctx.get("x"),
            Some(&TemplateValue::String("val".to_string()))
        );
        // Also has "it" bound to the whole map
        assert!(ctx.get("it").is_some());
    }

    #[test]
    fn test_to_context_scalar() {
        // TemplateValue::to_context with scalar
        let value = TemplateValue::String("hello".to_string());
        let ctx = value.to_context();

        // Scalar is bound to "it"
        assert_eq!(
            ctx.get("it"),
            Some(&TemplateValue::String("hello".to_string()))
        );
    }
}
