//! AST walker that annotates [`CodeBlock`] and inline [`Code`] nodes
//! with `data-hl-spans` attribute values.
//!
//! Rules (per the design plan, resolved decision 1):
//!
//! - For each `CodeBlock` and `Code`, take the **first class** and resolve
//!   it against the language registry (user grammars first if provided,
//!   then built-ins).
//! - If the class resolves and no `data-hl-spans` key is already present
//!   on the node's attribute KV map, compute the highlight and insert it.
//! - If the class doesn't resolve, or `data-hl-spans` is already set
//!   (filter-authored), leave the node alone. "Filter wins" is deliberate:
//!   a user can override the built-in highlighter by producing the
//!   attribute themselves in an earlier filter stage.
//!
//! The walker recurses through all block/inline container types that
//! may contain `CodeBlock`s or `Code` spans.
//!
//! This walker is target-agnostic: the same code runs on native and
//! wasm32. User grammars are supplied through the
//! [`UserGrammarProvider`](crate::UserGrammarProvider) trait, whose
//! concrete implementation differs by target (wasmtime on native, JS
//! interop in the browser) but whose interface is identical.

use quarto_pandoc_types::pandoc::Pandoc;
use quarto_pandoc_types::{Attr, Block, Blocks, Code, CodeBlock, Inline, Inlines, Slot};

use crate::SPANS_ATTR_KEY;
use crate::error::HighlightError;
use crate::provider::UserGrammarProvider;
use crate::registry::Registry;

/// Walk `ast` and annotate every `CodeBlock` / inline `Code` whose first
/// class resolves to a registered grammar. Mutates nodes in place.
///
/// User grammars (when `user` is `Some`) are consulted first; a class
/// present in both a user grammar and the built-in registry is handled
/// by the user grammar, not the built-in.
pub fn annotate_pandoc(
    ast: &mut Pandoc,
    user: Option<&mut dyn UserGrammarProvider>,
) -> Result<(), HighlightError> {
    // Forward to the generic impl with `P = dyn UserGrammarProvider`.
    // Keeping the public signature non-generic lets callers write
    // `annotate_pandoc(&mut doc, None)` without turbofish; the internal
    // generic `Walker` avoids the lifetime-variance pitfalls that arise
    // from `&mut dyn Trait` directly in a struct field (manual reborrows
    // fight `&mut T`'s invariance; with a generic `P: ?Sized` there is
    // only one lifetime to shorten and `as_deref_mut` works cleanly).
    annotate_pandoc_generic(ast, user)
}

fn annotate_pandoc_generic<P>(ast: &mut Pandoc, user: Option<&mut P>) -> Result<(), HighlightError>
where
    P: UserGrammarProvider + ?Sized,
{
    let mut walker = Walker { user };
    walker.visit_blocks(&mut ast.blocks)
}

struct Walker<'a, P: ?Sized> {
    user: Option<&'a mut P>,
}

