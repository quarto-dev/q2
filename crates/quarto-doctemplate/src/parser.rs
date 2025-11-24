/*
 * parser.rs
 * Copyright (c) 2025 Posit, PBC
 */

//! Template parser using tree-sitter.
//!
//! This module converts tree-sitter parse trees into the template AST.
//! It uses the generic traversal utilities from `quarto-treesitter-ast`.

use crate::ast::{
    BreakableSpace, Comment, Conditional, ForLoop, Literal, Nesting, Partial, Pipe, PipeArg,
    TemplateNode, VariableRef,
};
use crate::error::{TemplateError, TemplateResult};
use quarto_source_map::{FileId, SourceContext, SourceInfo};
use quarto_treesitter_ast::bottomup_traverse_concrete_tree;
use tree_sitter::{Node, Parser};

/// A compiled template ready for evaluation.
#[derive(Debug, Clone)]
pub struct Template {
    /// The parsed template AST.
    pub(crate) nodes: Vec<TemplateNode>,

    /// Original source (for error reporting).
    #[allow(dead_code)]
    pub(crate) source: String,
}

/// Parser context passed through the bottom-up traversal.
#[derive(Debug)]
pub struct ParserContext {
    /// Source context for tracking locations.
    pub source_context: SourceContext,
    /// The current file ID.
    pub file_id: FileId,
}

impl ParserContext {
    /// Create a new parser context for a file.
    pub fn new(filename: &str) -> Self {
        let mut source_context = SourceContext::new();
        let file_id = source_context.add_file(filename.to_string(), None);
        Self {
            source_context,
            file_id,
        }
    }

    /// Create source info from a tree-sitter node.
    fn source_info_from_node(&self, node: &Node) -> SourceInfo {
        let range = quarto_source_map::Range {
            start: quarto_source_map::Location {
                offset: node.start_byte(),
                row: node.start_position().row,
                column: node.start_position().column,
            },
            end: quarto_source_map::Location {
                offset: node.end_byte(),
                row: node.end_position().row,
                column: node.end_position().column,
            },
        };
        SourceInfo::from_range(self.file_id, range)
    }
}

/// Intermediate representation during bottom-up traversal.
/// Each node kind produces one of these, which gets accumulated
/// as we traverse up the tree.
#[derive(Debug)]
enum Intermediate {
    /// Final template nodes (from template_element)
    Nodes(Vec<TemplateNode>),
    /// A single template node
    Node(TemplateNode),
    /// A variable reference (used in conditionals and loops)
    VarRef(VariableRef),
    /// A pipe transformation
    Pipe(Pipe),
    /// Literal text (for intermediate values like partial names, pipe args)
    Text(String),
    /// A partial reference (name only, source info is reconstructed from outer node)
    Partial(String),
    /// Content for conditional branches
    ConditionalThen(Vec<TemplateNode>),
    ConditionalElse(Vec<TemplateNode>),
    ConditionalElseIf(VariableRef, Vec<TemplateNode>),
    /// Content for loops
    LoopContent(Vec<TemplateNode>),
    LoopSeparator(Vec<TemplateNode>),
    LoopVariable(String, SourceInfo),
    /// Literal separator for partials/variables
    LiteralSeparator(String),
    /// Unknown/marker node (ignored in processing)
    Unknown,
}

impl Template {
    /// Compile a template from source text.
    ///
    /// # Arguments
    /// * `source` - The template source text
    ///
    /// # Returns
    /// A compiled template, or an error if parsing fails.
    pub fn compile(source: &str) -> TemplateResult<Self> {
        Self::compile_with_filename(source, "<template>")
    }

