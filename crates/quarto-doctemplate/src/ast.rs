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
    /// Resolved partial template nodes (populated during compilation).
    ///
    /// This is `None` after parsing and before partial resolution.
    /// After `resolve_partials()` is called, this contains the parsed
    /// nodes from the partial template file.
    pub resolved: Option<Vec<TemplateNode>>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_source_map::{FileId, SourceInfo};

    fn test_source_info() -> SourceInfo {
        SourceInfo::original(FileId(0), 0, 10)
    }

    // ========================================================================
    // VariableRef tests
    // ========================================================================

    #[test]
    fn test_variable_ref_new() {
        let source = test_source_info();
        let var = VariableRef::new(vec!["name".to_string()], source.clone());

        assert_eq!(var.path, vec!["name".to_string()]);
        assert!(var.pipes.is_empty());
        assert!(var.separator.is_none());
        assert_eq!(var.source_info, source);
    }

    #[test]
    fn test_variable_ref_new_nested_path() {
        let source = test_source_info();
        let var = VariableRef::new(
            vec!["employee".to_string(), "salary".to_string()],
            source.clone(),
        );

        assert_eq!(var.path, vec!["employee".to_string(), "salary".to_string()]);
    }

    #[test]
    fn test_variable_ref_with_pipes() {
        let source = test_source_info();
        let pipes = vec![Pipe::new("uppercase", source.clone())];
        let var = VariableRef::with_pipes(vec!["name".to_string()], pipes.clone(), source.clone());

        assert_eq!(var.path, vec!["name".to_string()]);
        assert_eq!(var.pipes.len(), 1);
        assert_eq!(var.pipes[0].name, "uppercase");
        assert!(var.separator.is_none());
    }

    #[test]
    fn test_variable_ref_with_separator() {
        let source = test_source_info();
        let var = VariableRef::with_separator(
            vec!["items".to_string()],
            vec![],
            ", ".to_string(),
            source.clone(),
        );

        assert_eq!(var.path, vec!["items".to_string()]);
        assert!(var.pipes.is_empty());
        assert_eq!(var.separator, Some(", ".to_string()));
    }

    #[test]
    fn test_variable_ref_with_pipes_and_separator() {
        let source = test_source_info();
        let pipes = vec![Pipe::new("uppercase", source.clone())];
        let var = VariableRef::with_separator(
            vec!["items".to_string()],
            pipes,
            "; ".to_string(),
            source.clone(),
        );

        assert_eq!(var.path, vec!["items".to_string()]);
        assert_eq!(var.pipes.len(), 1);
        assert_eq!(var.separator, Some("; ".to_string()));
    }

    // ========================================================================
    // Pipe tests
    // ========================================================================

    #[test]
    fn test_pipe_new() {
        let source = test_source_info();
        let pipe = Pipe::new("uppercase", source.clone());

        assert_eq!(pipe.name, "uppercase");
        assert!(pipe.args.is_empty());
        assert_eq!(pipe.source_info, source);
    }

    #[test]
    fn test_pipe_new_with_string_name() {
        let source = test_source_info();
        let pipe = Pipe::new(String::from("lowercase"), source.clone());

        assert_eq!(pipe.name, "lowercase");
    }

    #[test]
    fn test_pipe_with_args_integer() {
        let source = test_source_info();
        let args = vec![PipeArg::Integer(20)];
        let pipe = Pipe::with_args("left", args, source.clone());

        assert_eq!(pipe.name, "left");
        assert_eq!(pipe.args.len(), 1);
        assert_eq!(pipe.args[0], PipeArg::Integer(20));
    }

    #[test]
    fn test_pipe_with_args_string() {
        let source = test_source_info();
        let args = vec![PipeArg::String("| ".to_string())];
        let pipe = Pipe::with_args("wrap", args, source.clone());

        assert_eq!(pipe.name, "wrap");
        assert_eq!(pipe.args.len(), 1);
        assert_eq!(pipe.args[0], PipeArg::String("| ".to_string()));
    }

    #[test]
    fn test_pipe_with_multiple_args() {
        let source = test_source_info();
        let args = vec![
            PipeArg::Integer(20),
            PipeArg::String("| ".to_string()),
            PipeArg::String(" |".to_string()),
        ];
        let pipe = Pipe::with_args("left", args, source.clone());

        assert_eq!(pipe.name, "left");
        assert_eq!(pipe.args.len(), 3);
        assert_eq!(pipe.args[0], PipeArg::Integer(20));
        assert_eq!(pipe.args[1], PipeArg::String("| ".to_string()));
        assert_eq!(pipe.args[2], PipeArg::String(" |".to_string()));
    }

    // ========================================================================
    // PipeArg tests
    // ========================================================================

    #[test]
    fn test_pipe_arg_integer_equality() {
        assert_eq!(PipeArg::Integer(42), PipeArg::Integer(42));
        assert_ne!(PipeArg::Integer(42), PipeArg::Integer(43));
    }

    #[test]
    fn test_pipe_arg_string_equality() {
        assert_eq!(
            PipeArg::String("test".to_string()),
            PipeArg::String("test".to_string())
        );
        assert_ne!(
            PipeArg::String("test".to_string()),
            PipeArg::String("other".to_string())
        );
    }

    #[test]
    fn test_pipe_arg_type_inequality() {
        assert_ne!(PipeArg::Integer(42), PipeArg::String("42".to_string()));
    }
}
