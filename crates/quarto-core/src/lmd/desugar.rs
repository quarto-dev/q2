/*
 * lmd/desugar.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! lmd source → plain Lua: every markdown fragment becomes `pandoc.*` calls.
//!
//! The Lua is parsed with `tree-sitter-lmd`; each `markdown_fragment` node
//! (never nested in the tree: fragment content is one raw token) is cut out
//! and replaced by the emitted expression. Markdown inside a fragment is
//! parsed with pampa's qmd reader; lmd code inside *that* markdown comes
//! back through [`desugar_lua_at`] / [`desugar_lua_expr_with`] from the
//! emitter, which is how fragments nest.
//!
//! With a source [`Anchor`] (where the text lives in the document), each
//! fragment's markdown is parsed *under* the span of its content, so the
//! nodes and any parse diagnostics reroot into the document; and in
//! [`EmitMode::LinePreserving`] every expansion is padded to the lines the
//! fragment occupied, so Lua line numbers stay document line numbers.

use quarto_error_reporting::DiagnosticMessage;
use quarto_pandoc_types::block::Block;
use quarto_source_map::SourceInfo;

use super::anchor::Anchor;
use super::emit::Emitter;

/// Why an lmd cell could not be desugared. Every variant leaves the cell as
/// it was; the stage reports the error.
#[derive(Debug)]
pub enum LmdError {
    /// Lua syntax error, with the position in the cell text (1-based).
    LuaParse { line: usize, column: usize },
    /// A fragment's markdown failed to parse. `diagnostics` are the reader's
    /// own, rerooted into the document when the parse was anchored.
    MarkdownParse {
        summary: String,
        diagnostics: Vec<DiagnosticMessage>,
    },
    /// A node kind the emitter has no constructor for, at its span when the
    /// parse was anchored.
    Unsupported {
        kind: &'static str,
        location: Option<SourceInfo>,
    },
    /// `{=[ … ]=}` asked for inlines but the markdown parsed to this many blocks.
    NotInlines(usize),
}

impl std::fmt::Display for LmdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LmdError::LuaParse { line, column } => {
                write!(f, "lmd: Lua syntax error at line {line}, column {column}")
            }
            LmdError::MarkdownParse { summary, .. } => {
                write!(f, "lmd: markdown fragment failed to parse: {summary}")
            }
            LmdError::Unsupported { kind, .. } => write!(
                f,
                "lmd: `{kind}` nodes are not supported inside markdown fragments yet"
            ),
            LmdError::NotInlines(n) => write!(
                f,
                "lmd: `{{=[ … ]=}}` asks for inlines but the markdown parsed to {n} blocks"
            ),
        }
    }
}

impl std::error::Error for LmdError {}

/// How emitted Lua is laid out. See `emit.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmitMode {
    /// Readable: one constructor per line.
    Pretty,
    /// Runnable with document line numbers: each fragment expansion occupies
    /// exactly the source lines the fragment did.
    LinePreserving,
}

/// What a desugar knows about where its text lives and how to lay out the
/// result.
#[derive(Debug, Clone, Copy)]
pub struct DesugarOptions<'a> {
    pub anchor: Option<&'a Anchor>,
    pub mode: EmitMode,
}

impl DesugarOptions<'_> {
    /// No anchor, pretty output: the form the plan's samples use.
    pub const PRETTY: DesugarOptions<'static> = DesugarOptions {
        anchor: None,
        mode: EmitMode::Pretty,
    };
}

/// Which of the three fragment forms a `markdown_fragment` node is, read
/// off the node kind of its `start` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FragmentKind {
    /// `{= … =}`: `pandoc.Nodes`, the surrounding context decides.
    Nodes,
    /// `{=: … :=}`: `pandoc.Blocks`.
    Blocks,
    /// `{=[ … ]=}`: `pandoc.Inlines`.
    Inlines,
}

impl FragmentKind {
    fn of(fragment: tree_sitter::Node<'_>) -> Self {
        let start = fragment
            .child_by_field_name("start")
            .expect("markdown_fragment has a start field");
        match start.kind() {
            tree_sitter_lmd::BLOCKS_START => Self::Blocks,
            tree_sitter_lmd::INLINES_START => Self::Inlines,
            tree_sitter_lmd::NODES_START => Self::Nodes,
            other => unreachable!("unknown fragment start kind {other:?}"),
        }
    }
}

/// Desugar `src` (lmd) into plain Lua: unanchored, pretty.
pub fn desugar_lua(src: &str) -> Result<String, LmdError> {
    desugar_lua_with(src, &DesugarOptions::PRETTY)
}

/// Desugar a chunk with the given anchor and layout.
pub fn desugar_lua_with(src: &str, opts: &DesugarOptions<'_>) -> Result<String, LmdError> {
    desugar_inner(src, 0, 0, opts)
}

/// [`desugar_lua_with`] for a chunk spliced in at column `base`: in pretty
/// mode, continuation lines of each emitted fragment are indented past it.
pub(super) fn desugar_lua_at(
    src: &str,
    base: usize,
    opts: &DesugarOptions<'_>,
) -> Result<String, LmdError> {
    desugar_inner(src, base, 0, opts)
}

/// Desugar a single Lua *expression* (what an inline `` `{lmd} expr` ``
/// holds), unanchored, pretty. A bare expression is not a chunk, so it is
/// parsed as `return <expr>` and the prefix is stripped again.
pub fn desugar_lua_expr(src: &str, base: usize) -> Result<String, LmdError> {
    desugar_lua_expr_with(src, base, &DesugarOptions::PRETTY)
}

/// [`desugar_lua_expr`] with an anchor and layout.
pub fn desugar_lua_expr_with(
    src: &str,
    base: usize,
    opts: &DesugarOptions<'_>,
) -> Result<String, LmdError> {
    const PREFIX: &str = "return ";
    let wrapped = format!("{PREFIX}{src}");
    let out = desugar_inner(&wrapped, base, PREFIX.len(), opts)?;
    Ok(out[PREFIX.len()..].to_string())
}

/// `prefix_len` bytes were prepended to the caller's text (all on its first
/// line): fragment columns on that line and all fragment byte offsets are
/// shifted back by it, so an exact anchor still lines up with the text.
fn desugar_inner(
    src: &str,
    base: usize,
    prefix_len: usize,
    opts: &DesugarOptions<'_>,
) -> Result<String, LmdError> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_lmd::LANGUAGE.into())
        .expect("lmd grammar loads");
    let tree = parser
        .parse(src, None)
        .expect("tree-sitter parse returns a tree");
    let root = tree.root_node();
    if root.has_error() {
        let err = first_error(root);
        let pos = err.start_position();
        let shift = if pos.row == 0 { prefix_len } else { 0 };
        return Err(LmdError::LuaParse {
            line: pos.row + 1,
            column: pos.column.saturating_sub(shift) + 1,
        });
    }

    let mut out = String::with_capacity(src.len());
    let mut last = 0;
    for fragment in fragments(root) {
        out.push_str(&src[last..fragment.start_byte()]);
        let content = fragment.child_by_field_name("content");
        let raw = content.map_or("", |c| &src[c.byte_range()]);
        let lead = raw.len() - raw.trim_start().len();
        let trail = raw.len() - raw.trim_end().len();
        let markdown = raw.trim();

        // The document span of the trimmed markdown, for the fragment
        // parse to reroot into. Exact anchors are byte-for-byte the
        // caller's text; approximate ones locate the containing node only.
        let parent = opts.anchor.map(|anchor| {
            if anchor.exact {
                let (s, e) = match content {
                    Some(c) => (c.start_byte() + lead, c.end_byte() - trail),
                    None => (fragment.start_byte(), fragment.end_byte()),
                };
                SourceInfo::substring(
                    anchor.span.clone(),
                    s.saturating_sub(prefix_len),
                    e.saturating_sub(prefix_len),
                )
            } else {
                anchor.span.clone()
            }
        });

        let pos = fragment.start_position();
        let shift = if pos.row == 0 { prefix_len } else { 0 };
        let column = base + pos.column.saturating_sub(shift);
        let mut emitted =
            emit_fragment(markdown, column, FragmentKind::of(fragment), parent, opts)?;
        // `callout{=: … :=}`: the fragment is the call's whole argument
        // list (as `f{…}` and `f"…"` are in Lua), so the expansion needs
        // the parentheses Lua wants around an expression argument.
        if is_call_argument(fragment) {
            emitted.insert(0, '(');
            emitted.push(')');
        }
        if opts.mode == EmitMode::LinePreserving {
            pad_to_source_lines(&mut emitted, fragment.end_position().row - pos.row);
        }
        out.push_str(&emitted);
        last = fragment.end_byte();
    }
    out.push_str(&src[last..]);
    Ok(out)
}

/// Make a fragment's expansion occupy exactly `rows` line breaks (the
/// fragment's own span) by padding newlines right after the opening `(`,
/// where Lua ignores them. Newlines already inside the expansion come from
/// a block lmd cell spliced from the fragment's markdown, which are source
/// lines of the fragment, so they count toward the budget.
fn pad_to_source_lines(emitted: &mut String, rows: usize) {
    let used = emitted.matches('\n').count();
    let pad = rows.saturating_sub(used);
    if pad > 0
        && let Some(open) = emitted.find('(')
    {
        emitted.insert_str(open + 1, &"\n".repeat(pad));
    }
}

/// Parse one fragment's markdown and print it as a Lua expression.
///
/// - `{= … =}` becomes `pandoc.Nodes({ blocks })`: the reader's blocks,
///   handed to a value that adapts to the context it lands in (a `Para`
///   stays a `Para` in block context and unwraps to its inlines in inline
///   context; see `pampa::lua::nodes` for the full table).
/// - `{=: … :=}` becomes `pandoc.Blocks({ blocks })`.
/// - `{=[ … ]=}` becomes `pandoc.Inlines({ inlines })`, where the markdown
///   must parse to a single paragraph (or nothing); anything else is an
///   error, since the author asked for inlines.
fn emit_fragment(
    markdown: &str,
    column: usize,
    kind: FragmentKind,
    parent: Option<SourceInfo>,
    opts: &DesugarOptions<'_>,
) -> Result<String, LmdError> {
    let anchored = parent.is_some();
    let blocks = parse_markdown(markdown, parent)?;
    let mut emitter = Emitter::new(column, opts.mode, anchored);
    match kind {
        FragmentKind::Nodes => emitter.blocks_call("Nodes", &blocks)?,
        FragmentKind::Blocks => emitter.blocks_call("Blocks", &blocks)?,
        FragmentKind::Inlines => match blocks.as_slice() {
            [] => emitter.inlines_call("Inlines", &[])?,
            [Block::Paragraph(p)] => emitter.inlines_call("Inlines", &p.content)?,
            [Block::Plain(p)] => emitter.inlines_call("Inlines", &p.content)?,
            other => return Err(LmdError::NotInlines(other.len())),
        },
    }
    Ok(emitter.finish())
}

/// Parse a fragment's markdown. With `parent`, the reader reroots every
/// node span and every parse diagnostic into it (the same path front-matter
/// markdown uses); without it, diagnostics are stripped of their locations,
/// which would otherwise point into a private, meaningless file.
///
/// The blocks are then finished as a fragment
/// (`pampa::transforms::fragment`): footnotes defined in the fragment are
/// paired with its references into `Note`s, and the ids the reader derived
/// for headings are dropped so the document derives them from the final
/// text.
fn parse_markdown(markdown: &str, parent: Option<SourceInfo>) -> Result<Vec<Block>, LmdError> {
    if markdown.is_empty() {
        return Ok(vec![]);
    }
    let anchored = parent.is_some();
    let mut sink: Vec<u8> = Vec::new();
    match pampa::readers::qmd::read(
        markdown.as_bytes(),
        false,
        "fragment.lmd",
        &mut sink,
        true,
        parent,
    ) {
        Ok((pandoc, _ctx, _warnings)) => Ok(pampa::transforms::fragment::prepare_fragment_blocks(
            pandoc.blocks,
        )),
        Err(mut diags) => {
            if !anchored {
                for d in &mut diags {
                    d.location = None;
                    for detail in &mut d.details {
                        detail.location = None;
                    }
                }
            }
            Err(LmdError::MarkdownParse {
                summary: diags
                    .iter()
                    .map(|d| d.title.clone())
                    .collect::<Vec<_>>()
                    .join("; "),
                diagnostics: diags,
            })
        }
    }
}

/// Whether `fragment` stands as a call's whole argument list, `f{= … =}`.
/// Inside `f({= … =})` the fragment's parent is the same `arguments` node,
/// but its first child is the `(`.
fn is_call_argument(fragment: tree_sitter::Node<'_>) -> bool {
    fragment.parent().is_some_and(|p| {
        p.kind() == tree_sitter_lmd::ARGUMENTS
            && p.child(0).is_some_and(|first| first.id() == fragment.id())
    })
}

/// Every `markdown_fragment` node under `root`, in source order. The walk
/// does not descend into a fragment: its content is a single raw token.
fn fragments(root: tree_sitter::Node<'_>) -> Vec<tree_sitter::Node<'_>> {
    let mut found = Vec::new();
    let mut cursor = root.walk();
    let mut descend = true;
    loop {
        let node = cursor.node();
        if node.kind() == tree_sitter_lmd::MARKDOWN_FRAGMENT {
            found.push(node);
            descend = false;
        }
        if descend && cursor.goto_first_child() {
            continue;
        }
        descend = true;
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return found;
            }
        }
    }
}

fn first_error(root: tree_sitter::Node<'_>) -> tree_sitter::Node<'_> {
    let mut cursor = root.walk();
    loop {
        let node = cursor.node();
        if node.is_error() || node.is_missing() {
            return node;
        }
        if cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return root;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line_preserving(src: &str) -> Result<String, LmdError> {
        desugar_lua_with(
            src,
            &DesugarOptions {
                anchor: None,
                mode: EmitMode::LinePreserving,
            },
        )
    }

    #[test]
    fn lua_without_fragments_is_unchanged() {
        let src = "local t = { 1, x = 2 }\nreturn t";
        assert_eq!(desugar_lua(src).unwrap(), src);
    }

    #[test]
    fn unmarked_fragment_becomes_nodes() {
        let out = desugar_lua("return {= **hello** =}").unwrap();
        assert_eq!(
            out,
            r#"return pandoc.Nodes({
         pandoc.Para({
           pandoc.Strong({
             pandoc.Str("hello"),
           }),
         }),
       })"#
        );
    }

    #[test]
    fn blocks_fragment_becomes_blocks() {
        let out = desugar_lua("return {=: **hello** :=}").unwrap();
        assert!(out.starts_with("return pandoc.Blocks({\n"), "{out}");
        assert!(out.contains("pandoc.Para({"), "{out}");
    }

    #[test]
    fn inlines_fragment_becomes_inlines_without_a_para() {
        let out = desugar_lua("return {=[ **hello** ]=}").unwrap();
        assert_eq!(
            out,
            r#"return pandoc.Inlines({
         pandoc.Strong({
           pandoc.Str("hello"),
         }),
       })"#
        );
    }

    #[test]
    fn inlines_fragment_rejects_several_blocks() {
        let err = desugar_lua("return {=[ a\n\nb ]=}").unwrap_err();
        assert!(matches!(err, LmdError::NotInlines(2)), "{err}");
    }

    #[test]
    fn inlines_fragment_can_sit_inside_a_constructor() {
        let out = desugar_lua("return pandoc.Strong({=[ *a* ]=})").unwrap();
        assert!(
            out.starts_with("return pandoc.Strong(pandoc.Inlines({\n"),
            "{out}"
        );
        assert!(out.contains("pandoc.Emph({"), "{out}");
    }

    #[test]
    fn fragment_headings_carry_no_derived_id_but_keep_explicit_ones() {
        let out =
            desugar_lua("return {=:\n### `{lmd} title`\n\n### Pinned {#pinned}\n:=}").unwrap();
        assert!(!out.contains("lmd-title"), "{out}");
        assert!(out.contains("pandoc.Header(3, {\n"), "{out}");
        assert!(out.contains("pandoc.Attr(\"pinned\""), "{out}");
    }

    #[test]
    fn inline_lmd_code_splices_lua_back_in() {
        let src = "return tmap(xs, function(v) return {=: **`{lmd} v`** :=} end)";
        let out = desugar_lua(src).unwrap();
        assert!(out.contains("pandoc.Strong({\n"), "{out}");
        assert!(out.contains("  v,\n"), "{out}");
        assert!(out.ends_with(" end)"), "{out}");
        assert!(!out.contains("{="), "{out}");
    }

    #[test]
    fn nested_fragment_inside_inline_lmd_code() {
        let src = "return {=: *a* `{lmd} f({=[ b ]=})` :=}";
        let out = desugar_lua(src).unwrap();
        assert!(out.contains("pandoc.Emph("), "{out}");
        assert!(out.contains("f(pandoc.Inlines({"), "{out}");
        assert!(out.contains("pandoc.Str(\"b\")"), "{out}");
        assert!(!out.contains("{="), "{out}");
    }

    #[test]
    fn block_lmd_cell_inside_fragment_becomes_an_iife() {
        let src = "return {=:\nIntro\n\n```{lmd}\nreturn x\n```\n:=}";
        let out = desugar_lua(src).unwrap();
        assert!(out.starts_with("return pandoc.Blocks({\n"), "{out}");
        assert!(out.contains("(function()\n"), "{out}");
        assert!(out.contains("return x\n"), "{out}");
        assert!(out.contains("end)(),\n"), "{out}");
    }

    #[test]
    fn empty_fragments() {
        assert_eq!(
            desugar_lua("return {==}").unwrap(),
            "return pandoc.Nodes({})"
        );
        assert_eq!(
            desugar_lua("return {=::=}").unwrap(),
            "return pandoc.Blocks({})"
        );
        assert_eq!(
            desugar_lua("return {=[]=}").unwrap(),
            "return pandoc.Inlines({})"
        );
    }

    /// `callout{=: … :=}`: a fragment as a call's argument list becomes a
    /// parenthesized argument, so the result is the call Lua expects.
    #[test]
    fn a_fragment_as_the_calls_argument_is_parenthesized() {
        let out = desugar_lua("return callout{=: body :=}").unwrap();
        assert!(out.starts_with("return callout(pandoc.Blocks({\n"), "{out}");
        assert!(out.ends_with("}))"), "{out}");
        let out = desugar_lua("return card:render{=[ title ]=}").unwrap();
        assert!(
            out.starts_with("return card:render(pandoc.Inlines({\n"),
            "{out}"
        );
        let out = desugar_lua("local x = f{= a =}\nreturn x").unwrap();
        assert!(out.starts_with("local x = f(pandoc.Nodes({\n"), "{out}");
        // An ordinary argument position is unchanged: no doubled parens.
        let out = desugar_lua("return f({= a =})").unwrap();
        assert!(out.starts_with("return f(pandoc.Nodes({\n"), "{out}");
        assert!(!out.contains("(("), "{out}");
    }

    #[test]
    fn a_call_argument_fragment_keeps_its_lines_when_line_preserving() {
        let src = "return callout{=:\nOne.\n\nTwo.\n:=}\n";
        let out = line_preserving(src).unwrap();
        assert_eq!(out.lines().count(), src.lines().count(), "{out}");
        assert!(
            out.starts_with("return callout(\n\n\n\npandoc.Blocks("),
            "{out}"
        );
        assert!(out.ends_with("}))\n"), "{out}");
    }

    /// A backslash makes the next character content to the scanner, so
    /// `\{=` does not open; and since Quarto markdown wants a literal brace
    /// escaped anyway (`\{`, `\}`), the spellings that read as the literal
    /// delimiters are `\{=` and `=\}`.
    #[test]
    fn a_backslash_escapes_the_delimiters() {
        let out = desugar_lua("return {=: write \\{= and =\\} :=}").unwrap();
        assert!(out.contains("pandoc.Str(\"{=\")"), "{out}");
        assert!(out.contains("pandoc.Str(\"=}\")"), "{out}");
        assert!(!out.contains('\\'), "{out}");
    }

    /// `{==: … :==}`: the content is raw up to the closer of the same
    /// level, so fragment syntax inside is text; inline lmd code inside
    /// is still code, since that is the markdown reader's business.
    #[test]
    fn a_leveled_fragment_quotes_fragment_syntax() {
        let out =
            desugar_lua("return {==: Write `{=: x :=}` or `{=[ y ]=}`, see `=}` :==}").unwrap();
        assert!(out.contains("pandoc.Code(\"{=: x :=}\")"), "{out}");
        assert!(out.contains("pandoc.Code(\"{=[ y ]=}\")"), "{out}");
        assert!(out.contains("pandoc.Code(\"=}\")"), "{out}");
        let out = desugar_lua("return {==[ **b** `{lmd} v` ]==}").unwrap();
        assert!(out.starts_with("return pandoc.Inlines({\n"), "{out}");
        assert!(out.contains("  v,\n"), "{out}");
        let out = desugar_lua("return {== a ==}").unwrap();
        assert!(out.starts_with("return pandoc.Nodes({\n"), "{out}");
        assert_eq!(
            desugar_lua("return {==::==}").unwrap(),
            "return pandoc.Blocks({})"
        );
    }

    #[test]
    fn several_fragments_in_one_chunk() {
        let out = desugar_lua("local a = {= x =}\nlocal b = {=[ y ]=}").unwrap();
        assert_eq!(out.matches("pandoc.Nodes(").count(), 1);
        assert_eq!(out.matches("pandoc.Inlines(").count(), 1);
        assert!(out.contains("pandoc.Str(\"x\")"));
        assert!(out.contains("pandoc.Str(\"y\")"));
    }

    #[test]
    fn lua_syntax_error_is_reported_with_position() {
        let err = desugar_lua("return {= a =} +").unwrap_err();
        assert!(matches!(err, LmdError::LuaParse { .. }), "{err}");
    }

    #[test]
    fn unsupported_node_is_an_error_without_a_location_when_unanchored() {
        // A shortcode: resolved later in the pipeline, no constructor
        // (bd-onsqz7sv).
        let src = "return {=: Version {{< meta version >}} here. :=}";
        let err = desugar_lua(src).unwrap_err();
        assert!(
            matches!(
                err,
                LmdError::Unsupported {
                    kind: "Shortcode",
                    location: None
                }
            ),
            "{err}"
        );
    }

    /// A footnote defined in the fragment is paired with its reference at
    /// parse time and printed as an inline `pandoc.Note`, since the
    /// reader's definition block has no constructor.
    #[test]
    fn a_footnote_defined_in_the_fragment_becomes_a_note() {
        let out = desugar_lua("return {=: text[^1]\n\n[^1]: the *note* :=}").unwrap();
        assert!(out.contains("pandoc.Note({\n"), "{out}");
        assert!(out.contains("pandoc.Emph({"), "{out}");
        assert!(!out.contains("quarto-note-reference"), "{out}");
        assert!(!out.contains("NoteDefinition"), "{out}");
        let fenced = desugar_lua("return {=:\ntext[^a]\n\n::: ^a\nOne.\n\nTwo.\n:::\n:=}").unwrap();
        assert_eq!(fenced.matches("pandoc.Note(").count(), 1, "{fenced}");
        assert_eq!(fenced.matches("pandoc.Para(").count(), 3, "{fenced}");
    }

    /// A reference whose definition lives in the document body stays the
    /// reader's reference span; the document's footnote transform resolves
    /// it after the cell has run.
    #[test]
    fn a_footnote_reference_defined_outside_the_fragment_stays_a_reference() {
        let out = desugar_lua("return {=[ see[^doc] ]=}").unwrap();
        assert!(
            out.contains("pandoc.Span({}, pandoc.Attr(\"\", {\n"),
            "{out}"
        );
        assert!(out.contains("\"quarto-note-reference\""), "{out}");
        assert!(out.contains("{\"reference-id\", \"doc\"}"), "{out}");
    }

    /// bd-7gqr148v: tables are printed, pipe tables and list-tables alike
    /// (the reader has already turned the latter into a `Table`).
    #[test]
    fn table_in_a_fragment_is_emitted() {
        let pipe = desugar_lua("return {=: | a | b |\n|---|--:|\n| 1 | 2 | :=}").unwrap();
        assert!(pipe.starts_with("return pandoc.Blocks({\n"), "{pipe}");
        assert!(pipe.contains("pandoc.Table({}, {\n"), "{pipe}");
        assert!(
            pipe.contains("{\"AlignDefault\", pandoc.ColWidthDefault}"),
            "{pipe}"
        );
        assert!(
            pipe.contains("{\"AlignRight\", pandoc.ColWidthDefault}"),
            "{pipe}"
        );
        assert!(pipe.contains("pandoc.TableHead({\n"), "{pipe}");
        assert!(pipe.contains("pandoc.Row({\n"), "{pipe}");
        assert!(pipe.contains("pandoc.Cell({\n"), "{pipe}");
        assert!(pipe.contains("}, \"AlignDefault\", 1, 1)"), "{pipe}");
        assert!(pipe.contains("pandoc.TableBody({\n"), "{pipe}");
        assert!(pipe.contains("pandoc.TableFoot({})"), "{pipe}");

        let list_table = desugar_lua(
            "return {=:\n::: {.list-table widths=\"30,70\"}\n* - k\n  - v\n* - a\n  - 1\n:::\n:=}",
        )
        .unwrap();
        assert!(list_table.contains("pandoc.Table("), "{list_table}");
        assert!(
            list_table.contains("{\"AlignDefault\", 0.3}"),
            "{list_table}"
        );
        assert!(
            list_table.contains("{\"AlignDefault\", 0.7}"),
            "{list_table}"
        );
        assert!(!list_table.contains("list-table"), "{list_table}");
    }

    #[test]
    fn line_preserving_table_stays_on_its_lines() {
        let src = "local t = {=:\n| a | b |\n|---|---|\n| 1 | 2 |\n:=}\nlocal b = 1";
        let out = line_preserving(src).unwrap();
        assert_eq!(out.lines().count(), src.lines().count(), "{out}");
        assert_eq!(out.lines().last(), Some("local b = 1"));
        assert!(out.contains("pandoc.Table({}, {{\"AlignDefault\", pandoc.ColWidthDefault}, {\"AlignDefault\", pandoc.ColWidthDefault}}, pandoc.TableHead({pandoc.Row({pandoc.Cell({pandoc.Plain({pandoc.Str(\"a\")})}, \"AlignDefault\", 1, 1), pandoc.Cell({pandoc.Plain({pandoc.Str(\"b\")})}, \"AlignDefault\", 1, 1)})}), {pandoc.TableBody({pandoc.Row({pandoc.Cell({pandoc.Plain({pandoc.Str(\"1\")})}, \"AlignDefault\", 1, 1), pandoc.Cell({pandoc.Plain({pandoc.Str(\"2\")})}, \"AlignDefault\", 1, 1)})}, {}, 0)}, pandoc.TableFoot({}))"), "{out}");
    }

    #[test]
    fn markdown_parse_errors_carry_the_readers_diagnostics() {
        // Q-2-10: a stray quote mark with whitespace on both sides opens
        // a quote that never closes.
        let src = "return {=: the groups ' Unique IDs :=}";
        let err = desugar_lua(src).unwrap_err();
        let LmdError::MarkdownParse { diagnostics, .. } = &err else {
            panic!("expected MarkdownParse, got {err}");
        };
        assert!(!diagnostics.is_empty());
        // Unanchored: no location, rather than one into a private file.
        assert!(diagnostics.iter().all(|d| d.location.is_none()));
    }

    // ----- line-preserving mode -----

    #[test]
    fn line_preserving_keeps_single_line_fragments_on_one_line() {
        let out = line_preserving("return {=: **hello** :=}").unwrap();
        assert_eq!(
            out,
            r#"return pandoc.Blocks({pandoc.Para({pandoc.Strong({pandoc.Str("hello")})})})"#
        );
    }

    #[test]
    fn line_preserving_pads_multi_line_fragments_to_their_source_lines() {
        let src = "local a = {=:\nOne.\n\nTwo.\n:=}\nlocal b = 1";
        let out = line_preserving(src).unwrap();
        assert_eq!(
            out.lines().count(),
            src.lines().count(),
            "line count must match\n{out}"
        );
        // `local b` is still on line 6 of the output.
        assert_eq!(out.lines().nth(5), Some("local b = 1"));
        assert!(
            out.starts_with("local a = pandoc.Blocks(\n\n\n\n{"),
            "{out}"
        );
    }

    #[test]
    fn line_preserving_counts_a_nested_cells_lines_toward_the_budget() {
        let src = "local a = {=:\nIntro\n\n```{lmd}\nlocal y = 1\nreturn y\n```\n:=}\nlocal b = 2";
        let out = line_preserving(src).unwrap();
        assert_eq!(out.lines().count(), src.lines().count(), "{out}");
        assert_eq!(out.lines().last(), Some("local b = 2"));
        assert!(
            out.contains("(function() local y = 1\nreturn y end)()"),
            "{out}"
        );
    }
}