    /// Compile a template from source text with a filename for error reporting.
    pub fn compile_with_filename(source: &str, filename: &str) -> TemplateResult<Self> {
        // Set up tree-sitter parser
        let mut parser = Parser::new();
        let language = tree_sitter_doctemplate::LANGUAGE;
        parser
            .set_language(&language.into())
            .map_err(|e| TemplateError::ParseError {
                message: format!("Failed to load template grammar: {}", e),
            })?;

        // Parse the source
        let tree = parser
            .parse(source, None)
            .ok_or_else(|| TemplateError::ParseError {
                message: "Tree-sitter parse failed".to_string(),
            })?;

        // Check for parse errors
        let root = tree.root_node();
        if root.has_error() {
            // Find the first error node for a useful error message
            let error_msg = find_parse_error(&root, source.as_bytes())
                .unwrap_or_else(|| "Unknown parse error".to_string());
            return Err(TemplateError::ParseError { message: error_msg });
        }

        // Set up context for traversal
        let context = ParserContext::new(filename);

        // Use bottom-up traversal to convert CST to AST
        let mut cursor = tree.walk();
        let (_kind, result) = bottomup_traverse_concrete_tree(
            &mut cursor,
            &mut |node, children, input_bytes, ctx| visit_node(node, children, input_bytes, ctx),
            source.as_bytes(),
            &context,
        );

        // Extract the final nodes
        let nodes = match result {
            Intermediate::Nodes(nodes) => nodes,
            Intermediate::Node(node) => vec![node],
            _ => Vec::new(),
        };

        Ok(Template {
            nodes,
            source: source.to_string(),
        })
    }

    /// Get the AST nodes of this template.
    pub fn nodes(&self) -> &[TemplateNode] {
        &self.nodes
    }
}

/// Find the first ERROR node and produce a useful error message.
fn find_parse_error(node: &Node, source: &[u8]) -> Option<String> {
    if node.is_error() || node.is_missing() {
        let start = node.start_position();
        let text = node.utf8_text(source).unwrap_or("<invalid>");
        return Some(format!(
            "Parse error at line {}, column {}: unexpected '{}'",
            start.row + 1,
            start.column + 1,
            text
        ));
    }

    // Recursively check children
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(err) = find_parse_error(&child, source) {
            return Some(err);
        }
    }
    None
}

