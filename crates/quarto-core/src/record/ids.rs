//! Positron's content hash and the unit ids (EXR-4 Phase 2; epic decisions 13, 38 and 39).
//!
//! `quarto_content_hash` is for **matching** and follows Positron exactly: the block content
//! is split on `\r?\n` and rejoined with `\n` (so a CRLF document and its LF twin hash alike),
//! then SHA-1 over UTF-8, hex digest truncated to 16 characters
//! (`quartoDocumentModel.ts:22`, `quartoParser.ts:113,173`, `hash.ts:190-233`). A Rust `&str`
//! cannot hold a lone surrogate, so Positron's U+FFFD mapping needs no code here.

use sha1::{Digest, Sha1};

use crate::cell_options::comment_syntax_for;
use crate::engine::units::{CellUnit, Unit, UnitId};

use super::types::{AuthorText, AuthorTexts};

/// Normalize line endings the way Positron's parser does: split on `\r?\n`, rejoin with `\n`.
fn normalize_lines(text: &str) -> String {
    text.split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Positron's `quarto_content_hash` of a block's content (the lines strictly between the
/// fences, option lines included): 16 lowercase hex characters.
pub fn content_hash(block_content: &str) -> String {
    let digest = Sha1::digest(normalize_lines(block_content).as_bytes());
    hex::encode(digest)[..16].to_string()
}

/// The `quarto_content_hash` of an inline span. Domain-separated (the hash input is
/// `inline:<lang>\n<code>`, with `<lang>` lowercased) so Positron can never match a span to a
/// real cell with identical text, and `{Python}` and `{python}` hash alike.
pub fn span_content_hash(lang: &str, code: &str) -> String {
    content_hash(&format!("inline:{}\n{code}", lang.to_ascii_lowercase()))
}

/// A cell id in Positron's format, `{index}-{hash8}-{label|unlabeled}`. `index` is the
/// document-wide cell index, `hash` a 16-hex content hash.
pub fn cell_id(index: usize, content_hash: &str, label: Option<&str>) -> String {
    format!(
        "{index}-{}-{}",
        &content_hash[..8.min(content_hash.len())],
        label.filter(|l| !l.is_empty()).unwrap_or("unlabeled")
    )
}

/// A span id, `{n}-{hash8}-inline`, where `n` counts every span the scanner claims, owned or
/// not, so cell indices are the same whatever spans surround them.
pub fn span_id(n: usize, content_hash: &str) -> String {
    format!("{n}-{}-inline", &content_hash[..8.min(content_hash.len())])
}

/// The label of a cell: the value of the first `label:` option in the leading run of option
/// lines of its author text (in the language's own option-comment syntax), or `None`.
pub fn author_label(author_text: &str, lang: &str) -> Option<String> {
    let syntax = comment_syntax_for(lang);
    for line in author_text.lines() {
        let rest = line.strip_prefix(syntax.prefix)?;
        let rest = rest.trim_start_matches([' ', '\t']).strip_prefix('|')?;
        let content = match syntax.suffix {
            Some(suffix) => rest.trim_end().strip_suffix(suffix).unwrap_or(rest),
            None => rest,
        };
        if let Some((key, value)) = content.split_once(':')
            && key.trim() == "label"
        {
            let value = value.trim();
            let value = value
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
                .unwrap_or(value);
            return Some(value.to_string());
        }
    }
    None
}

/// The author text of a unit that crossref sugaring did not touch: its scanned text (the
/// lines between the fences with container prefixes and indentation removed, one trailing
/// newline dropped, line endings normalized); for a span, the expression.
pub fn scanned_author_text(unit: &Unit) -> String {
    match unit {
        Unit::Inline(i) => i.code.clone(),
        Unit::Cell(c) => {
            let stripped = c
                .code
                .split('\n')
                .map(|line| strip_cell_prefix(c, line))
                .collect::<Vec<_>>()
                .join("\n");
            let stripped = stripped.strip_suffix('\n').unwrap_or(&stripped);
            normalize_lines(stripped)
        }
    }
}

fn strip_cell_prefix<'a>(cell: &CellUnit, line: &'a str) -> &'a str {
    let line = line
        .strip_prefix(cell.container_prefix.as_str())
        .unwrap_or(line);
    let spaces = line
        .bytes()
        .take(cell.indent)
        .take_while(|b| *b == b' ')
        .count();
    &line[spaces..]
}

impl AuthorTexts {
    /// Author texts for units no crossref sugaring touched: the scanned text of each.
    pub fn from_scanned(units: &[Unit]) -> Self {
        Self(
            units
                .iter()
                .map(|u| {
                    let text = scanned_author_text(u);
                    let content_hash = match u {
                        Unit::Inline(i) => span_content_hash(&i.lang, &text),
                        Unit::Cell(_) => content_hash(&text),
                    };
                    AuthorText { text, content_hash }
                })
                .collect(),
        )
    }
}

