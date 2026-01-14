/*
 * reconcile-viewer
 * Copyright (c) 2025 Posit, PBC
 *
 * Experimental tool for viewing QMD reconciliation plans in a human-readable JSON format.
 * Unlike the raw reconciliation plan output, this shows actual content snippets
 * alongside the alignment decisions.
 */
#![feature(trim_prefix_suffix)]

use clap::Parser;
use pampa::readers;
use quarto_pandoc_types::block::{Block, Blocks};
use quarto_pandoc_types::inline::{Inline, Inlines};
use quarto_pandoc_types::reconcile::{
    BlockAlignment, InlineAlignment, InlineReconciliationPlan, ReconciliationPlan,
    ReconciliationStats, compute_reconciliation,
};
use serde::Serialize;
use std::io;

#[derive(Parser, Debug)]
#[command(name = "reconcile-viewer")]
#[command(about = "View QMD reconciliation plans in human-readable JSON format")]
struct Args {
    /// The first qmd file (before)
    #[arg(short = 'b', long = "before")]
    before: String,

    /// The second qmd file (after)
    #[arg(short = 'a', long = "after")]
    after: String,

    /// Maximum content snippet length (default: 60)
    #[arg(short = 's', long = "snippet-len", default_value = "60")]
    snippet_len: usize,
}

/// Human-readable reconciliation report
#[derive(Serialize)]
struct ReadableReport {
    before_file: String,
    after_file: String,
    stats: ReconciliationStats,
    block_operations: Vec<ReadableBlockOp>,
}

/// Human-readable block operation
#[derive(Serialize)]
struct ReadableBlockOp {
    /// Position in the result
    result_index: usize,
    /// The action taken
    action: String,
    /// Type of block in result
    block_type: String,
    /// Content snippet from the source block
    #[serde(skip_serializing_if = "Option::is_none")]
    before_content: Option<String>,
    /// Content snippet from the target block
    #[serde(skip_serializing_if = "Option::is_none")]
    after_content: Option<String>,
    /// Nested block operations (for containers)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    nested_block_ops: Vec<ReadableBlockOp>,
    /// Nested inline operations
    #[serde(skip_serializing_if = "Vec::is_empty")]
    inline_ops: Vec<ReadableInlineOp>,
}

/// Human-readable inline operation
#[derive(Serialize)]
struct ReadableInlineOp {
    result_index: usize,
    action: String,
    inline_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    before_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    after_content: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    nested_inline_ops: Vec<ReadableInlineOp>,
}

/// Extract a text snippet from a block
fn block_snippet(block: &Block, max_len: usize) -> String {
    let text = extract_block_text(block);
    truncate_snippet(&text, max_len)
}

/// Extract plain text from a block
fn extract_block_text(block: &Block) -> String {
    match block {
        Block::Plain(p) => extract_inlines_text(&p.content),
        Block::Paragraph(p) => extract_inlines_text(&p.content),
        Block::Header(h) => format!("H{}: {}", h.level, extract_inlines_text(&h.content)),
        Block::CodeBlock(c) => format!("```{}\n{}", c.attr.1.join(","), c.text),
        Block::RawBlock(r) => format!("raw({}): {}", r.format, r.text),
        Block::BlockQuote(_) => "[blockquote]".to_string(),
        Block::OrderedList(l) => format!("[ordered list: {} items]", l.content.len()),
        Block::BulletList(l) => format!("[bullet list: {} items]", l.content.len()),
        Block::DefinitionList(l) => format!("[definition list: {} items]", l.content.len()),
        Block::HorizontalRule(_) => "---".to_string(),
        Block::Table(_) => "[table]".to_string(),
        Block::Figure(_) => "[figure]".to_string(),
        Block::Div(d) => format!("[div .{}]", d.attr.1.first().unwrap_or(&String::new())),
        Block::LineBlock(l) => l
            .content
            .iter()
            .map(|line| extract_inlines_text(line))
            .collect::<Vec<_>>()
            .join("\n"),
        Block::BlockMetadata(_) => "[metadata]".to_string(),
        Block::NoteDefinitionPara(n) => {
            format!("[^{}]: {}", n.id, extract_inlines_text(&n.content))
        }
        Block::NoteDefinitionFencedBlock(n) => format!("[^{}]: [fenced block]", n.id),
        Block::CaptionBlock(_) => "[caption]".to_string(),
        Block::Custom(c) => format!("[custom: {}]", c.type_name),
    }
}