/// The visitor function for bottom-up traversal.
/// Converts tree-sitter nodes to intermediate representations.
fn visit_node(
    node: &Node,
    children: Vec<(String, Intermediate)>,
    input_bytes: &[u8],
    context: &ParserContext,
) -> Intermediate {
    let source_info = context.source_info_from_node(node);
    let node_text = || node.utf8_text(input_bytes).unwrap_or("").to_string();

    match node.kind() {
        // Root node - collect all template elements
        "template" => {
            let nodes = collect_nodes(children);
            Intermediate::Nodes(nodes)
        }

        // Content wrapper - collect child nodes
        "_content" => {
            let nodes = collect_nodes(children);
            Intermediate::Nodes(nodes)
        }

        // Plain text
        "text" => {
            let text = node_text();
            Intermediate::Node(TemplateNode::Literal(Literal { text, source_info }))
        }

        // Comment: $-- ...
        "comment" => {
            let text = node_text();
            // Strip the $-- prefix
            let text = text.strip_prefix("$--").unwrap_or(&text).to_string();
            Intermediate::Node(TemplateNode::Comment(Comment { text, source_info }))
        }

        // Escaped dollar: $$ -> literal "$"
        "escaped_dollar" => Intermediate::Node(TemplateNode::Literal(Literal {
            text: "$".to_string(),
            source_info,
        })),

        // Nesting directive: $^$
        "nesting" => Intermediate::Node(TemplateNode::Nesting(Nesting {
            children: Vec::new(),
            source_info,
        })),

        // Variable name
        "variable_name" => {
            let name = node_text();
            let var_ref = VariableRef::new(vec![name], source_info);
            Intermediate::VarRef(var_ref)
        }

        // Interpolation: $var$ or ${var}
        "interpolation" | "_interpolation" => {
            let (var_ref, pipes, separator, partial_name) = extract_interpolation_parts(children);

            if let Some(partial) = partial_name {
                // This is a partial application: $var:partial()$
                Intermediate::Node(TemplateNode::Partial(Partial {
                    name: partial,
                    var: var_ref,
                    separator,
                    pipes,
                    source_info,
                }))
            } else if let Some(var) = var_ref {
                // Regular variable interpolation
                let mut var = var;
                var.pipes = pipes;
                var.separator = separator;
                var.source_info = source_info;
                Intermediate::Node(TemplateNode::Variable(var))
            } else {
                Intermediate::Unknown
            }
        }

        // Pipes
        "pipe" => {
            // The pipe node contains the actual pipe type as a child
            for (kind, _) in &children {
                if kind.starts_with("pipe_") {
                    let pipe_name = kind.strip_prefix("pipe_").unwrap_or(kind);
                    return Intermediate::Pipe(Pipe::new(pipe_name, source_info));
                }
            }
            Intermediate::Unknown
        }

        "pipe_left" | "pipe_center" | "pipe_right" => {
            let pipe_name = node.kind().strip_prefix("pipe_").unwrap_or(node.kind());
            let args = extract_pipe_args(&children);
            Intermediate::Pipe(Pipe::with_args(pipe_name, args, source_info))
        }

        // Simple pipe aliases
        kind if kind.starts_with("pipe_") => {
            let pipe_name = kind.strip_prefix("pipe_").unwrap_or(kind);
            Intermediate::Pipe(Pipe::new(pipe_name, source_info))
        }

        // Partial reference
        "partial" => {
            // Find the partial_name child
            for (kind, child) in children {
                if kind == "partial_name" {
                    if let Intermediate::Text(name) = child {
                        return Intermediate::Partial(name);
                    }
                }
            }
            let name = node_text();
            // Strip the () suffix if present
            let name = name.strip_suffix("()").unwrap_or(&name).to_string();
            Intermediate::Partial(name)
        }

        "partial_name" => {
            let name = node_text();
            Intermediate::Text(name)
        }

        // Literal separator [sep]
        "literal_separator" | "partial_array_separator" => {
            let sep = node_text();
            Intermediate::LiteralSeparator(sep)
        }

        // Conditional
        "conditional" => {
            let (condition, then_body, elseifs, else_body) = extract_conditional_parts(children);

            if let Some(cond) = condition {
                let mut branches = vec![(cond, then_body)];
                branches.extend(elseifs);

                Intermediate::Node(TemplateNode::Conditional(Conditional {
                    branches,
                    else_branch: if else_body.is_empty() {
                        None
                    } else {
                        Some(else_body)
                    },
                    source_info,
                }))
            } else {
                Intermediate::Unknown
            }
        }

        "conditional_condition" => {
            // Extract variable name from condition
            for (_kind, child) in children {
                if let Intermediate::VarRef(var) = child {
                    return Intermediate::VarRef(var);
                }
            }
            Intermediate::Unknown
        }

        "conditional_then" => {
            let nodes = collect_nodes(children);
            Intermediate::ConditionalThen(nodes)
        }

        "conditional_else" => {
            let nodes = collect_nodes(children);
            Intermediate::ConditionalElse(nodes)
        }

        "conditional_elseif" | "_conditional_elseif_1" | "_conditional_elseif_2" => {
            let (condition, body) = extract_elseif_parts(children);
            if let Some(cond) = condition {
                Intermediate::ConditionalElseIf(cond, body)
            } else {
                Intermediate::Unknown
            }
        }

        // For loop
        "forloop" => {
            let (var, body, separator) = extract_forloop_parts(children);

            if let Some(var_ref) = var {
                Intermediate::Node(TemplateNode::ForLoop(ForLoop {
                    var: var_ref,
                    body,
                    separator: if separator.is_empty() {
                        None
                    } else {
                        Some(separator)
                    },
                    source_info,
                }))
            } else {
                Intermediate::Unknown
            }
        }

        "forloop_variable" => {
            let name = node_text();
            Intermediate::LoopVariable(name, source_info)
        }

        "forloop_content" => {
            let nodes = collect_nodes(children);
            Intermediate::LoopContent(nodes)
        }

        "forloop_separator" => {
            let nodes = collect_nodes(children);
            Intermediate::LoopSeparator(nodes)
        }

        // Breakable block: $~$...$~$
        "breakable_block" => {
            let nodes = collect_nodes(children);
            Intermediate::Node(TemplateNode::BreakableSpace(BreakableSpace {
                children: nodes,
                source_info,
            }))
        }

        // Template element wrapper - pass through child
        "template_element" => {
            for (_kind, child) in children {
                match child {
                    Intermediate::Node(_) | Intermediate::Nodes(_) => return child,
                    _ => {}
                }
            }
            Intermediate::Unknown
        }

        // Pipe argument nodes
        "n" => {
            let n: i64 = node_text().parse().unwrap_or(0);
            Intermediate::Text(n.to_string())
        }

        "leftborder" | "rightborder" => {
            let text = node_text();
            Intermediate::Text(text)
        }

        // Unknown or marker nodes
        _ => Intermediate::Unknown,
    }
}

