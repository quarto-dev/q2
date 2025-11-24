/*
 * ast.rs
 * Copyright (c) 2025 Posit, PBC
 */

//! Template AST types.
//!
//! This module defines the abstract syntax tree for parsed templates.
//! Each node includes source location information for error reporting.

use quarto_source_map::SourceInfo;

/// A node in the template AST.
#[derive(Debug, Clone, PartialEq)]
pub enum TemplateNode {
    /// Literal text to be output as-is.
    Literal(Literal),

    /// Variable interpolation: `$var$` or `$obj.field$`
    Variable(VariableRef),

    /// Conditional block: `$if(var)$...$else$...$endif$`
    Conditional(Conditional),

    /// For loop: `$for(var)$...$sep$...$endfor$`
    ForLoop(ForLoop),

    /// Partial (sub-template): `$partial()$` or `$var:partial()$`
    Partial(Partial),

    /// Nesting directive: `$^$` marks indentation point
    Nesting(Nesting),

    /// Breakable space block: `$~$...$~$`
    BreakableSpace(BreakableSpace),

    /// Comment (not rendered): `$-- comment`
    Comment(Comment),
}

/// Literal text node.
#[derive(Debug, Clone, PartialEq)]
pub struct Literal {
    /// The literal text content.
    pub text: String,
    /// Source location of this literal.
    pub source_info: SourceInfo,
}

/// Conditional block: `$if(var)$...$else$...$endif$`
#[derive(Debug, Clone, PartialEq)]
pub struct Conditional {
    /// List of (condition, body) pairs for if/elseif branches.
    pub branches: Vec<(VariableRef, Vec<TemplateNode>)>,
    /// Optional else branch.
    pub else_branch: Option<Vec<TemplateNode>>,
    /// Source location of the entire conditional.
    pub source_info: SourceInfo,
}

/// For loop: `$for(var)$...$sep$...$endfor$`
#[derive(Debug, Clone, PartialEq)]
pub struct ForLoop {
    /// Variable to iterate over.
    pub var: VariableRef,
    /// Loop body.
    pub body: Vec<TemplateNode>,
    /// Optional separator between iterations (from `$sep$`).
    pub separator: Option<Vec<TemplateNode>>,
    /// Source location of the entire loop.
    pub source_info: SourceInfo,
}

/// Partial (sub-template): `$partial()$` or `$var:partial()$`
#[derive(Debug, Clone, PartialEq)]
pub struct Partial {
    /// Partial template name.
    pub name: String,
    /// Optional variable to apply partial to.
    pub var: Option<VariableRef>,
    /// Optional literal separator for array iteration (from `[sep]` syntax).
    pub separator: Option<String>,
    /// Pipes to apply to partial output.
    pub pipes: Vec<Pipe>,
    /// Source location of this partial reference.
    pub source_info: SourceInfo,
}

/// Nesting directive: `$^$` marks indentation point.
#[derive(Debug, Clone, PartialEq)]
pub struct Nesting {
    /// Content affected by nesting.
    pub children: Vec<TemplateNode>,
    /// Source location of the nesting directive.
    pub source_info: SourceInfo,
}

/// Breakable space block: `$~$...$~$`
#[derive(Debug, Clone, PartialEq)]
pub struct BreakableSpace {
    /// Content with breakable spaces.
    pub children: Vec<TemplateNode>,
    /// Source location of the breakable space block.
    pub source_info: SourceInfo,
}

/// Comment (not rendered): `$-- comment`
#[derive(Debug, Clone, PartialEq)]
pub struct Comment {
    /// The comment text.
    pub text: String,
    /// Source location of this comment.
    pub source_info: SourceInfo,
}

/// A reference to a variable, possibly with pipes and separator.
#[derive(Debug, Clone, PartialEq)]
pub struct VariableRef {
    /// Path components (e.g., `["employee", "salary"]` for `employee.salary`).
    pub path: Vec<String>,
    /// Pipes to apply to the variable value.
    pub pipes: Vec<Pipe>,
    /// Optional literal separator for array iteration (from `$var[, ]$` syntax).
    /// When present, the variable is iterated as an array with this separator.
    pub separator: Option<String>,
    /// Source location of this variable reference.
    pub source_info: SourceInfo,
}

impl VariableRef {
    /// Create a new variable reference with no pipes or separator.
    pub fn new(path: Vec<String>, source_info: SourceInfo) -> Self {
        Self {
            path,
            pipes: Vec::new(),
            separator: None,
            source_info,
        }
    }

    /// Create a new variable reference with pipes.
    pub fn with_pipes(path: Vec<String>, pipes: Vec<Pipe>, source_info: SourceInfo) -> Self {
        Self {
            path,
            pipes,
            separator: None,
            source_info,
        }
    }

    /// Create a new variable reference with separator.
    pub fn with_separator(
        path: Vec<String>,
        pipes: Vec<Pipe>,
        separator: String,
        source_info: SourceInfo,
    ) -> Self {
        Self {
            path,
            pipes,
            separator: Some(separator),
            source_info,
        }
    }
}

/// A pipe transformation applied to a value.
#[derive(Debug, Clone, PartialEq)]
pub struct Pipe {
    /// Pipe name (e.g., "uppercase", "left").
    pub name: String,
    /// Pipe arguments (for pipes like `left 20 "| "`).
    pub args: Vec<PipeArg>,
    /// Source location of this pipe.
    pub source_info: SourceInfo,
}

impl Pipe {
    /// Create a new pipe with no arguments.
    pub fn new(name: impl Into<String>, source_info: SourceInfo) -> Self {
        Self {
            name: name.into(),
            args: Vec::new(),
            source_info,
        }
    }

    /// Create a new pipe with arguments.
    pub fn with_args(name: impl Into<String>, args: Vec<PipeArg>, source_info: SourceInfo) -> Self {
        Self {
            name: name.into(),
            args,
            source_info,
        }
    }
}

/// An argument to a pipe.
#[derive(Debug, Clone, PartialEq)]
pub enum PipeArg {
    /// Integer argument (e.g., width in `left 20`).
    Integer(i64),
    /// String argument (e.g., border in `left 20 "| "`).
    String(String),
}
