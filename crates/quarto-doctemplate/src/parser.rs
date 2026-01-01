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
use crate::resolver::{PartialResolver, remove_final_newline, resolve_partial_path};
use quarto_source_map::{FileId, SourceContext, SourceInfo};
use quarto_treesitter_ast::bottomup_traverse_concrete_tree;
use std::path::Path;
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
///
/// This struct holds only the file ID needed during parsing.
/// The `SourceContext` is managed at the `Template` level, not here.
#[derive(Debug)]
pub struct ParserContext {
    /// The current file ID within the source context.
    pub file_id: FileId,
}

impl ParserContext {
    /// Create a new parser context with a specific file ID.
    ///
    /// The file should already be added to a `SourceContext` before calling this.
    pub fn new(file_id: FileId) -> Self {
        Self { file_id }
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
    /// A bare partial reference: $partial()$ with optional pipes
    BarePartial(String, Vec<Pipe>, SourceInfo),
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
    /// This creates an internal `SourceContext` for standalone use.
    /// For integrated use with a shared context, use [`compile_with_source_context`].
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
    ///
    /// This creates an internal `SourceContext` for standalone use.
    /// For integrated use with a shared context, use [`compile_with_source_context`].
    pub fn compile_with_filename(source: &str, filename: &str) -> TemplateResult<Self> {
        let mut source_context = SourceContext::new();
        Self::compile_with_source_context(source, filename, &mut source_context)
    }

    /// Compile a template from source text with a shared `SourceContext`.
    ///
    /// This allows multiple templates (main template + partials) to share the same
    /// `SourceContext`, ensuring unique file IDs across all files. This is essential
    /// for correct diagnostic reporting when templates are used within a larger
    /// compilation pipeline.
    ///
    /// # Arguments
    /// * `source` - The template source text
    /// * `filename` - The filename for error reporting
    /// * `source_context` - The shared source context (files will be added to this)
    ///
    /// # Returns
    /// A compiled template, or an error if parsing fails.
    pub fn compile_with_source_context(
        source: &str,
        filename: &str,
        source_context: &mut SourceContext,
    ) -> TemplateResult<Self> {
        // Add this file to the shared context
        let file_id = source_context.add_file(filename.to_string(), Some(source.to_string()));

        // Parse using the assigned file ID
        Self::parse_with_file_id(source, file_id)
    }

    /// Internal: Parse a template with a pre-assigned file ID.
    ///
    /// This is the core parsing logic, separated from SourceContext management.
    fn parse_with_file_id(source: &str, file_id: FileId) -> TemplateResult<Self> {
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
        let context = ParserContext::new(file_id);

        // Use bottom-up traversal to convert CST to AST
        let mut cursor = tree.walk();
        let (_kind, result) = bottomup_traverse_concrete_tree(
            &mut cursor,
            &mut |node, children, input_bytes, ctx| visit_node(node, children, input_bytes, ctx),
            source.as_bytes(),
            &context,
        );

        // Extract the final nodes
        let mut nodes = match result {
            Intermediate::Nodes(nodes) => nodes,
            Intermediate::Node(node) => vec![node],
            _ => Vec::new(),
        };

        // Normalize multiline directives to match Pandoc's newline handling
        normalize_multiline_directives(&mut nodes);

        Ok(Template {
            nodes,
            source: source.to_string(),
        })
    }

    /// Get the AST nodes of this template.
    pub fn nodes(&self) -> &[TemplateNode] {
        &self.nodes
    }

    /// Compile a template from a file, resolving partials from the filesystem.
    ///
    /// This is the main entry point for loading templates that may reference partials.
    /// Partials are loaded from the same directory as the template file.
    ///
    /// # Arguments
    /// * `path` - Path to the template file
    ///
    /// # Returns
    /// A compiled template with all partials resolved, or an error.
    pub fn compile_from_file(path: &Path) -> TemplateResult<Self> {
        let source = std::fs::read_to_string(path)?;
        let filename = path.to_string_lossy();
        Self::compile_with_resolver(&source, path, &crate::resolver::FileSystemResolver, 0).map_err(
            |e| {
                // Enhance error message with filename
                match e {
                    TemplateError::ParseError { message } => TemplateError::ParseError {
                        message: format!("{}: {}", filename, message),
                    },
                    _ => e,
                }
            },
        )
    }

    /// Compile a template with a custom partial resolver.
    ///
    /// This creates an internal `SourceContext` for standalone use.
    /// For integrated use with a shared context, use [`compile_with_resolver_and_context`].
    ///
    /// # Arguments
    /// * `source` - The template source text
    /// * `template_path` - Path used for resolving relative partial references
    /// * `resolver` - The partial resolver to use
    /// * `depth` - Current nesting depth (for recursion protection)
    ///
    /// # Returns
    /// A compiled template with all partials resolved, or an error.
    pub fn compile_with_resolver(
        source: &str,
        template_path: &Path,
        resolver: &impl PartialResolver,
        depth: usize,
    ) -> TemplateResult<Self> {
        let mut source_context = SourceContext::new();
        Self::compile_with_resolver_and_context(
            source,
            template_path,
            resolver,
            depth,
            &mut source_context,
        )
    }

    /// Compile a template with a custom partial resolver and shared `SourceContext`.
    ///
    /// This allows multiple templates (main template + partials) to share the same
    /// `SourceContext`, ensuring unique file IDs across all files.
    ///
    /// # Arguments
    /// * `source` - The template source text
    /// * `template_path` - Path used for resolving relative partial references
    /// * `resolver` - The partial resolver to use
    /// * `depth` - Current nesting depth (for recursion protection)
    /// * `source_context` - The shared source context (files will be added to this)
    ///
    /// # Returns
    /// A compiled template with all partials resolved, or an error.
    pub fn compile_with_resolver_and_context(
        source: &str,
        template_path: &Path,
        resolver: &impl PartialResolver,
        depth: usize,
        source_context: &mut SourceContext,
    ) -> TemplateResult<Self> {
        const MAX_DEPTH: usize = 50;

        // First, parse the template with the shared source context
        let filename = template_path.to_string_lossy();
        let mut template = Self::compile_with_source_context(source, &filename, source_context)?;

        // Then resolve partials recursively (also using the shared context)
        resolve_partials_with_context(
            &mut template.nodes,
            template_path,
            resolver,
            depth,
            MAX_DEPTH,
            source_context,
        )?;

        Ok(template)
    }
}

/// Recursively resolve partial references with a shared `SourceContext`.
///
/// This function traverses the AST and for each `Partial` node:
/// 1. Loads the partial source using the resolver
/// 2. Parses the partial source with the shared `SourceContext`
/// 3. Recursively resolves any partials in the loaded partial
/// 4. Stores the resolved nodes in the `Partial.resolved` field
///
/// Using a shared `SourceContext` ensures unique file IDs across all parsed
/// templates, which is essential for correct diagnostic attribution.
fn resolve_partials_with_context(
    nodes: &mut [TemplateNode],
    template_path: &Path,
    resolver: &impl PartialResolver,
    depth: usize,
    max_depth: usize,
    source_context: &mut SourceContext,
) -> TemplateResult<()> {
    // Check recursion limit
    if depth > max_depth {
        // Find the first partial to report in the error
        for node in nodes.iter() {
            if let TemplateNode::Partial(partial) = node {
                return Err(TemplateError::RecursivePartial {
                    name: partial.name.clone(),
                    max_depth,
                });
            }
        }
        // Shouldn't reach here, but fallback
        return Err(TemplateError::RecursivePartial {
            name: "<unknown>".to_string(),
            max_depth,
        });
    }

    for node in nodes.iter_mut() {
        match node {
            TemplateNode::Partial(partial) => {
                // Load the partial source
                let partial_source = resolver
                    .get_partial(&partial.name, template_path)
                    .ok_or_else(|| TemplateError::PartialNotFound {
                        name: partial.name.clone(),
                    })?;

                // Remove final newline (Pandoc behavior)
                let partial_source = remove_final_newline(&partial_source);

                // Determine the path for this partial (for nested partial resolution)
                let partial_path = resolve_partial_path(&partial.name, template_path);

                // Parse the partial using the shared source context
                let partial_template = Template::compile_with_resolver_and_context(
                    partial_source,
                    &partial_path,
                    resolver,
                    depth + 1,
                    source_context,
                )?;

                // Store the resolved nodes
                partial.resolved = Some(partial_template.nodes);
            }

            // Recurse into nested structures
            TemplateNode::Conditional(cond) => {
                for (_, body) in &mut cond.branches {
                    resolve_partials_with_context(
                        body,
                        template_path,
                        resolver,
                        depth,
                        max_depth,
                        source_context,
                    )?;
                }
                if let Some(else_branch) = &mut cond.else_branch {
                    resolve_partials_with_context(
                        else_branch,
                        template_path,
                        resolver,
                        depth,
                        max_depth,
                        source_context,
                    )?;
                }
            }

            TemplateNode::ForLoop(for_loop) => {
                resolve_partials_with_context(
                    &mut for_loop.body,
                    template_path,
                    resolver,
                    depth,
                    max_depth,
                    source_context,
                )?;
                if let Some(sep) = &mut for_loop.separator {
                    resolve_partials_with_context(
                        sep,
                        template_path,
                        resolver,
                        depth,
                        max_depth,
                        source_context,
                    )?;
                }
            }

            TemplateNode::Nesting(nesting) => {
                resolve_partials_with_context(
                    &mut nesting.children,
                    template_path,
                    resolver,
                    depth,
                    max_depth,
                    source_context,
                )?;
            }

            TemplateNode::BreakableSpace(bs) => {
                resolve_partials_with_context(
                    &mut bs.children,
                    template_path,
                    resolver,
                    depth,
                    max_depth,
                    source_context,
                )?;
            }

            // Other nodes don't need recursion
            TemplateNode::Literal(_) | TemplateNode::Variable(_) | TemplateNode::Comment(_) => {}
        }
    }

    Ok(())
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
            match extract_interpolation_parts(children) {
                InterpolationResult::BarePartial {
                    partial_name,
                    pipes,
                    source_info: bare_source_info,
                } => {
                    // Bare partial: $partial()$
                    Intermediate::Node(TemplateNode::Partial(Partial {
                        name: partial_name,
                        var: None,
                        separator: None,
                        pipes,
                        source_info: bare_source_info,
                        resolved: None,
                    }))
                }
                InterpolationResult::AppliedPartial {
                    var_ref,
                    partial_name,
                    pipes,
                    separator,
                } => {
                    // Applied partial: $var:partial()$
                    Intermediate::Node(TemplateNode::Partial(Partial {
                        name: partial_name,
                        var: var_ref,
                        separator,
                        pipes,
                        source_info,
                        resolved: None,
                    }))
                }
                InterpolationResult::Variable {
                    var_ref: Some(var),
                    pipes,
                    separator,
                } => {
                    // Regular variable interpolation
                    let mut var = var;
                    var.pipes = pipes;
                    var.separator = separator;
                    var.source_info = source_info;
                    Intermediate::Node(TemplateNode::Variable(var))
                }
                InterpolationResult::Variable { var_ref: None, .. } => Intermediate::Unknown,
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

        // Partial reference (applied partial: $var:partial()$)
        "partial" => {
            // Find the partial_name child
            for (kind, child) in children {
                if kind == "partial_name"
                    && let Intermediate::Text(name) = child
                {
                    return Intermediate::Partial(name);
                }
            }
            let name = node_text();
            // Strip the () suffix if present
            let name = name.strip_suffix("()").unwrap_or(&name).to_string();
            Intermediate::Partial(name)
        }

        // Bare partial reference: $partial()$
        "bare_partial" => {
            // Extract partial_name and pipes from children
            let mut partial_name = String::new();
            let mut pipes = Vec::new();

            for (kind, child) in children {
                match child {
                    Intermediate::Text(name) if kind == "partial_name" => {
                        partial_name = name;
                    }
                    Intermediate::Pipe(pipe) => {
                        pipes.push(pipe);
                    }
                    _ => {}
                }
            }

            // Return a BarePartial intermediate that will be converted to a Partial node
            Intermediate::BarePartial(partial_name, pipes, source_info)
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

/// Result of extracting interpolation parts.
enum InterpolationResult {
    /// Regular variable interpolation
    Variable {
        var_ref: Option<VariableRef>,
        pipes: Vec<Pipe>,
        separator: Option<String>,
    },
    /// Applied partial: $var:partial()$
    AppliedPartial {
        var_ref: Option<VariableRef>,
        partial_name: String,
        pipes: Vec<Pipe>,
        separator: Option<String>,
    },
    /// Bare partial: $partial()$
    BarePartial {
        partial_name: String,
        pipes: Vec<Pipe>,
        source_info: SourceInfo,
    },
}

/// Extract parts from an interpolation node.
fn extract_interpolation_parts(children: Vec<(String, Intermediate)>) -> InterpolationResult {
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
            // Bare partial is already fully parsed
            Intermediate::BarePartial(name, bare_pipes, source_info) => {
                return InterpolationResult::BarePartial {
                    partial_name: name,
                    pipes: bare_pipes,
                    source_info,
                };
            }
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
            // Pass through partial nodes from nested _interpolation
            Intermediate::Node(TemplateNode::Partial(partial)) if kind == "_interpolation" => {
                return InterpolationResult::BarePartial {
                    partial_name: partial.name,
                    pipes: partial.pipes,
                    source_info: partial.source_info,
                };
            }
            _ => {}
        }
    }

    if let Some(name) = partial_name {
        InterpolationResult::AppliedPartial {
            var_ref,
            partial_name: name,
            pipes,
            separator,
        }
    } else {
        InterpolationResult::Variable {
            var_ref,
            pipes,
            separator,
        }
    }
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

/// Normalize multiline directives to match Pandoc's newline handling.
///
/// In Pandoc's doctemplates, when a directive like `$if(...)$` or `$for(...)$`
/// is on its own line (preceded by only whitespace, followed by newline),
/// the trailing newline is "swallowed" - it doesn't appear in the output.
/// Similarly, the newline after the closing `$endif$` or `$endfor$` is swallowed.
///
/// Since our tree-sitter parser preserves all text literally, we need to
/// detect these "multiline" directives and strip the extra newlines.
///
/// Detection: A directive is "multiline" if its body starts with a Literal
/// beginning with '\n'. This indicates the directive was followed by a newline.
fn normalize_multiline_directives(nodes: &mut Vec<TemplateNode>) {
    // Process each node, with access to the next sibling for lookahead
    let mut i = 0;
    while i < nodes.len() {
        match &mut nodes[i] {
            TemplateNode::Conditional(cond) => {
                // Check if this is a multiline conditional
                let is_multiline = is_first_child_newline_literal(&cond.branches);

                if is_multiline {
                    // Strip leading newline from body of each branch
                    for (_condition, body) in &mut cond.branches {
                        strip_leading_newline_from_nodes(body);
                        // Recursively normalize nested directives
                        normalize_multiline_directives(body);
                    }

                    // Strip leading newline from else branch if present
                    if let Some(else_body) = &mut cond.else_branch {
                        strip_leading_newline_from_nodes(else_body);
                        normalize_multiline_directives(else_body);
                    }

                    // Strip leading newline from next sibling if it's a Literal
                    if i + 1 < nodes.len() {
                        strip_leading_newline_from_node(&mut nodes[i + 1]);
                    }
                } else {
                    // Still need to recursively normalize nested directives
                    for (_condition, body) in &mut cond.branches {
                        normalize_multiline_directives(body);
                    }
                    if let Some(else_body) = &mut cond.else_branch {
                        normalize_multiline_directives(else_body);
                    }
                }
            }

            TemplateNode::ForLoop(for_loop) => {
                // Check if this is a multiline for loop
                let is_multiline = first_node_is_newline_literal(&for_loop.body);

                if is_multiline {
                    // Strip leading newline from body
                    strip_leading_newline_from_nodes(&mut for_loop.body);
                    normalize_multiline_directives(&mut for_loop.body);

                    // Strip leading newline from separator if present
                    if let Some(sep) = &mut for_loop.separator {
                        strip_leading_newline_from_nodes(sep);
                        normalize_multiline_directives(sep);
                    }

                    // Strip leading newline from next sibling if it's a Literal
                    if i + 1 < nodes.len() {
                        strip_leading_newline_from_node(&mut nodes[i + 1]);
                    }
                } else {
                    // Still need to recursively normalize nested directives
                    normalize_multiline_directives(&mut for_loop.body);
                    if let Some(sep) = &mut for_loop.separator {
                        normalize_multiline_directives(sep);
                    }
                }
            }

            TemplateNode::Nesting(nesting) => {
                normalize_multiline_directives(&mut nesting.children);
            }

            TemplateNode::BreakableSpace(bs) => {
                normalize_multiline_directives(&mut bs.children);
            }

            // Other node types don't need processing
            TemplateNode::Literal(_)
            | TemplateNode::Variable(_)
            | TemplateNode::Partial(_)
            | TemplateNode::Comment(_) => {}
        }

        i += 1;
    }
}

/// Check if the first branch's first node is a Literal starting with '\n'.
fn is_first_child_newline_literal(branches: &[(VariableRef, Vec<TemplateNode>)]) -> bool {
    if let Some((_, body)) = branches.first() {
        first_node_is_newline_literal(body)
    } else {
        false
    }
}

/// Check if the first node in a list is a Literal starting with '\n'.
fn first_node_is_newline_literal(nodes: &[TemplateNode]) -> bool {
    if let Some(TemplateNode::Literal(lit)) = nodes.first() {
        lit.text.starts_with('\n')
    } else {
        false
    }
}

/// Strip a leading '\n' from the first Literal node if present.
fn strip_leading_newline_from_nodes(nodes: &mut Vec<TemplateNode>) {
    if let Some(first) = nodes.first_mut() {
        strip_leading_newline_from_node(first);
        // If the node became empty, remove it
        if let TemplateNode::Literal(lit) = first
            && lit.text.is_empty()
        {
            nodes.remove(0);
        }
    }
}

/// Strip a leading '\n' from a node if it's a Literal starting with '\n'.
fn strip_leading_newline_from_node(node: &mut TemplateNode) {
    if let TemplateNode::Literal(lit) = node
        && lit.text.starts_with('\n')
    {
        lit.text = lit.text[1..].to_string();
    }
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
        // Comment includes the trailing newline (chomped)
        let template = Template::compile("$-- This is a comment\ntext").unwrap();
        assert_eq!(template.nodes.len(), 2);
        match &template.nodes[0] {
            TemplateNode::Comment(c) => assert!(c.text.contains("This is a comment")),
            _ => panic!("Expected Comment node"),
        }
        // Second node is just the text after the newline
        match &template.nodes[1] {
            TemplateNode::Literal(lit) => assert_eq!(lit.text, "text"),
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