/// Extract plain text from inlines
fn extract_inlines_text(inlines: &Inlines) -> String {
    let mut result = String::new();
    for inline in inlines {
        result.push_str(&extract_inline_text(inline));
    }
    result
}

/// Extract plain text from a single inline
fn extract_inline_text(inline: &Inline) -> String {
    match inline {
        Inline::Str(s) => s.text.clone(),
        Inline::Space(_) => " ".to_string(),
        Inline::SoftBreak(_) => " ".to_string(),
        Inline::LineBreak(_) => "\n".to_string(),
        Inline::Emph(e) => extract_inlines_text(&e.content),
        Inline::Strong(s) => extract_inlines_text(&s.content),
        Inline::Underline(u) => extract_inlines_text(&u.content),
        Inline::Strikeout(s) => extract_inlines_text(&s.content),
        Inline::Superscript(s) => extract_inlines_text(&s.content),
        Inline::Subscript(s) => extract_inlines_text(&s.content),
        Inline::SmallCaps(s) => extract_inlines_text(&s.content),
        Inline::Quoted(q) => format!("\"{}\"", extract_inlines_text(&q.content)),
        Inline::Cite(c) => format!("[cite: {}]", c.citations.len()),
        Inline::Code(c) => format!("`{}`", c.text),
        Inline::Math(m) => format!("${}$", m.text),
        Inline::RawInline(r) => format!("raw({})", r.format),
        Inline::Link(l) => format!("[{}]({})", extract_inlines_text(&l.content), l.target.0),
        Inline::Image(i) => format!("![{}]({})", extract_inlines_text(&i.content), i.target.0),
        Inline::Note(_) => "[note]".to_string(),
        Inline::Span(s) => extract_inlines_text(&s.content),
        Inline::Shortcode(s) => format!("{{{{< {} >}}}}", s.name),
        Inline::NoteReference(n) => format!("[^{}]", n.id),
        Inline::Attr(_, _) => "".to_string(),
        Inline::Insert(i) => format!("++{}++", extract_inlines_text(&i.content)),
        Inline::Delete(d) => format!("~~{}~~", extract_inlines_text(&d.content)),
        Inline::Highlight(h) => format!("=={}==", extract_inlines_text(&h.content)),
        Inline::EditComment(c) => format!(">>{}<<", extract_inlines_text(&c.content)),
        Inline::Custom(c) => format!("[custom: {}]", c.type_name),
    }
}

/// Get the type name of a block
fn block_type_name(block: &Block) -> &'static str {
    match block {
        Block::Plain(_) => "Plain",
        Block::Paragraph(_) => "Paragraph",
        Block::LineBlock(_) => "LineBlock",
        Block::CodeBlock(_) => "CodeBlock",
        Block::RawBlock(_) => "RawBlock",
        Block::BlockQuote(_) => "BlockQuote",
        Block::OrderedList(_) => "OrderedList",
        Block::BulletList(_) => "BulletList",
        Block::DefinitionList(_) => "DefinitionList",
        Block::Header(_) => "Header",
        Block::HorizontalRule(_) => "HorizontalRule",
        Block::Table(_) => "Table",
        Block::Figure(_) => "Figure",
        Block::Div(_) => "Div",
        Block::BlockMetadata(_) => "BlockMetadata",
        Block::NoteDefinitionPara(_) => "NoteDefinitionPara",
        Block::NoteDefinitionFencedBlock(_) => "NoteDefinitionFencedBlock",
        Block::CaptionBlock(_) => "CaptionBlock",
        Block::Custom(_) => "Custom",
    }
}

/// Get the type name of an inline
fn inline_type_name(inline: &Inline) -> &'static str {
    match inline {
        Inline::Str(_) => "Str",
        Inline::Emph(_) => "Emph",
        Inline::Underline(_) => "Underline",
        Inline::Strong(_) => "Strong",
        Inline::Strikeout(_) => "Strikeout",
        Inline::Superscript(_) => "Superscript",
        Inline::Subscript(_) => "Subscript",
        Inline::SmallCaps(_) => "SmallCaps",
        Inline::Quoted(_) => "Quoted",
        Inline::Cite(_) => "Cite",
        Inline::Code(_) => "Code",
        Inline::Space(_) => "Space",
        Inline::SoftBreak(_) => "SoftBreak",
        Inline::LineBreak(_) => "LineBreak",
        Inline::Math(_) => "Math",
        Inline::RawInline(_) => "RawInline",
        Inline::Link(_) => "Link",
        Inline::Image(_) => "Image",
        Inline::Note(_) => "Note",
        Inline::Span(_) => "Span",
        Inline::Shortcode(_) => "Shortcode",
        Inline::NoteReference(_) => "NoteReference",
        Inline::Attr(_, _) => "Attr",
        Inline::Insert(_) => "Insert",
        Inline::Delete(_) => "Delete",
        Inline::Highlight(_) => "Highlight",
        Inline::EditComment(_) => "EditComment",
        Inline::Custom(_) => "Custom",
    }
}