/// The unit list and the author-text mapping have different lengths.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("cannot fill unit ids: {units} units but {author_texts} author texts")]
pub struct FillIdsError {
    pub units: usize,
    pub author_texts: usize,
}

/// Fill every unit's typed id slot (EXR-2's `Unit.id`), the one place ids are computed.
///
/// Cells get `{index}-{hash8}-{label|unlabeled}` from the document-wide cell index and the
/// author text's content hash; spans get `{n}-{hash8}-inline` from the span counter. The
/// mapping must be total (one entry per unit): a length mismatch is an error naming both
/// lengths and leaves every slot untouched, never a partly filled list or a guess.
/// Idempotent: filling a filled list again changes nothing.
pub fn fill_ids(units: &mut [Unit], author_texts: &AuthorTexts) -> Result<(), FillIdsError> {
    if units.len() != author_texts.len() {
        return Err(FillIdsError {
            units: units.len(),
            author_texts: author_texts.len(),
        });
    }
    for (unit, author) in units.iter_mut().zip(&author_texts.0) {
        match unit {
            Unit::Cell(c) => {
                let label = author_label(&author.text, &c.lang);
                c.id = Some(UnitId(cell_id(
                    c.index,
                    &author.content_hash,
                    label.as_deref(),
                )));
            }
            Unit::Inline(i) => {
                i.id = Some(UnitId(span_id(i.span_index, &author.content_hash)));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::units::scan_units;

    fn filled(doc: &str) -> Vec<Unit> {
        let mut units = scan_units(doc);
        let texts = AuthorTexts::from_scanned(&units);
        fill_ids(&mut units, &texts).unwrap();
        units
    }

    fn ids(units: &[Unit]) -> Vec<String> {
        units
            .iter()
            .map(|u| match u {
                Unit::Cell(c) => c.id.clone().unwrap().0,
                Unit::Inline(i) => i.id.clone().unwrap().0,
            })
            .collect()
    }

    #[derive(serde::Deserialize)]
    struct Vector {
        name: String,
        input: String,
        expected: String,
    }

    /// Every Phase 0 vector, computed with node `crypto` (see
    /// `scripts/exr-record/content_hash_vectors.mjs`): the four plan values, the empty cell,
    /// a lone surrogate (stored as U+FFFD), CRLF and a trailing newline.
    #[test]
    fn content_hash_matches_every_phase_0_vector() {
        let vectors: Vec<Vector> = serde_json::from_str(include_str!(
            "../../tests/fixtures/records/content-hash-vectors.json"
        ))
        .unwrap();
        assert!(vectors.len() >= 8);
        for v in vectors {
            assert_eq!(content_hash(&v.input), v.expected, "vector `{}`", v.name);
        }
    }

    #[test]
    fn a_crlf_block_hashes_like_its_lf_twin() {
        assert_eq!(
            content_hash("#| label: a\r\nx <- 1\r\ny <- 2"),
            content_hash("#| label: a\nx <- 1\ny <- 2")
        );
        // A lone `\r` is not a line ending in Positron's split, so it is kept.
        assert_ne!(content_hash("a\rb"), content_hash("a\nb"));
    }

    #[test]
    fn id_formats() {
        assert_eq!(cell_id(0, "4390f67858f3f1c7", None), "0-4390f678-unlabeled");
        assert_eq!(
            cell_id(3, "4390f67858f3f1c7", Some("fig-x")),
            "3-4390f678-fig-x"
        );
        assert_eq!(
            cell_id(3, "4390f67858f3f1c7", Some("")),
            "3-4390f678-unlabeled"
        );
        assert_eq!(span_id(7, "4390f67858f3f1c7"), "7-4390f678-inline");
    }

    #[test]
    fn the_label_is_the_first_label_option_in_the_leading_option_run() {
        let l = |t: &str| author_label(t, "python");
        assert_eq!(l("#| label: a\n#| label: b\nx = 1"), Some("a".into()));
        assert_eq!(
            l("#| fig-width: 7\n#| label: fig-a\nx = 1"),
            Some("fig-a".into())
        );
        assert_eq!(l("#| label: \"quoted\"\nx = 1"), Some("quoted".into()));
        assert_eq!(
            l("x = 1\n#| label: late"),
            None,
            "not an option once code starts"
        );
        assert_eq!(l("x = 1"), None);
        assert_eq!(
            author_label("%%| label: d\nflowchart LR", "mermaid"),
            Some("d".into())
        );
        assert_eq!(author_label("#| label: d\nflowchart LR", "mermaid"), None);
    }

    #[test]
    fn fill_ids_names_both_lengths_and_fills_nothing_on_a_mismatch() {
        let mut units = scan_units("```{python}\nx = 1\n```\n\n```{python}\ny = 2\n```\n");
        assert_eq!(units.len(), 2);
        let short = AuthorTexts::from_scanned(&units[..1]);
        let err = fill_ids(&mut units, &short).unwrap_err();
        assert_eq!(
            err,
            FillIdsError {
                units: 2,
                author_texts: 1
            }
        );
        let msg = err.to_string();
        assert!(msg.contains('2') && msg.contains('1'), "{msg}");
        assert!(
            units
                .iter()
                .all(|u| matches!(u, Unit::Cell(c) if c.id.is_none()))
        );
    }

    #[test]
    fn filling_is_idempotent() {
        let mut units = scan_units("```{python}\n#| label: a\nx = 1\n```\n\nSee `{python} 1+1`.\n");
        let texts = AuthorTexts::from_scanned(&units);
        fill_ids(&mut units, &texts).unwrap();
        let first = units.clone();
        fill_ids(&mut units, &texts).unwrap();
        assert_eq!(units, first);
        assert_eq!(
            ids(&units)[0],
            format!("0-{}-a", &content_hash("#| label: a\nx = 1")[..8])
        );
    }

    #[test]
    fn a_span_inserted_before_a_cell_leaves_the_cell_id_unchanged() {
        let cell = "```{python}\nx = 1\n```\n";
        let before = ids(&filled(cell));
        let after = ids(&filled(&format!("Sum is `{{python}} 1+1` here.\n\n{cell}")));
        assert_eq!(after.len(), 2);
        assert_eq!(after[1], before[0]);
        // And removing it again restores the span numbering of what follows.
        let with_two = ids(&filled(&format!(
            "`{{python}} 1`\n\n`{{python}} 2`\n\n{cell}"
        )));
        assert_eq!(with_two[2], before[0]);
    }

    #[test]
    fn span_ids_count_every_span_owned_or_not() {
        let units = filled("`{python} 1` and `{r} 2` and `{julia} 3`.\n");
        let numbers: Vec<_> = ids(&units)
            .iter()
            .map(|i| i.split('-').next().unwrap().to_string())
            .collect();
        assert_eq!(numbers, ["0", "1", "2"]);
    }

    #[test]
    fn a_front_matter_span_takes_the_first_number_and_shifts_later_spans() {
        let cell = "```{python}\nx = 1\n```\n";
        let body = format!("Body `{{python}} 1+1`.\n\n{cell}");
        let plain = ids(&filled(&body));
        let with_fm = ids(&filled(&format!(
            "---\ntitle: \"`{{python}} 5`\"\n---\n\n{body}"
        )));
        assert_eq!(with_fm.len(), 3);
        assert!(
            with_fm[0].starts_with("0-"),
            "front matter span is first: {with_fm:?}"
        );
        assert!(
            with_fm[1].starts_with("1-"),
            "later span shifts by one: {with_fm:?}"
        );
        assert!(plain[0].starts_with("0-"));
        assert_eq!(with_fm[2], plain[1], "the cell id is unchanged");
    }

    #[test]
    fn the_cell_index_counts_unowned_braced_fences_but_not_raw_blocks() {
        let doc = "```{python}\na = 1\n```\n\n```{=html}\n<b/>\n```\n\n```{r}\nb <- 2\n```\n\n```{python}\nc = 3\n```\n";
        let got = ids(&filled(doc));
        let indices: Vec<_> = got.iter().map(|i| i.split('-').next().unwrap()).collect();
        assert_eq!(
            indices,
            ["0", "1", "2"],
            "raw {{=html}} is not a cell, `r` counts: {got:?}"
        );
    }

    #[test]
    fn the_language_is_in_neither_the_id_nor_the_hash() {
        let dotted = filled("```{python.marimo}\nx = 1\n```\n");
        let spaced = filled("```{python .marimo}\nx = 1\n```\n");
        assert_eq!(ids(&dotted), ids(&spaced));
        let Unit::Cell(c) = &dotted[0] else { panic!() };
        assert_eq!(c.lang, "python.marimo", "quarto_lang keeps the full token");
        let Unit::Cell(s) = &spaced[0] else { panic!() };
        assert_eq!(s.lang, "python");
    }

    #[test]
    fn span_hashes_ignore_the_language_case_and_differ_from_a_cell_with_the_same_text() {
        assert_eq!(
            span_content_hash("Python", "1+1"),
            span_content_hash("python", "1+1")
        );
        assert_ne!(span_content_hash("python", "1+1"), content_hash("1+1"));
        let units = filled("`{Python} 1+1` and `{python} 1+1`\n");
        let got = ids(&units);
        assert_eq!(got[0].split('-').nth(1), got[1].split('-').nth(1));
    }

    #[test]
    fn a_blockquoted_cell_hashes_its_unprefixed_text() {
        let quoted = filled("> ```{python}\n> x = 1\n> ```\n");
        let plain = filled("```{python}\nx = 1\n```\n");
        let hash = |u: &[Unit]| ids(u)[0].split('-').nth(1).unwrap().to_string();
        assert_eq!(hash(&quoted), hash(&plain));
    }
}
