//! Aligning the pre-sugaring snapshot with the scanned units (epic decision 36).
//!
//! Crossref sugaring strips the label and caption lines from the engine-visible text, so the
//! stage cannot hash what it scans. `pre_engine_sugaring` (EXR-5) records, before the pass
//! rewrites anything, one [`SugaredBlock`] for **every** braced executable block, in document
//! order: the author's text and the text the engine will see. [`author_texts`] pairs each
//! scanned unit with its entry. This module is pure and holds no pipeline code.

use crate::engine::units::Unit;

use super::ids::{content_hash, scanned_author_text, span_content_hash};
use super::types::{AuthorText, AuthorTexts};

/// One braced block as the sugaring pass saw it, in document order.
///
/// The snapshot must include blocks the pass leaves alone (where the two texts are equal).
/// If it listed only rewritten blocks, an untouched cell with the same text as a rewritten
/// cell's stripped text would take that cell's entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SugaredBlock {
    /// The lines between the fences exactly as the author wrote them, `#|` lines included.
    pub author_text: String,
    /// The text after sugaring, which is what the engine sees and the scanner scans.
    pub sugared_text: String,
}

/// Build the stage's [`AuthorTexts`]: one entry per unit, index-for-index.
///
/// A cell takes the next snapshot entry (in document order) whose sugared text equals the
/// unit's scanned text, so duplicate identical cells align by order and a block the scanner
/// does not report (a raw block, say) is skipped. A cell with no matching entry falls back to
/// its scanned text, which is exactly right for a block sugaring did not touch. A span has no
/// label or caption lines and always uses its expression.
pub fn author_texts(units: &[Unit], snapshot: &[SugaredBlock]) -> AuthorTexts {
    let mut cursor = 0;
    AuthorTexts(
        units
            .iter()
            .map(|unit| match unit {
                Unit::Inline(i) => AuthorText {
                    text: i.code.clone(),
                    content_hash: span_content_hash(&i.lang, &i.code),
                },
                Unit::Cell(_) => {
                    let scanned = scanned_author_text(unit);
                    let found = snapshot[cursor..]
                        .iter()
                        .position(|b| b.sugared_text == scanned);
                    let text = match found {
                        Some(offset) => {
                            let block = &snapshot[cursor + offset];
                            cursor += offset + 1;
                            block.author_text.clone()
                        }
                        None => scanned,
                    };
                    AuthorText {
                        content_hash: content_hash(&text),
                        text,
                    }
                }
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crossref::RefTypeRegistry;
    use crate::crossref::codeblock_shorthand::desugar_blocks;
    use crate::engine::units::scan_units;
    use crate::record::{author_label, fill_ids};
    use quarto_pandoc_types::block::Block;

    fn cell(text: &str) -> Unit {
        scan_units(&format!("```{{python}}\n{text}\n```\n")).remove(0)
    }

    fn block(author: &str, sugared: &str) -> SugaredBlock {
        SugaredBlock {
            author_text: author.into(),
            sugared_text: sugared.into(),
        }
    }

    #[test]
    fn an_untouched_cell_uses_its_scanned_text_with_no_snapshot() {
        let units = [cell("x <- 1")];
        let texts = author_texts(&units, &[]);
        assert_eq!(texts.0[0].text, "x <- 1");
        assert_eq!(texts.0[0].content_hash, "4390f67858f3f1c7");
    }

    #[test]
    fn a_sugared_cell_hashes_the_authors_raw_lines_label_and_caption_included() {
        let author =
            "#| label: fig-plot\n#| fig-width: 7\n#| fig-cap: A caption\n#| fig-height: 5\nx = 1";
        let sugared = "#| fig-width: 7\n#| fig-height: 5\nx = 1";
        let texts = author_texts(&[cell(sugared)], &[block(author, sugared)]);
        assert_eq!(texts.0[0].text, author);
        assert_eq!(texts.0[0].content_hash, content_hash(author));
        assert_ne!(texts.0[0].content_hash, content_hash(sugared));
    }

    /// Run the real sugaring pass and align: the label and `fig-cap` lines come back.
    #[test]
    fn real_sugaring_output_aligns_with_a_snapshot_of_the_original() {
        let author =
            "#| label: fig-plot\n#| fig-width: 7\n#| fig-cap: A caption\n#| fig-height: 5\nx = 1";
        let doc = format!("Intro\n\n```{{python}}\n{author}\n```\n");
        let (mut ast, ctx, _) = pampa::readers::qmd::read(
            doc.as_bytes(),
            false,
            "t.qmd",
            &mut std::io::sink(),
            false,
            None,
        )
        .unwrap();
        let Block::CodeBlock(before) = &ast.blocks[1] else {
            panic!()
        };
        assert_eq!(
            before.text, author,
            "precondition: the parser keeps the raw lines"
        );
        let original = before.text.clone();
        desugar_blocks(
            &mut ast.blocks,
            &RefTypeRegistry::builtin(),
            &ctx.source_context,
            &mut Vec::new(),
        );
        let Block::Div(div) = &ast.blocks[1] else {
            panic!("expected the float Div")
        };
        let Block::CodeBlock(after) = &div.content[0] else {
            panic!()
        };
        assert_ne!(
            after.text, original,
            "precondition: sugaring stripped lines"
        );

        let (buf, _) = pampa::writers::qmd::write_with_source_info(&ast).unwrap();
        let mut units = scan_units(&String::from_utf8(buf).unwrap());
        let cells: Vec<_> = units
            .iter()
            .filter(|u| matches!(u, Unit::Cell(_)))
            .collect();
        assert_eq!(cells.len(), 1);
        let snapshot = [block(&original, &after.text)];
        let texts = author_texts(&units, &snapshot);
        assert_eq!(texts.0[0].text, original);
        assert_eq!(texts.0[0].content_hash, content_hash(&original));
        fill_ids(&mut units, &texts).unwrap();
        let Unit::Cell(c) = &units[0] else { panic!() };
        assert_eq!(
            c.id.as_ref().unwrap().0,
            format!("0-{}-fig-plot", &content_hash(&original)[..8]),
            "the label segment comes from the author's own label line"
        );
    }

    #[test]
    fn a_non_crossref_label_stays_in_the_text() {
        let text = "#| label: my-chunk\nx = 1";
        let texts = author_texts(&[cell(text)], &[block(text, text)]);
        assert_eq!(texts.0[0].text, text);
        assert_eq!(
            author_label(&texts.0[0].text, "python").as_deref(),
            Some("my-chunk")
        );
    }

    /// An untouched cell with the same text as a rewritten cell's stripped text must not take
    /// that cell's entry: the snapshot lists the untouched block too, in order.
    #[test]
    fn identical_untouched_and_sugared_cells_align_by_order() {
        let plain = "x = 1";
        let author = "#| label: fig-a\n#| fig-cap: Cap\nx = 1";
        let units = [cell(plain), cell(plain)];
        let snapshot = [block(plain, plain), block(author, plain)];
        let texts = author_texts(&units, &snapshot);
        assert_eq!(texts.0[0].text, plain);
        assert_eq!(texts.0[1].text, author);
    }

    #[test]
    fn a_snapshot_entry_the_scanner_does_not_report_is_skipped() {
        let units = [cell("y = 2")];
        let snapshot = [block("raw", "raw"), block("y = 2", "y = 2")];
        assert_eq!(author_texts(&units, &snapshot).0[0].text, "y = 2");
    }

    #[test]
    fn spans_use_their_expression_and_the_domain_separated_hash() {
        let units = scan_units("See `{python} 1+1`.\n");
        let texts = author_texts(&units, &[]);
        assert_eq!(texts.0[0].text, "1+1");
        assert_eq!(texts.0[0].content_hash, span_content_hash("python", "1+1"));
    }

    #[test]
    fn the_mapping_is_total() {
        let units = scan_units("```{python}\na\n```\n\n`{r} 1`\n\n```{r}\nb\n```\n");
        assert_eq!(author_texts(&units, &[]).len(), units.len());
    }
}
