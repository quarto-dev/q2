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
use crate::parser::Template;

impl Template {
    /// Render this template with the given context.
    ///
    /// # Arguments
    /// * `context` - The variable context for evaluation
    ///
    /// # Returns
    /// The rendered output string, or an error if evaluation fails.
    pub fn render(&self, context: &TemplateContext) -> TemplateResult<String> {
        let doc = evaluate(&self.nodes, context)?;
        Ok(doc.render(None))
    }

    /// Evaluate this template to a Doc tree.
    ///
    /// This is useful when you need the structured representation
    /// for further processing before final string rendering.
    pub fn evaluate(&self, context: &TemplateContext) -> TemplateResult<Doc> {
        evaluate(&self.nodes, context)
    }
}

/// Evaluate a list of template nodes to a Doc.
pub fn evaluate(nodes: &[TemplateNode], context: &TemplateContext) -> TemplateResult<Doc> {
    let docs: Result<Vec<Doc>, _> = nodes.iter().map(|n| evaluate_node(n, context)).collect();
    Ok(concat_docs(docs?))
}

/// Evaluate a single template node to a Doc.
fn evaluate_node(node: &TemplateNode, context: &TemplateContext) -> TemplateResult<Doc> {
    match node {
        TemplateNode::Literal(Literal { text, .. }) => Ok(Doc::text(text)),

        TemplateNode::Variable(var) => Ok(render_variable(var, context)),

        TemplateNode::Conditional(Conditional {
            branches,
            else_branch,
            ..
        }) => evaluate_conditional(branches, else_branch, context),

        TemplateNode::ForLoop(ForLoop {
            var,
            body,
            separator,
            ..
        }) => evaluate_for_loop(var, body, separator, context),

        TemplateNode::Partial(Partial {
            name,
            var,
            separator,
            pipes,
            ..
        }) => {
            // TODO: Implement partial loading and evaluation
            let _ = (name, var, separator, pipes);
            Ok(Doc::Empty)
        }

        TemplateNode::Nesting(Nesting { children, .. }) => {
            // TODO: Implement nesting/indentation tracking
            // For now, just evaluate children without nesting
            evaluate(children, context)
        }

        TemplateNode::BreakableSpace(BreakableSpace { children, .. }) => {
            // For now, breakable spaces just evaluate their children
            // Full breakable space semantics require line-width-aware rendering
            evaluate(children, context)
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
    context: &'a TemplateContext,
) -> Option<&'a TemplateValue> {
    // Variable paths may contain dots (e.g., "employee.salary" is a single path element)
    // Split on dots to get the actual path components
    let path: Vec<&str> = var.path.iter().flat_map(|s| s.split('.')).collect();
    context.get_path(&path)
}

/// Render a variable reference to a Doc.
fn render_variable(var: &VariableRef, context: &TemplateContext) -> Doc {
    match resolve_variable(var, context) {
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
        None => Doc::Empty,
    }
}

/// Evaluate a conditional block.
fn evaluate_conditional(
    branches: &[(VariableRef, Vec<TemplateNode>)],
    else_branch: &Option<Vec<TemplateNode>>,
    context: &TemplateContext,
) -> TemplateResult<Doc> {
    // Try each if/elseif branch
    for (condition, body) in branches {
        if let Some(value) = resolve_variable(condition, context) {
            if value.is_truthy() {
                return evaluate(body, context);
            }
        }
    }

    // No branch matched, try else
    if let Some(else_body) = else_branch {
        evaluate(else_body, context)
    } else {
        Ok(Doc::Empty)
    }
}

/// Evaluate a for loop.
fn evaluate_for_loop(
    var: &VariableRef,
    body: &[TemplateNode],
    separator: &Option<Vec<TemplateNode>>,
    context: &TemplateContext,
) -> TemplateResult<Doc> {
    let value = resolve_variable(var, context);

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
        Some(evaluate(sep_nodes, context)?)
    } else {
        None
    };

    // Render each iteration
    let mut results = Vec::new();
    for item in &items {
        let mut child_ctx = context.child();

        // Bind to variable name AND "it" (Pandoc semantics)
        child_ctx.insert(var_name, (*item).clone());
        child_ctx.insert("it", (*item).clone());

        results.push(evaluate(body, &child_ctx)?);
    }

    // Join with separator
    match sep_doc {
        Some(sep) => Ok(intersperse_docs(results, sep)),
        None => Ok(concat_docs(results)),
    }
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
}
