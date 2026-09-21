/*
 * normalize.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! CST → [`MathAst`](crate::ast) normalization.
//!
//! Walks mitex's tree with the reader's span side table and the spec's
//! semantics and produces the writer-facing tree. Every rule here has a
//! fixture snapshot and a structural test in
//! `tests/integration/normalize.rs`; the rules themselves are listed in the
//! plan's Phase 1 design section. Nothing in this pass aborts: unknown or
//! malformed input becomes an [`NodeKind::Error`] plus a [`Problem`], and the
//! seam decides whether to emit the partial tree or fall back to verbatim TeX.

use std::collections::HashMap;
use std::ops::Range;

use mitex_parser::syntax::{SyntaxKind, SyntaxNode, SyntaxToken};
use rowan::NodeOrToken;

use crate::ast::{Node, NodeKind};
use crate::reader::{Parsed, parse};
use crate::spec::{EnvLayout, LimLoc, ScriptsOp, Semantics, Spec};

/// Inline or display math. Normalization is mode-independent today (limit
/// placement is resolved by the writers), but the seam always knows the
/// mode and the writers need it, so it travels with the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Inline,
    Display,
}

/// Categories of things the normalizer could not fully accept. The seam
/// maps each to a `Q-22-*` code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProblemKind {
    UnknownCommand,
    UnknownEnvironment,
    EnvironmentMismatch,
    ArityMismatch,
    UnbalancedBrace,
    UnbalancedDelimiter,
    DoubleScript,
    ExpansionLimit,
    Unsupported,
    RaggedRows,
    IgnoredLimits,
    Dropped,
    Syntax,
}

/// One thing worth telling the author, with its span in the math text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub kind: ProblemKind,
    pub message: String,
    pub span: Option<Range<usize>>,
}

/// Result of [`normalize`].
#[derive(Debug, Clone)]
pub struct Normalized {
    pub mode: Mode,
    /// Always a `Row` spanning the whole text.
    pub root: Node,
    pub problems: Vec<Problem>,
}

/// Parse and normalize the body of a math node.
pub fn normalize(text: &str, mode: Mode, spec: &Spec) -> Normalized {
    let parsed = parse(text, spec);
    let mut n = Normalizer::new(text, &parsed, spec);
    let mut items = Vec::new();
    n.items_into(parsed.root.children_with_tokens(), &mut items);
    let root = Node::new(NodeKind::Row(items), Some(0..text.len()));
    Normalized {
        mode,
        root,
        problems: n.problems,
    }
}

struct Normalizer<'a> {
    text: &'a str,
    spec: &'a Spec,
    /// Leaf index by the token's start offset in the (macro-expanded) tree
    /// text; see [`Parsed::spans`].
    leaf_index: HashMap<u32, usize>,
    spans: &'a [Option<Range<usize>>],
    problems: Vec<Problem>,
}

/// Union of two optional spans.
fn join(a: Option<Range<usize>>, b: Option<Range<usize>>) -> Option<Range<usize>> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.start.min(b.start)..a.end.max(b.end)),
        (Some(a), None) | (None, Some(a)) => Some(a),
        (None, None) => None,
    }
}

fn span_of_nodes(nodes: &[Node]) -> Option<Range<usize>> {
    nodes.iter().fold(None, |acc, n| join(acc, n.span.clone()))
}

fn is_whitespace_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::TokenWhiteSpace | SyntaxKind::TokenLineBreak | SyntaxKind::TokenComment
    )
}

impl<'a> Normalizer<'a> {
    fn new(text: &'a str, parsed: &'a Parsed, spec: &'a Spec) -> Self {
        let mut leaf_index = HashMap::new();
        let mut i = 0usize;
        index_tokens(&parsed.root, &mut leaf_index, &mut i);
        Normalizer {
            text,
            spec,
            leaf_index,
            spans: &parsed.spans,
            problems: Vec::new(),
        }
    }

    fn problem(
        &mut self,
        kind: ProblemKind,
        message: impl Into<String>,
        span: Option<Range<usize>>,
    ) {
        self.problems.push(Problem {
            kind,
            message: message.into(),
            span,
        });
    }

    // -- spans ---------------------------------------------------------------

    fn token_span(&self, tok: &SyntaxToken) -> Option<Range<usize>> {
        let start: u32 = tok.text_range().start().into();
        self.leaf_index
            .get(&start)
            .and_then(|i| self.spans.get(*i).cloned().flatten())
    }

    /// Union of the spans of every token under `node`.
    fn node_span(&self, node: &SyntaxNode) -> Option<Range<usize>> {
        let mut acc = None;
        for el in node.descendants_with_tokens() {
            if let NodeOrToken::Token(t) = el {
                acc = join(acc, self.token_span(&t));
            }
        }
        acc
    }

    /// Extend a span over syntax mitex drops around it: `\begin{`/`\end{`
    /// before, `}` after.
    fn extend_over_env_syntax(&self, span: Option<Range<usize>>) -> Option<Range<usize>> {
        let mut r = span?;
        for prefix in ["\\begin{", "\\end{"] {
            if self.text[..r.start].ends_with(prefix) {
                r.start -= prefix.len();
                break;
            }
        }
        if self.text[r.end..].starts_with('}') {
            r.end += 1;
        }
        Some(r)
    }

    fn verbatim(&self, span: &Option<Range<usize>>, fallback: &str) -> String {
        match span {
            Some(r) => self.text[r.clone()].to_string(),
            None => fallback.to_string(),
        }
    }

    fn error(
        &mut self,
        kind: ProblemKind,
        message: &str,
        span: Option<Range<usize>>,
        verbatim: String,
    ) -> Node {
        self.problem(kind, message, span.clone());
        Node::new(
            NodeKind::Error {
                message: message.to_string(),
                verbatim,
            },
            span,
        )
    }

    /// A `{` group that mitex closed at end of input: keep its content and
    /// append an error so the seam falls back to verbatim TeX.
    fn unclosed_group(&mut self, row: Node, span: Option<Range<usize>>) -> Node {
        let verbatim = self.verbatim(&span, "{");
        let err = self.error(
            ProblemKind::UnbalancedBrace,
            "unclosed `{`",
            span.clone(),
            verbatim,
        );
        let mut items = match row.kind {
            NodeKind::Row(items) => items,
            other => vec![Node::new(other, row.span)],
        };
        items.push(err);
        Node::new(NodeKind::Row(items), span)
    }

    // -- sequences -----------------------------------------------------------

    /// Normalize a sequence of siblings into `out`, flattening nested rows
    /// that only exist for grouping.
    fn items_into(
        &mut self,
        children: impl Iterator<Item = NodeOrToken<SyntaxNode, SyntaxToken>>,
        out: &mut Vec<Node>,
    ) {
        for child in children {
            match child {
                NodeOrToken::Token(t) => {
                    if let Some(node) = self.token(&t) {
                        out.push(node);
                    }
                }
                NodeOrToken::Node(n) if n.kind() == SyntaxKind::ItemText => {
                    // A lexical grouping of words and spaces, not a scope.
                    self.items_into(n.children_with_tokens(), out);
                }
                NodeOrToken::Node(n) => {
                    if let Some(node) = self.node(&n) {
                        out.push(node);
                    }
                }
            }
        }
    }

    /// A row from a sequence of siblings, spanning `span` (or its items).
    fn row_from(
        &mut self,
        children: impl Iterator<Item = NodeOrToken<SyntaxNode, SyntaxToken>>,
        span: Option<Range<usize>>,
    ) -> Node {
        let mut items = Vec::new();
        self.items_into(children, &mut items);
        let span = span.or_else(|| span_of_nodes(&items));
        Node::new(NodeKind::Row(items), span)
    }

    /// A single node for an argument-like syntax node: a brace group becomes
    /// a `Row` of its content, anything else its own normalized form.
    fn operand(&mut self, node: &SyntaxNode) -> Node {
        match node.kind() {
            SyntaxKind::ClauseArgument => {
                // An argument clause wraps exactly one item (or a bare token).
                // A brace/bracket group loses its delimiters here.
                let span = self.node_span(node);
                let group = node.children().find(|c| {
                    matches!(
                        c.kind(),
                        SyntaxKind::ItemCurly | SyntaxKind::ItemBracket | SyntaxKind::ItemParen
                    )
                });
                if let Some(group) = group {
                    let inner = self.operand(&group);
                    return Node::new(inner.kind, span);
                }
                let mut items = Vec::new();
                self.items_into(node.children_with_tokens(), &mut items);
                unwrap_single(items, span)
            }
            SyntaxKind::ItemCurly | SyntaxKind::ItemBracket | SyntaxKind::ItemParen => {
                let span = self.node_span(node);
                let inner = node
                    .children_with_tokens()
                    .filter(|c| !is_brace_token(c, node.kind()));
                let row = self.row_from(inner, None);
                if node.kind() == SyntaxKind::ItemCurly && !has_closing_brace(node) {
                    return self.unclosed_group(row, span);
                }
                Node::new(row.kind, span)
            }
            _ => self
                .node(node)
                .unwrap_or_else(|| Node::new(NodeKind::Row(vec![]), self.node_span(node))),
        }
    }

    // -- tokens --------------------------------------------------------------

    fn token(&mut self, t: &SyntaxToken) -> Option<Node> {
        use SyntaxKind::*;
        let span = self.token_span(t);
        let text = t.text();
        let node = match t.kind() {
            TokenWhiteSpace | TokenLineBreak | TokenComment => return None,
            TokenWord | TokenComma | TokenSlash | TokenAsterisk | TokenAtSign | TokenHash
            | TokenSemicolon | TokenDitto | TokenLBracket | TokenRBracket | TokenLParen
            | TokenRParen => Node::new(NodeKind::Run(text.to_string()), span),
            TokenTilde => Node::new(NodeKind::Sym("\u{a0}".to_string()), span),
            TokenApostrophe => Node::new(NodeKind::Sym("′".to_string()), span),
            TokenCaret | TokenUnderscore => {
                // A script operator that did not attach to anything.
                Node::new(NodeKind::Run(text.to_string()), span)
            }
            TokenLBrace | TokenRBrace => {
                let verbatim = text.to_string();
                return Some(self.error(
                    ProblemKind::UnbalancedBrace,
                    "unbalanced brace",
                    span,
                    verbatim,
                ));
            }
            TokenAmpersand => {
                // Only meaningful inside an environment, where `env` consumes
                // it before we get here.
                let verbatim = text.to_string();
                return Some(self.error(
                    ProblemKind::Syntax,
                    "alignment `&` outside an environment",
                    span,
                    verbatim,
                ));
            }
            TokenDollar | TokenBeginMath | TokenEndMath => return None,
            // mitex emits `\\` as a *token* of kind ItemNewLine.
            ItemNewLine => Node::new(NodeKind::Break, span),
            TokenCommandSym => return self.command(&text[1..], span.clone(), &[], span),
            TokenError => {
                let message = text.to_string();
                let kind = match message.as_str() {
                    "invalid number of arguments" => ProblemKind::ArityMismatch,
                    "macro expansion limit exceeded" => ProblemKind::ExpansionLimit,
                    "unmatched environment" | "invalid environment" => {
                        ProblemKind::EnvironmentMismatch
                    }
                    _ => ProblemKind::Syntax,
                };
                let verbatim = self.verbatim(&span, &message);
                return Some(self.error(kind, &message, span, verbatim));
            }
            ClauseCommandName => {
                // Handled by the enclosing ItemCmd; a bare one is a command
                // without arguments that mitex left at this level.
                return self.command(&text[1..], span.clone(), &[], span);
            }
            _ => Node::new(NodeKind::Run(text.to_string()), span),
        };
        Some(node)
    }

    // -- nodes ---------------------------------------------------------------

    fn node(&mut self, n: &SyntaxNode) -> Option<Node> {
        use SyntaxKind::*;
        match n.kind() {
            ItemText | ItemFormula | ScopeRoot => {
                let span = self.node_span(n);
                let mut items = Vec::new();
                self.items_into(n.children_with_tokens(), &mut items);
                Some(unwrap_single(items, span))
            }
            ItemCurly => {
                let span = self.node_span(n);
                let inner = n
                    .children_with_tokens()
                    .filter(|c| !is_brace_token(c, ItemCurly));
                let row = self.row_from(inner, None);
                if !has_closing_brace(n) {
                    return Some(self.unclosed_group(row, span));
                }
                Some(Node::new(row.kind, span))
            }
            ItemBracket | ItemParen => {
                // Bare `[…]` / `(…)`: keep the brackets as runs.
                let span = self.node_span(n);
                let mut items = Vec::new();
                self.items_into(n.children_with_tokens(), &mut items);
                Some(Node::new(NodeKind::Row(items), span))
            }
            ItemCmd => Some(self.cmd(n)),
            ItemAttachComponent => Some(self.attach(n)),
            ItemLR => Some(self.left_right(n)),
            ItemEnv => Some(self.env(n)),
            ItemNewLine => Some(Node::new(NodeKind::Break, self.node_span(n))),
            ItemBegin | ItemEnd => {
                // A stray `\begin`/`\end` outside a well-formed environment.
                let span = self.extend_over_env_syntax(self.node_span(n));
                let verbatim = self.verbatim(&span, "");
                Some(self.error(
                    ProblemKind::EnvironmentMismatch,
                    "stray environment delimiter",
                    span,
                    verbatim,
                ))
            }
            ItemBlockComment | ItemTypstCode => None,
            ClauseArgument | ClauseLR => {
                let span = self.node_span(n);
                let mut items = Vec::new();
                self.items_into(n.children_with_tokens(), &mut items);
                Some(unwrap_single(items, span))
            }
            _ => {
                let span = self.node_span(n);
                let mut items = Vec::new();
                self.items_into(n.children_with_tokens(), &mut items);
                Some(unwrap_single(items, span))
            }
        }
    }

    // -- commands ------------------------------------------------------------

    fn cmd(&mut self, n: &SyntaxNode) -> Node {
        let name_tok = n
            .children_with_tokens()
            .filter_map(|c| c.into_token())
            .find(|t| t.kind() == SyntaxKind::ClauseCommandName);
        let span = self.node_span(n);
        let Some(name_tok) = name_tok else {
            let verbatim = self.verbatim(&span, "");
            return self.error(
                ProblemKind::Syntax,
                "command without a name",
                span,
                verbatim,
            );
        };
        let name = name_tok.text()[1..].to_string();
        let name_span = self.token_span(&name_tok);
        let args: Vec<SyntaxNode> = n
            .children()
            .filter(|c| c.kind() == SyntaxKind::ClauseArgument)
            .collect();
        self.command(&name, name_span, &args, span)
            .unwrap_or_else(|| Node::new(NodeKind::Row(vec![]), None))
    }

    /// Interpret command `name` with its argument clauses.
    fn command(
        &mut self,
        name: &str,
        name_span: Option<Range<usize>>,
        args: &[SyntaxNode],
        span: Option<Range<usize>>,
    ) -> Option<Node> {
        let Some(sem) = self.spec.semantics(name).cloned() else {
            let verbatim = self.verbatim(&name_span, name);
            let err = self.error(
                ProblemKind::UnknownCommand,
                &format!("unknown command `\\{name}`"),
                name_span,
                verbatim,
            );
            // Lenient parse: any argument mitex bound stays as content.
            let mut items = vec![err];
            for a in args {
                items.push(self.operand(a));
            }
            return Some(unwrap_single(items, span));
        };
        let mut operands: Vec<Node> = args.iter().map(|a| self.operand(a)).collect();
        let full_span = join(span.clone(), span_of_nodes(&operands));
        let node = match sem {
            Semantics::Sym { text } => Node::new(NodeKind::Sym(text), name_span),
            Semantics::Nary { text, limits } => {
                Node::new(NodeKind::Nary { text, limits }, name_span)
            }
            Semantics::Func {
                name: fname,
                limits,
            } => Node::new(
                NodeKind::Func {
                    name: fname,
                    limits,
                },
                name_span,
            ),
            Semantics::Frac { style } => {
                let (Some(num), Some(den)) = (take_op(&mut operands, 0), take_op(&mut operands, 1))
                else {
                    return Some(self.arity_error(name, full_span));
                };
                Node::new(
                    NodeKind::Frac {
                        style,
                        num: Box::new(num),
                        den: Box::new(den),
                    },
                    full_span,
                )
            }
            Semantics::Sqrt | Semantics::XArrow { .. } => {
                // Glob `{,b}t`: an optional bracket argument before the body.
                let (optional, body) = split_optional(args, &mut operands);
                let Some(body) = body else {
                    return Some(self.arity_error(name, full_span));
                };
                match sem {
                    Semantics::Sqrt => Node::new(
                        NodeKind::Sqrt {
                            degree: optional.map(Box::new),
                            body: Box::new(body),
                        },
                        full_span,
                    ),
                    Semantics::XArrow { text } => Node::new(
                        NodeKind::XArrow {
                            text,
                            above: Some(Box::new(body)),
                            below: optional.map(Box::new),
                        },
                        full_span,
                    ),
                    _ => unreachable!(),
                }
            }
            Semantics::Accent { text } => {
                let Some(body) = take_op(&mut operands, 0) else {
                    return Some(self.arity_error(name, full_span));
                };
                Node::new(
                    NodeKind::Accent {
                        text,
                        body: Box::new(body),
                    },
                    full_span,
                )
            }
            Semantics::Bar { pos } => {
                let Some(body) = take_op(&mut operands, 0) else {
                    return Some(self.arity_error(name, full_span));
                };
                Node::new(
                    NodeKind::Bar {
                        pos,
                        body: Box::new(body),
                    },
                    full_span,
                )
            }
            Semantics::GroupChr { text, pos } => {
                let Some(body) = take_op(&mut operands, 0) else {
                    return Some(self.arity_error(name, full_span));
                };
                Node::new(
                    NodeKind::GroupChr {
                        text,
                        pos,
                        body: Box::new(body),
                    },
                    full_span,
                )
            }
            Semantics::LimPos { pos } => {
                let (Some(annotation), Some(body)) =
                    (take_op(&mut operands, 0), take_op(&mut operands, 1))
                else {
                    return Some(self.arity_error(name, full_span));
                };
                Node::new(
                    NodeKind::LimPos {
                        pos,
                        annotation: Box::new(annotation),
                        body: Box::new(body),
                    },
                    full_span,
                )
            }
            Semantics::Style { variant } => {
                // Fixed-arity styles take one operand; greedy ones (`\bf`)
                // take everything mitex bound to them.
                let body = match operands.len() {
                    0 => return Some(self.arity_error(name, full_span)),
                    1 => take_op(&mut operands, 0).unwrap(),
                    _ => {
                        let items = std::mem::take(&mut operands);
                        let s = span_of_nodes(&items);
                        Node::new(NodeKind::Row(items), s)
                    }
                };
                Node::new(
                    NodeKind::Style {
                        variant,
                        body: Box::new(body),
                    },
                    full_span,
                )
            }
            Semantics::Text { variant } => {
                let Some(arg) = args.first() else {
                    return Some(self.arity_error(name, full_span));
                };
                let text = argument_text(arg);
                Node::new(NodeKind::Text { variant, text }, full_span)
            }
            Semantics::Space { em } => {
                // `\hspace{1em}`-style arguments override the default width.
                let em = args
                    .first()
                    .and_then(|a| parse_em(&argument_text(a)))
                    .unwrap_or(em);
                Node::new(NodeKind::Space(em), full_span)
            }
            Semantics::Phantom { h, v } => {
                let Some(body) = take_op(&mut operands, 0) else {
                    return Some(self.arity_error(name, full_span));
                };
                Node::new(
                    NodeKind::Phantom {
                        h,
                        v,
                        body: Box::new(body),
                    },
                    full_span,
                )
            }
            Semantics::Cancel => {
                let Some(body) = take_op(&mut operands, 0) else {
                    return Some(self.arity_error(name, full_span));
                };
                Node::new(NodeKind::Cancel(Box::new(body)), full_span)
            }
            Semantics::Not => {
                let Some(arg) = take_op(&mut operands, 0) else {
                    return Some(self.arity_error(name, full_span));
                };
                let negated = match single_text(&arg) {
                    Some(t) => format!("{t}\u{338}"),
                    None => {
                        let verbatim = self.verbatim(&full_span, name);
                        return Some(self.error(
                            ProblemKind::Unsupported,
                            "`\\not` applies to a single symbol",
                            full_span,
                            verbatim,
                        ));
                    }
                };
                Node::new(NodeKind::Sym(negated), full_span)
            }
            Semantics::Color => {
                let Some(color_arg) = args.first() else {
                    return Some(self.arity_error(name, full_span));
                };
                let color = argument_text(color_arg).trim().to_string();
                let body_items: Vec<Node> = operands.drain(1..).collect();
                let body_span = span_of_nodes(&body_items);
                let body = unwrap_single(body_items, body_span);
                Node::new(
                    NodeKind::Color {
                        color,
                        body: Box::new(body),
                    },
                    full_span,
                )
            }
            Semantics::Scripts { which } => {
                // `\limits` / `\nolimits`: the operand is the operator.
                let Some(base) = take_op(&mut operands, 0) else {
                    return Some(self.arity_error(name, full_span));
                };
                let target = match which {
                    ScriptsOp::Limits => LimLoc::UndOvr,
                    ScriptsOp::NoLimits => LimLoc::SubSup,
                    ScriptsOp::Sup | ScriptsOp::Sub => {
                        return Some(base);
                    }
                };
                let mut base = base;
                if !set_limits(&mut base, target) {
                    self.problem(
                        ProblemKind::IgnoredLimits,
                        format!("`\\{name}` applies to a big operator or function; ignored"),
                        name_span,
                    );
                }
                base.span = full_span;
                base
            }
            Semantics::Env { layout, delims, .. } => {
                // A command with environment semantics (`\substack{a \\ b}`):
                // its one argument holds the rows.
                let Some(arg) = take_op(&mut operands, 0) else {
                    return Some(self.arity_error(name, full_span));
                };
                let items = match arg.kind {
                    NodeKind::Row(items) => items,
                    other => vec![Node::new(other, arg.span)],
                };
                let rows = rows_from_breaks(items);
                let delims = match layout {
                    EnvLayout::Matrix => delims,
                    _ => None,
                };
                Node::new(
                    NodeKind::Matrix {
                        layout,
                        delims,
                        rows,
                    },
                    full_span,
                )
            }
            Semantics::Ignore => {
                if matches!(name, "hline" | "cline") {
                    self.problem(
                        ProblemKind::Dropped,
                        format!("`\\{name}` has no equivalent in a matrix; dropped"),
                        name_span,
                    );
                    return None;
                }
                // Greedy ignorables (`\displaystyle`) keep their content.
                if operands.is_empty() {
                    return None;
                }
                let items = std::mem::take(&mut operands);
                let s = span_of_nodes(&items);
                unwrap_single(items, s)
            }
            Semantics::Unsupported { why } => {
                let verbatim = self.verbatim(&full_span, name);
                self.error(
                    ProblemKind::Unsupported,
                    &format!("`\\{name}` is not supported: {why}"),
                    full_span,
                    verbatim,
                )
            }
        };
        Some(node)
    }

    fn arity_error(&mut self, name: &str, span: Option<Range<usize>>) -> Node {
        let verbatim = self.verbatim(&span, name);
        self.error(
            ProblemKind::ArityMismatch,
            &format!("`\\{name}` is missing an argument"),
            span,
            verbatim,
        )
    }

    // -- attachments ---------------------------------------------------------

    /// `ItemAttachComponent`: `[args(base)] (^|_) script`, nested to the
    /// left. Fold into one `Scripts`; a repeated `^` or `_` is an error that
    /// keeps the first.
    fn attach(&mut self, n: &SyntaxNode) -> Node {
        let span = self.node_span(n);
        let mut base: Option<Node> = None;
        let mut op: Option<char> = None;
        let mut script_items: Vec<Node> = Vec::new();
        for child in n.children_with_tokens() {
            match &child {
                NodeOrToken::Node(c) if c.kind() == SyntaxKind::ClauseArgument && op.is_none() => {
                    base = Some(self.operand(c));
                }
                NodeOrToken::Token(t)
                    if matches!(
                        t.kind(),
                        SyntaxKind::TokenCaret | SyntaxKind::TokenUnderscore
                    ) && op.is_none() =>
                {
                    op = Some(if t.kind() == SyntaxKind::TokenCaret {
                        '^'
                    } else {
                        '_'
                    });
                }
                NodeOrToken::Token(t) if is_whitespace_kind(t.kind()) => {}
                NodeOrToken::Node(c)
                    if matches!(c.kind(), SyntaxKind::ItemCurly | SyntaxKind::ClauseArgument) =>
                {
                    let node = self.operand(c);
                    script_items.push(node);
                }
                _ => {
                    self.items_into(std::iter::once(child.clone()), &mut script_items);
                }
            }
        }
        let script_span = span_of_nodes(&script_items);
        let script = unwrap_single(script_items, script_span);
        let base = base.unwrap_or_else(|| Node::new(NodeKind::Row(vec![]), None));
        let Some(op) = op else {
            return base;
        };

        // Fold onto an existing Scripts node from the inner attachment.
        let mut target = base;
        let mut rejected: Option<Node> = None;
        match &mut target.kind {
            NodeKind::Scripts { sub, sup, .. } => {
                let slot = if op == '^' { sup } else { sub };
                if slot.is_none() {
                    *slot = Some(Box::new(script));
                } else {
                    rejected = Some(script);
                }
                target.span = span.clone();
            }
            _ => {
                let limits = operator_limits(&target);
                let (sub, sup) = if op == '^' {
                    (None, Some(Box::new(script)))
                } else {
                    (Some(Box::new(script)), None)
                };
                target = Node::new(
                    NodeKind::Scripts {
                        base: Box::new(target),
                        sub,
                        sup,
                        limits,
                    },
                    span.clone(),
                );
            }
        }
        match rejected {
            None => target,
            Some(script) => {
                let which = if op == '^' {
                    "superscript"
                } else {
                    "subscript"
                };
                let verbatim = self.verbatim(&script.span, "");
                let err = self.error(
                    ProblemKind::DoubleScript,
                    &format!("double {which}"),
                    script.span.clone(),
                    verbatim,
                );
                Node::new(NodeKind::Row(vec![target, err]), span)
            }
        }
    }

    // -- \left … \right ------------------------------------------------------

    fn left_right(&mut self, n: &SyntaxNode) -> Node {
        let span = self.node_span(n);
        let mut left: Option<Option<String>> = None;
        let mut right: Option<Option<String>> = None;
        let mut parts: Vec<Vec<Node>> = vec![Vec::new()];
        for child in n.children_with_tokens() {
            match &child {
                NodeOrToken::Node(c) if c.kind() == SyntaxKind::ClauseLR => {
                    let (is_left, sym) = self.lr_clause(c);
                    if is_left && left.is_none() {
                        left = Some(sym);
                    } else if !is_left {
                        right = Some(sym);
                    }
                }
                NodeOrToken::Node(c) if is_middle(c) => {
                    parts.push(Vec::new());
                }
                _ => {
                    let part = parts.last_mut().unwrap();
                    self.items_into(std::iter::once(child.clone()), part);
                }
            }
        }
        if right.is_none() {
            self.problem(
                ProblemKind::UnbalancedDelimiter,
                "`\\left` without a matching `\\right`",
                span.clone(),
            );
        }
        if left.is_none() {
            self.problem(
                ProblemKind::UnbalancedDelimiter,
                "`\\right` without a matching `\\left`",
                span.clone(),
            );
        }
        let parts: Vec<Node> = parts
            .into_iter()
            .map(|items| {
                let s = span_of_nodes(&items);
                Node::new(NodeKind::Row(items), s)
            })
            .collect();
        let unbalanced = left.is_none() || right.is_none();
        let node = Node::new(
            NodeKind::Delimited {
                left: left.flatten(),
                right: right.flatten(),
                parts,
            },
            span.clone(),
        );
        if unbalanced {
            let verbatim = self.verbatim(&span, "");
            return Node::new(
                NodeKind::Row(vec![
                    node,
                    Node::new(
                        NodeKind::Error {
                            message: "unbalanced \\left/\\right".to_string(),
                            verbatim,
                        },
                        span.clone(),
                    ),
                ]),
                span,
            );
        }
        node
    }

    /// `(is_left, delimiter)` for a `ClauseLR`; `.` is `None`.
    fn lr_clause(&mut self, c: &SyntaxNode) -> (bool, Option<String>) {
        let mut is_left = false;
        let mut sym: Option<String> = None;
        for t in c.children_with_tokens().filter_map(|c| c.into_token()) {
            match t.kind() {
                SyntaxKind::ClauseCommandName => is_left = t.text() == "\\left",
                k if is_whitespace_kind(k) => {}
                SyntaxKind::TokenCommandSym => {
                    sym = Some(self.delimiter_text(&t.text()[1..]));
                }
                _ => sym = Some(t.text().to_string()),
            }
        }
        let sym = sym.filter(|s| s != ".");
        (is_left, sym)
    }

    /// Delimiter text for a `\name` used after `\left`/`\right`.
    fn delimiter_text(&self, name: &str) -> String {
        match self.spec.semantics(name) {
            Some(Semantics::Sym { text }) => text.clone(),
            _ => format!("\\{name}"),
        }
    }

    // -- environments --------------------------------------------------------

    fn env(&mut self, n: &SyntaxNode) -> Node {
        let raw_span = self.node_span(n);
        let span = self.extend_over_env_syntax(raw_span.clone());
        let begin = n.children().find(|c| c.kind() == SyntaxKind::ItemBegin);
        let end = n.children().find(|c| c.kind() == SyntaxKind::ItemEnd);
        let name_tok = begin
            .as_ref()
            .and_then(|b| b.first_token())
            .filter(|t| t.kind() == SyntaxKind::TokenCommandSym);
        let Some(name_tok) = name_tok else {
            let verbatim = self.verbatim(&span, "");
            return self.error(
                ProblemKind::Syntax,
                "environment without a name",
                span,
                verbatim,
            );
        };
        let name = name_tok.text().to_string();
        let name_span = self.token_span(&name_tok);
        if end.is_none() {
            // The body's extent is a guess without `\end`; keep it verbatim.
            let verbatim = self.verbatim(&span, &name);
            return self.error(
                ProblemKind::EnvironmentMismatch,
                &format!("`\\begin{{{name}}}` without `\\end{{{name}}}`"),
                span,
                verbatim,
            );
        }
        if let Some(end_name) = end
            .as_ref()
            .and_then(|e| e.first_token())
            .filter(|t| t.kind() == SyntaxKind::TokenCommandSym)
            .map(|t| t.text().to_string())
            && end_name != name
        {
            // Which environment the author meant is unknowable; keep the
            // whole thing verbatim rather than guess a layout.
            let verbatim = self.verbatim(&span, &name);
            return self.error(
                ProblemKind::EnvironmentMismatch,
                &format!("`\\begin{{{name}}}` closed by `\\end{{{end_name}}}`"),
                span,
                verbatim,
            );
        }

        let Some(Semantics::Env {
            layout,
            delims,
            cols_from_arg,
        }) = self.spec.semantics(&name).cloned()
        else {
            let verbatim = self.verbatim(&span, &name);
            let mut items = vec![self.error(
                ProblemKind::UnknownEnvironment,
                &format!("unknown environment `{name}`"),
                name_span,
                verbatim,
            )];
            // Keep the body as content so nothing is silently lost.
            for child in n.children_with_tokens() {
                match child {
                    NodeOrToken::Node(c)
                        if matches!(c.kind(), SyntaxKind::ItemBegin | SyntaxKind::ItemEnd) => {}
                    _ => self.items_into(std::iter::once(child), &mut items),
                }
            }
            return Node::new(NodeKind::Row(items), span);
        };

        // `array`'s column spec is an argument of \begin, not a cell.
        let mut column_spec: Option<String> = None;
        if cols_from_arg
            && let Some(arg) = begin.as_ref().and_then(|b| {
                b.children()
                    .find(|c| c.kind() == SyntaxKind::ClauseArgument)
            })
        {
            column_spec = Some(argument_text(&arg));
        }
        let _ = column_spec; // alignment only; not used by the writers yet

        // Split the body into rows (at `\\`) and cells (at `&`).
        let mut rows: Vec<Vec<Vec<Node>>> = vec![vec![Vec::new()]];
        let mut row_has_content = false;
        let has_nontrivia = |items: &[Node]| !items.is_empty();
        for child in n.children_with_tokens() {
            match &child {
                NodeOrToken::Node(c)
                    if matches!(c.kind(), SyntaxKind::ItemBegin | SyntaxKind::ItemEnd) => {}
                NodeOrToken::Token(t) if t.kind() == SyntaxKind::ItemNewLine => {
                    rows.push(vec![Vec::new()]);
                    row_has_content = false;
                }
                NodeOrToken::Token(t) if t.kind() == SyntaxKind::TokenAmpersand => {
                    rows.last_mut().unwrap().push(Vec::new());
                    row_has_content = true;
                }
                _ => {
                    let cell = rows.last_mut().unwrap().last_mut().unwrap();
                    let before = cell.len();
                    self.items_into(std::iter::once(child.clone()), cell);
                    if cell.len() > before {
                        row_has_content = true;
                    }
                }
            }
        }
        let _ = row_has_content;
        // Drop rows that carry nothing (a trailing `\\`, an `\hline` line).
        let mut rows: Vec<Vec<Vec<Node>>> = rows
            .into_iter()
            .filter(|cells| cells.len() > 1 || cells.iter().any(|c| has_nontrivia(c)))
            .collect();
        let width = rows.iter().map(Vec::len).max().unwrap_or(0);
        let mut ragged = false;
        for cells in &mut rows {
            if cells.len() < width {
                ragged = true;
                while cells.len() < width {
                    cells.push(Vec::new());
                }
            }
        }
        if ragged {
            self.problem(
                ProblemKind::RaggedRows,
                format!("rows of `{name}` have different numbers of cells; short rows padded"),
                name_span,
            );
        }
        let rows: Vec<Vec<Node>> = rows
            .into_iter()
            .map(|cells| {
                cells
                    .into_iter()
                    .map(|items| {
                        let s = span_of_nodes(&items);
                        Node::new(NodeKind::Row(items), s)
                    })
                    .collect()
            })
            .collect();
        let delims = match layout {
            EnvLayout::Matrix => delims,
            _ => None,
        };
        Node::new(
            NodeKind::Matrix {
                layout,
                delims,
                rows,
            },
            span,
        )
    }
}

// -- helpers ------------------------------------------------------------------

fn index_tokens(node: &SyntaxNode, index: &mut HashMap<u32, usize>, next: &mut usize) {
    for child in node.children_with_tokens() {
        match child {
            NodeOrToken::Node(n) => index_tokens(&n, index, next),
            NodeOrToken::Token(t) => {
                index.insert(t.text_range().start().into(), *next);
                *next += 1;
            }
        }
    }
}

/// A single item stands for itself; several become a `Row`.
fn unwrap_single(mut items: Vec<Node>, span: Option<Range<usize>>) -> Node {
    if items.len() == 1 {
        let mut n = items.pop().unwrap();
        if let (Some(s), Some(inner)) = (&span, &n.span)
            && (s.start < inner.start || s.end > inner.end)
        {
            // Keep the wider span (a brace group is wider than its content).
            return Node::new(NodeKind::Row(vec![n]), span);
        }
        if n.span.is_none() {
            n.span = span;
        }
        n
    } else {
        Node::new(NodeKind::Row(items), span)
    }
}

fn is_brace_token(c: &NodeOrToken<SyntaxNode, SyntaxToken>, kind: SyntaxKind) -> bool {
    let NodeOrToken::Token(t) = c else {
        return false;
    };
    match kind {
        SyntaxKind::ItemCurly => {
            matches!(t.kind(), SyntaxKind::TokenLBrace | SyntaxKind::TokenRBrace)
        }
        SyntaxKind::ItemBracket => {
            matches!(
                t.kind(),
                SyntaxKind::TokenLBracket | SyntaxKind::TokenRBracket
            )
        }
        SyntaxKind::ItemParen => {
            matches!(t.kind(), SyntaxKind::TokenLParen | SyntaxKind::TokenRParen)
        }
        _ => false,
    }
}

fn has_closing_brace(curly: &SyntaxNode) -> bool {
    curly
        .last_token()
        .is_some_and(|t| t.kind() == SyntaxKind::TokenRBrace)
}

fn is_middle(c: &SyntaxNode) -> bool {
    c.kind() == SyntaxKind::ItemCmd
        && c.children_with_tokens()
            .filter_map(|c| c.into_token())
            .any(|t| t.kind() == SyntaxKind::ClauseCommandName && t.text() == "\\middle")
}

/// The literal text of an argument clause, braces stripped.
fn argument_text(arg: &SyntaxNode) -> String {
    let inner = arg
        .children()
        .find(|c| matches!(c.kind(), SyntaxKind::ItemCurly | SyntaxKind::ItemBracket));
    match inner {
        Some(group) => group
            .children_with_tokens()
            .filter(|c| !is_brace_token(c, group.kind()))
            .map(|c| match c {
                NodeOrToken::Node(n) => n.text().to_string(),
                NodeOrToken::Token(t) => t.text().to_string(),
            })
            .collect(),
        None => arg.text().to_string(),
    }
}

/// `1em`, `0.5em`, `2 em` → em; other units are left to the default.
fn parse_em(s: &str) -> Option<f32> {
    let s = s.trim();
    let num = s.strip_suffix("em")?.trim();
    num.parse::<f32>().ok()
}

/// For `\sqrt` / `\xrightarrow` (glob `{,b}t`): the operand from a bracket
/// argument, if present, and the body.
fn split_optional(args: &[SyntaxNode], operands: &mut Vec<Node>) -> (Option<Node>, Option<Node>) {
    let mut optional = None;
    let mut body = None;
    for (arg, operand) in args.iter().zip(operands.drain(..)) {
        let is_bracket = arg.children().any(|c| c.kind() == SyntaxKind::ItemBracket);
        if is_bracket && optional.is_none() && body.is_none() {
            optional = Some(operand);
        } else if body.is_none() {
            body = Some(operand);
        }
    }
    (optional, body)
}

/// Move operand `i` out, leaving an empty row behind.
fn take_op(operands: &mut [Node], i: usize) -> Option<Node> {
    if i < operands.len() {
        Some(std::mem::replace(
            &mut operands[i],
            Node::new(NodeKind::Row(vec![]), None),
        ))
    } else {
        None
    }
}

/// Split a flat item list into single-cell rows at `Break` nodes.
fn rows_from_breaks(items: Vec<Node>) -> Vec<Vec<Node>> {
    let mut rows: Vec<Vec<Node>> = vec![Vec::new()];
    for item in items {
        if matches!(item.kind, NodeKind::Break) {
            rows.push(Vec::new());
        } else {
            rows.last_mut().unwrap().push(item);
        }
    }
    rows.into_iter()
        .filter(|r| !r.is_empty())
        .map(|items| {
            let s = span_of_nodes(&items);
            vec![Node::new(NodeKind::Row(items), s)]
        })
        .collect()
}

fn single_text(node: &Node) -> Option<&str> {
    match &node.kind {
        NodeKind::Run(t) | NodeKind::Sym(t) => Some(t),
        NodeKind::Row(items) if items.len() == 1 => single_text(&items[0]),
        _ => None,
    }
}

fn operator_limits(node: &Node) -> LimLoc {
    match &node.kind {
        NodeKind::Nary { limits, .. } | NodeKind::Func { limits, .. } => *limits,
        _ => LimLoc::Auto,
    }
}

/// Set explicit limits on an operator, looking through `Scripts`.
fn set_limits(node: &mut Node, target: LimLoc) -> bool {
    match &mut node.kind {
        NodeKind::Nary { limits, .. } | NodeKind::Func { limits, .. } => {
            *limits = target;
            true
        }
        NodeKind::Scripts { base, limits, .. } => {
            if set_limits(base, target) {
                *limits = target;
                true
            } else {
                false
            }
        }
        NodeKind::Row(items) if items.len() == 1 => set_limits(&mut items[0], target),
        _ => false,
    }
}