/// Collect TemplateNode values from children.
fn collect_nodes(children: Vec<(String, Intermediate)>) -> Vec<TemplateNode> {
    let mut nodes = Vec::new();
    for (_kind, child) in children {
        match child {
            Intermediate::Node(node) => nodes.push(node),
            Intermediate::Nodes(mut inner_nodes) => nodes.append(&mut inner_nodes),
            _ => {}
        }
    }
    nodes
}

/// Extract parts from an interpolation node.
fn extract_interpolation_parts(
    children: Vec<(String, Intermediate)>,
) -> (
    Option<VariableRef>,
    Vec<Pipe>,
    Option<String>,
    Option<String>,
) {
    let mut var_ref = None;
    let mut pipes = Vec::new();
    let mut separator = None;
    let mut partial_name = None;

    for (kind, child) in children {
        match child {
            Intermediate::VarRef(var) => var_ref = Some(var),
            Intermediate::Pipe(pipe) => pipes.push(pipe),
            Intermediate::LiteralSeparator(sep) => separator = Some(sep),
            Intermediate::Partial(name) => partial_name = Some(name),
            // Also check for _interpolation which passes through
            Intermediate::Node(TemplateNode::Variable(var)) if kind == "_interpolation" => {
                var_ref = Some(VariableRef {
                    path: var.path,
                    pipes: var.pipes.clone(),
                    separator: var.separator.clone(),
                    source_info: var.source_info,
                });
                pipes = var.pipes;
                separator = var.separator;
            }
            _ => {}
        }
    }

    (var_ref, pipes, separator, partial_name)
}

/// Extract pipe arguments (n, leftborder, rightborder).
fn extract_pipe_args(children: &[(String, Intermediate)]) -> Vec<PipeArg> {
    let mut args = Vec::new();
    for (kind, child) in children {
        if let Intermediate::Text(text) = child {
            match kind.as_str() {
                "n" => {
                    if let Ok(n) = text.parse::<i64>() {
                        args.push(PipeArg::Integer(n));
                    }
                }
                "leftborder" | "rightborder" => {
                    args.push(PipeArg::String(text.clone()));
                }
                _ => {}
            }
        }
    }
    args
}

/// Extract parts from a conditional node.
fn extract_conditional_parts(
    children: Vec<(String, Intermediate)>,
) -> (
    Option<VariableRef>,
    Vec<TemplateNode>,
    Vec<(VariableRef, Vec<TemplateNode>)>,
    Vec<TemplateNode>,
) {
    let mut condition = None;
    let mut then_body = Vec::new();
    let mut elseifs = Vec::new();
    let mut else_body = Vec::new();

    for (_kind, child) in children {
        match child {
            Intermediate::VarRef(var) if condition.is_none() => condition = Some(var),
            Intermediate::ConditionalThen(nodes) => then_body = nodes,
            Intermediate::ConditionalElse(nodes) => else_body = nodes,
            Intermediate::ConditionalElseIf(var, nodes) => elseifs.push((var, nodes)),
            _ => {}
        }
    }

    (condition, then_body, elseifs, else_body)
}