impl<'a, P> Walker<'a, P>
where
    P: UserGrammarProvider + ?Sized,
{
    fn visit_blocks(&mut self, blocks: &mut Blocks) -> Result<(), HighlightError> {
        for block in blocks.iter_mut() {
            self.visit_block(block)?;
        }
        Ok(())
    }

    fn visit_block(&mut self, block: &mut Block) -> Result<(), HighlightError> {
        match block {
            Block::CodeBlock(cb) => self.annotate_codeblock(cb),
            Block::Paragraph(p) => self.visit_inlines(&mut p.content),
            Block::Plain(p) => self.visit_inlines(&mut p.content),
            Block::Header(h) => self.visit_inlines(&mut h.content),
            Block::BlockQuote(bq) => self.visit_blocks(&mut bq.content),
            Block::Div(div) => self.visit_blocks(&mut div.content),
            Block::OrderedList(ol) => {
                for item in ol.content.iter_mut() {
                    self.visit_blocks(item)?;
                }
                Ok(())
            }
            Block::BulletList(bl) => {
                for item in bl.content.iter_mut() {
                    self.visit_blocks(item)?;
                }
                Ok(())
            }
            Block::DefinitionList(dl) => {
                for (term, defs) in dl.content.iter_mut() {
                    self.visit_inlines(term)?;
                    for def in defs.iter_mut() {
                        self.visit_blocks(def)?;
                    }
                }
                Ok(())
            }
            Block::LineBlock(lb) => {
                for line in lb.content.iter_mut() {
                    self.visit_inlines(line)?;
                }
                Ok(())
            }
            Block::Figure(fig) => {
                self.visit_blocks(&mut fig.content)?;
                if let Some(long) = fig.caption.long.as_mut() {
                    self.visit_blocks(long)?;
                }
                if let Some(short) = fig.caption.short.as_mut() {
                    self.visit_inlines(short)?;
                }
                Ok(())
            }
            Block::Table(_) => {
                // Tables may contain code in cells. Leave recursion into
                // table cells to a follow-up — table internals are less
                // commonly used for highlighted code blocks and their AST
                // shape adds walker complexity we don't need for v1.
                Ok(())
            }
            Block::Custom(node) => {
                for (_k, slot) in node.slots.iter_mut() {
                    self.visit_slot(slot)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn visit_slot(&mut self, slot: &mut Slot) -> Result<(), HighlightError> {
        match slot {
            Slot::Block(b) => self.visit_block(b),
            Slot::Blocks(bs) => self.visit_blocks(bs),
            Slot::Inline(i) => self.visit_inline(i),
            Slot::Inlines(is) => self.visit_inlines(is),
        }
    }

    fn visit_inlines(&mut self, inlines: &mut Inlines) -> Result<(), HighlightError> {
        for inline in inlines.iter_mut() {
            self.visit_inline(inline)?;
        }
        Ok(())
    }

    fn visit_inline(&mut self, inline: &mut Inline) -> Result<(), HighlightError> {
        match inline {
            Inline::Code(c) => self.annotate_inline_code(c),
            Inline::Emph(e) => self.visit_inlines(&mut e.content),
            Inline::Underline(u) => self.visit_inlines(&mut u.content),
            Inline::Strong(s) => self.visit_inlines(&mut s.content),
            Inline::Strikeout(s) => self.visit_inlines(&mut s.content),
            Inline::Superscript(s) => self.visit_inlines(&mut s.content),
            Inline::Subscript(s) => self.visit_inlines(&mut s.content),
            Inline::SmallCaps(s) => self.visit_inlines(&mut s.content),
            Inline::Quoted(q) => self.visit_inlines(&mut q.content),
            Inline::Link(l) => self.visit_inlines(&mut l.content),
            Inline::Image(i) => self.visit_inlines(&mut i.content),
            Inline::Note(n) => self.visit_blocks(&mut n.content),
            Inline::Span(s) => self.visit_inlines(&mut s.content),
            Inline::Insert(i) => self.visit_inlines(&mut i.content),
            Inline::Delete(d) => self.visit_inlines(&mut d.content),
            Inline::Highlight(h) => self.visit_inlines(&mut h.content),
            Inline::Custom(node) => {
                for (_k, slot) in node.slots.iter_mut() {
                    self.visit_slot(slot)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn annotate_codeblock(&mut self, cb: &mut CodeBlock) -> Result<(), HighlightError> {
        annotate_attr(&mut cb.attr, &cb.text, self.user.as_deref_mut())
    }

    fn annotate_inline_code(&mut self, c: &mut Code) -> Result<(), HighlightError> {
        annotate_attr(&mut c.attr, &c.text, self.user.as_deref_mut())
    }
}

fn annotate_attr<P>(attr: &mut Attr, text: &str, user: Option<&mut P>) -> Result<(), HighlightError>
where
    P: UserGrammarProvider + ?Sized,
{
    // Rule: filter-authored annotations win. Skip if already present.
    if attr.2.contains_key(SPANS_ATTR_KEY) {
        return Ok(());
    }

    // Rule: first class wins. Pick the first class that resolves to any
    // registered grammar (user-grammar first, then built-in).
    //
    // We immutably reborrow `user` for class resolution so we can later
    // take it `&mut` for the highlight call. The returned `&str` is
    // borrowed from `attr.1`, not from `user`, so we can drop the
    // immutable reborrow of `user` before the mutable one.
    let class = match pick_first_resolvable_class(&attr.1, user.as_deref()) {
        Some(c) => c.to_string(),
        None => return Ok(()),
    };

    let encoded = match user {
        Some(u) if u.contains(&class) => u.highlight(&class, text)?,
        _ => Registry::global().highlight(&class, text)?,
    };

    if let Some(encoded) = encoded {
        attr.2.insert(SPANS_ATTR_KEY.to_string(), encoded);
    }
    Ok(())
}

fn pick_first_resolvable_class<'a, P>(classes: &'a [String], user: Option<&P>) -> Option<&'a str>
where
    P: UserGrammarProvider + ?Sized,
{
    classes.iter().map(|s| s.as_str()).find(|class| {
        if let Some(u) = user {
            if u.contains(class) {
                return true;
            }
        }
        Registry::global().resolve(class).is_some()
    })
}