/// Truncate a string to max_len, adding "..." if truncated
fn truncate_snippet(s: &str, max_len: usize) -> String {
    let s = s.replace('\n', "\\n");
    if s.len() <= max_len {
        s
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}

/// Get inline snippet
fn inline_snippet(inline: &Inline, max_len: usize) -> String {
    truncate_snippet(&extract_inline_text(inline), max_len)
}

/// Build readable inline operations from a plan
fn build_inline_ops(
    plan: &InlineReconciliationPlan,
    before_inlines: &Inlines,
    after_inlines: &Inlines,
    snippet_len: usize,
) -> Vec<ReadableInlineOp> {
    let mut ops = Vec::new();

    for (result_idx, alignment) in plan.inline_alignments.iter().enumerate() {
        let (action, before_idx, after_idx) = match alignment {
            InlineAlignment::KeepBefore(idx) => ("keep_before", Some(*idx), None),
            InlineAlignment::UseAfter(idx) => ("use_after", None, Some(*idx)),
            InlineAlignment::RecurseIntoContainer {
                before_idx,
                after_idx,
            } => ("recurse", Some(*before_idx), Some(*after_idx)),
        };

        let before_inline = before_idx.and_then(|i| before_inlines.get(i));
        let after_inline = after_idx.and_then(|i| after_inlines.get(i));

        let inline_type = before_inline
            .or(after_inline)
            .map(inline_type_name)
            .unwrap_or("Unknown");

        let before_content = before_inline.map(|i| inline_snippet(i, snippet_len));
        let after_content = after_inline.map(|i| inline_snippet(i, snippet_len));

        // Handle nested inline plans for containers
        let nested_inline_ops =
            if let Some(nested_plan) = plan.inline_container_plans.get(&result_idx) {
                let nested_before = before_inline.map(get_inline_children).unwrap_or_default();
                let nested_after = after_inline.map(get_inline_children).unwrap_or_default();
                build_inline_ops(nested_plan, &nested_before, &nested_after, snippet_len)
            } else {
                Vec::new()
            };

        ops.push(ReadableInlineOp {
            result_index: result_idx,
            action: action.to_string(),
            inline_type: inline_type.to_string(),
            before_content,
            after_content,
            nested_inline_ops,
        });
    }

    ops
}

/// Get children of an inline container
fn get_inline_children(inline: &Inline) -> Inlines {
    match inline {
        Inline::Emph(e) => e.content.clone(),
        Inline::Strong(s) => s.content.clone(),
        Inline::Underline(u) => u.content.clone(),
        Inline::Strikeout(s) => s.content.clone(),
        Inline::Superscript(s) => s.content.clone(),
        Inline::Subscript(s) => s.content.clone(),
        Inline::SmallCaps(s) => s.content.clone(),
        Inline::Quoted(q) => q.content.clone(),
        Inline::Link(l) => l.content.clone(),
        Inline::Image(i) => i.content.clone(),
        Inline::Span(s) => s.content.clone(),
        Inline::Insert(i) => i.content.clone(),
        Inline::Delete(d) => d.content.clone(),
        Inline::Highlight(h) => h.content.clone(),
        Inline::EditComment(c) => c.content.clone(),
        _ => Vec::new(),
    }
}

/// Get inline content of a block (for Paragraph, Plain, Header, etc.)
fn get_block_inlines(block: &Block) -> Option<&Inlines> {
    match block {
        Block::Plain(p) => Some(&p.content),
        Block::Paragraph(p) => Some(&p.content),
        Block::Header(h) => Some(&h.content),
        _ => None,
    }
}

/// Get children of a container block
fn get_block_children(block: &Block) -> Blocks {
    match block {
        Block::BlockQuote(b) => b.content.clone(),
        Block::Div(d) => d.content.clone(),
        Block::Figure(f) => f.content.clone(),
        _ => Vec::new(),
    }
}

/// Build readable block operations from a plan
fn build_block_ops(
    plan: &ReconciliationPlan,
    before_blocks: &Blocks,
    after_blocks: &Blocks,
    snippet_len: usize,
) -> Vec<ReadableBlockOp> {
    let mut ops = Vec::new();

    for (result_idx, alignment) in plan.block_alignments.iter().enumerate() {
        let (action, before_idx, after_idx) = match alignment {
            BlockAlignment::KeepBefore(idx) => ("keep_before", Some(*idx), None),
            BlockAlignment::UseAfter(idx) => ("use_after", None, Some(*idx)),
            BlockAlignment::RecurseIntoContainer {
                before_idx,
                after_idx,
            } => ("recurse", Some(*before_idx), Some(*after_idx)),
        };

        let before_block = before_idx.and_then(|i| before_blocks.get(i));
        let after_block = after_idx.and_then(|i| after_blocks.get(i));

        let block_type = before_block
            .or(after_block)
            .map(block_type_name)
            .unwrap_or("Unknown");

        let before_content = before_block.map(|b| block_snippet(b, snippet_len));
        let after_content = after_block.map(|b| block_snippet(b, snippet_len));

        // Handle nested block plans for containers
        let nested_block_ops =
            if let Some(nested_plan) = plan.block_container_plans.get(&result_idx) {
                let nested_before = before_block.map(get_block_children).unwrap_or_default();
                let nested_after = after_block.map(get_block_children).unwrap_or_default();
                build_block_ops(nested_plan, &nested_before, &nested_after, snippet_len)
            } else {
                Vec::new()
            };

        // Handle inline plans
        let inline_ops = if let Some(inline_plan) = plan.inline_plans.get(&result_idx) {
            let before_inlines = before_block
                .and_then(get_block_inlines)
                .cloned()
                .unwrap_or_default();
            let after_inlines = after_block
                .and_then(get_block_inlines)
                .cloned()
                .unwrap_or_default();
            build_inline_ops(inline_plan, &before_inlines, &after_inlines, snippet_len)
        } else {
            Vec::new()
        };

        ops.push(ReadableBlockOp {
            result_index: result_idx,
            action: action.to_string(),
            block_type: block_type.to_string(),
            before_content,
            after_content,
            nested_block_ops,
            inline_ops,
        });
    }

    ops
}

fn main() {
    let args = Args::parse();

    // Read before file
    let before_content = std::fs::read_to_string(&args.before).unwrap_or_else(|e| {
        eprintln!("Error reading before file '{}': {}", args.before, e);
        std::process::exit(1);
    });

    // Read after file
    let after_content = std::fs::read_to_string(&args.after).unwrap_or_else(|e| {
        eprintln!("Error reading after file '{}': {}", args.after, e);
        std::process::exit(1);
    });

    // Ensure files end with newline
    let before_content = if before_content.ends_with('\n') {
        before_content
    } else {
        format!("{}\n", before_content)
    };
    let after_content = if after_content.ends_with('\n') {
        after_content
    } else {
        format!("{}\n", after_content)
    };

    // Parse before file
    let mut sink = io::sink();
    let (before_ast, _, _) = match readers::qmd::read(
        before_content.as_bytes(),
        false,
        &args.before,
        &mut sink,
        true,
        None,
    ) {
        Ok(result) => result,
        Err(diagnostics) => {
            eprintln!("Error parsing before file '{}':", args.before);
            for diag in diagnostics {
                eprintln!("  {}", diag.to_text(None));
            }
            std::process::exit(1);
        }
    };

    // Parse after file
    let (after_ast, _, _) = match readers::qmd::read(
        after_content.as_bytes(),
        false,
        &args.after,
        &mut sink,
        true,
        None,
    ) {
        Ok(result) => result,
        Err(diagnostics) => {
            eprintln!("Error parsing after file '{}':", args.after);
            for diag in diagnostics {
                eprintln!("  {}", diag.to_text(None));
            }
            std::process::exit(1);
        }
    };

    // Compute reconciliation plan
    let plan = compute_reconciliation(&before_ast, &after_ast);

    // Build readable report
    let block_operations = build_block_ops(
        &plan,
        &before_ast.blocks,
        &after_ast.blocks,
        args.snippet_len,
    );

    let report = ReadableReport {
        before_file: args.before,
        after_file: args.after,
        stats: plan.stats,
        block_operations,
    };

    // Output pretty JSON
    match serde_json::to_string_pretty(&report) {
        Ok(s) => println!("{}", s),
        Err(e) => {
            eprintln!("Error serializing to JSON: {}", e);
            std::process::exit(1);
        }
    }
}