/// Extract parts from an elseif node.
fn extract_elseif_parts(
    children: Vec<(String, Intermediate)>,
) -> (Option<VariableRef>, Vec<TemplateNode>) {
    let mut condition = None;
    let mut body = Vec::new();

    for (_kind, child) in children {
        match child {
            Intermediate::VarRef(var) => condition = Some(var),
            Intermediate::Nodes(nodes) => body = nodes,
            Intermediate::Node(node) => body.push(node),
            _ => {}
        }
    }

    (condition, body)
}

/// Extract parts from a forloop node.
fn extract_forloop_parts(
    children: Vec<(String, Intermediate)>,
) -> (Option<VariableRef>, Vec<TemplateNode>, Vec<TemplateNode>) {
    let mut var = None;
    let mut body = Vec::new();
    let mut separator = Vec::new();

    for (_kind, child) in children {
        match child {
            Intermediate::LoopVariable(name, source_info) => {
                var = Some(VariableRef::new(vec![name], source_info));
            }
            Intermediate::LoopContent(nodes) => body = nodes,
            Intermediate::LoopSeparator(nodes) => separator = nodes,
            Intermediate::VarRef(v) => var = Some(v),
            _ => {}
        }
    }

    (var, body, separator)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_literal() {
        let template = Template::compile("Hello, World!").unwrap();
        assert_eq!(template.nodes.len(), 1);
        match &template.nodes[0] {
            TemplateNode::Literal(lit) => assert_eq!(lit.text, "Hello, World!"),
            _ => panic!("Expected Literal node"),
        }
    }

    #[test]
    fn test_parse_comment() {
        // Comment ends at newline; newline is captured in subsequent text node
        let template = Template::compile("$-- This is a comment\ntext").unwrap();
        assert_eq!(template.nodes.len(), 2);
        match &template.nodes[0] {
            TemplateNode::Comment(c) => assert!(c.text.contains("This is a comment")),
            _ => panic!("Expected Comment node"),
        }
        // Second node includes the newline
        match &template.nodes[1] {
            TemplateNode::Literal(lit) => assert_eq!(lit.text, "\ntext"),
            _ => panic!("Expected Literal node after comment"),
        }
    }

    #[test]
    fn test_parse_variable() {
        let template = Template::compile("$name$").unwrap();
        assert_eq!(template.nodes.len(), 1);
        match &template.nodes[0] {
            TemplateNode::Variable(var) => {
                assert_eq!(var.path, vec!["name"]);
            }
            _ => panic!("Expected Variable node"),
        }
    }

    #[test]
    fn test_parse_variable_braces() {
        let template = Template::compile("${name}").unwrap();
        assert_eq!(template.nodes.len(), 1);
        match &template.nodes[0] {
            TemplateNode::Variable(var) => {
                assert_eq!(var.path, vec!["name"]);
            }
            _ => panic!("Expected Variable node"),
        }
    }

    #[test]
    fn test_parse_nesting() {
        let template = Template::compile("$^$").unwrap();
        assert_eq!(template.nodes.len(), 1);
        match &template.nodes[0] {
            TemplateNode::Nesting(_) => {}
            _ => panic!("Expected Nesting node"),
        }
    }

    #[test]
    fn test_parse_escaped_dollar() {
        let template = Template::compile("Price: $$100").unwrap();
        assert_eq!(template.nodes.len(), 3);
        // First node is "Price: "
        match &template.nodes[0] {
            TemplateNode::Literal(lit) => assert_eq!(lit.text, "Price: "),
            _ => panic!("Expected Literal node"),
        }
        // Second node is the escaped dollar becoming "$"
        match &template.nodes[1] {
            TemplateNode::Literal(lit) => assert_eq!(lit.text, "$"),
            _ => panic!("Expected Literal node for escaped dollar"),
        }
        // Third node is "100"
        match &template.nodes[2] {
            TemplateNode::Literal(lit) => assert_eq!(lit.text, "100"),
            _ => panic!("Expected Literal node for trailing text"),
        }
    }
}
