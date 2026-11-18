//! `textDocument/semanticTokens/full`: Quarto 2's own highlighting over LSP
//! (epic bd-xnpyb2vv, P1; plan
//! `claude-notes/plans/2026-11-17-q2-vscode-extension.md`, D-B).
//!
//! The spans come from [`quarto_lsp_core::get_semantic_tokens`], the same
//! extractor hub-client paints Monaco with. Its token types index into
//! [`QMD_TOKEN_LEGEND`], whose dotted names (`qmd.markup.heading`) are not
//! valid LSP/VS Code token-type identifiers (`^\w+[-_\w]*$`). So the LSP has
//! its **own** legend, derived from the Monaco one by [`lsp_type_for`]:
//!
//! - embedded code (`qmd.code.*`) maps onto the standard LSP token types
//!   (`keyword`, `string`, …) where one fits, so every editor theme already
//!   colours it;
//! - structural markup, and code captures with no standard type, map onto
//!   custom `qmd*` types. The VS Code extension declares these
//!   (`contributes.semanticTokenTypes`) and maps them to the TextMate
//!   scopes themes style (`contributes.semanticTokenScopes`), so colours
//!   follow the user's theme. Other clients see them as `qmdHeading` etc.
//!
//! The mapping is total over [`QMD_TOKEN_LEGEND`] (pinned by a test), so a
//! new Monaco legend entry fails the build here rather than going uncoloured.

use std::sync::LazyLock;

use lsp_types::{SemanticToken, SemanticTokenType, SemanticTokens, SemanticTokensLegend};
use quarto_lsp_core::types::{QMD_TOKEN_LEGEND, SemanticToken as QmdToken};

/// The LSP token type for one [`QMD_TOKEN_LEGEND`] entry. `None` only for a
/// name that is not in the legend (the totality test keeps that empty).
pub fn lsp_type_for(qmd: &str) -> Option<&'static str> {
    Some(match qmd {
        // --- structural ---
        "qmd.markup.heading" => "qmdHeading",
        "qmd.markup.emphasis" => "qmdEmphasis",
        "qmd.markup.strong" => "qmdStrong",
        "qmd.markup.strikethrough" => "qmdStrikethrough",
        "qmd.markup.link.label" => "qmdLinkText",
        "qmd.markup.link.url" => "qmdLinkUrl",
        "qmd.markup.link.title" => "qmdLinkTitle",
        "qmd.markup.image.label" => "qmdImageText",
        "qmd.markup.image.url" => "qmdImageUrl",
        "qmd.markup.raw.inline" => "qmdInlineCode",
        "qmd.markup.raw" => "qmdCodeBlock",
        "qmd.markup.raw.info" => "qmdCodeInfo",
        "qmd.markup.math" => "qmdMath",
        "qmd.markup.shortcode" => "qmdShortcode",
        "qmd.markup.list" => "qmdListMarker",
        "qmd.markup.quote" => "qmdQuote",
        "qmd.markup.comment" => "comment",
        "qmd.punctuation.special" => "qmdPunctuation",
        "qmd.punctuation.special.image" => "qmdPunctuation",
        "qmd.punctuation.bracket" => "qmdPunctuation",
        "qmd.punctuation.delimiter.fence" => "qmdFence",
        "qmd.punctuation.delimiter.frontmatter" => "qmdFrontMatterFence",
        "qmd.attribute.specifier" => "qmdAttribute",
        // --- embedded code ---
        "qmd.code.attribute" => "decorator",
        "qmd.code.boolean" => "qmdBoolean",
        "qmd.code.character" => "string",
        "qmd.code.comment" => "comment",
        "qmd.code.constant" => "qmdConstant",
        "qmd.code.constructor" => "class",
        "qmd.code.embedded" => "qmdEmbedded",
        "qmd.code.error" => "qmdError",
        "qmd.code.escape" => "qmdEscape",
        "qmd.code.function" => "function",
        "qmd.code.keyword" => "keyword",
        "qmd.code.label" => "label",
        "qmd.code.markup" => "qmdCodeMarkup",
        "qmd.code.module" => "namespace",
        "qmd.code.namespace" => "namespace",
        "qmd.code.number" => "number",
        "qmd.code.operator" => "operator",
        "qmd.code.property" => "property",
        "qmd.code.punctuation" => "qmdCodePunctuation",
        "qmd.code.special" => "regexp",
        "qmd.code.string" => "string",
        "qmd.code.tag" => "qmdTag",
        "qmd.code.type" => "type",
        "qmd.code.variable" => "variable",
        _ => return None,
    })
}

/// The LSP legend (unique types, in first-use order over
/// [`QMD_TOKEN_LEGEND`]) and, for each Monaco legend index, its index here.
struct LspLegend {
    types: Vec<&'static str>,
    remap: Vec<u32>,
}

static LEGEND: LazyLock<LspLegend> = LazyLock::new(|| {
    let mut types: Vec<&'static str> = Vec::new();
    let mut remap = Vec::with_capacity(QMD_TOKEN_LEGEND.len());
    for name in QMD_TOKEN_LEGEND {
        let lsp = lsp_type_for(name)
            .unwrap_or_else(|| panic!("no LSP token type for legend entry {name}"));
        let index = match types.iter().position(|t| *t == lsp) {
            Some(i) => i,
            None => {
                types.push(lsp);
                types.len() - 1
            }
        };
        remap.push(index as u32);
    }
    LspLegend { types, remap }
});

/// The token types this server advertises, in legend order.
pub fn legend_token_types() -> &'static [&'static str] {
    &LEGEND.types
}

/// The legend for `semanticTokensProvider`.
pub fn legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: LEGEND
            .types
            .iter()
            .map(|t| SemanticTokenType::new(t))
            .collect(),
        token_modifiers: Vec::new(),
    }
}

/// Delta-encode absolute, sorted, single-line tokens into the LSP wire form,
/// remapping their types onto the LSP legend. Tokens with an unknown type
/// index are dropped.
pub fn encode(tokens: &[QmdToken]) -> SemanticTokens {
    let remap = &LEGEND.remap;
    let mut data = Vec::with_capacity(tokens.len());
    let (mut prev_line, mut prev_start) = (0u32, 0u32);
    for t in tokens {
        let Some(&token_type) = remap.get(t.token_type as usize) else {
            continue;
        };
        if t.length == 0 {
            continue;
        }
        let delta_line = t.line - prev_line;
        let delta_start = if delta_line == 0 {
            t.character - prev_start
        } else {
            t.character
        };
        data.push(SemanticToken {
            delta_line,
            delta_start,
            length: t.length,
            token_type,
            token_modifiers_bitset: 0,
        });
        prev_line = t.line;
        prev_start = t.character;
    }
    SemanticTokens {
        result_id: None,
        data,
    }
}

/// Semantic tokens for a qmd buffer's text.
pub fn tokens_for_text(uri: &str, text: &str) -> SemanticTokens {
    let doc = quarto_lsp_core::Document::new(uri, text);
    encode(&quarto_lsp_core::get_semantic_tokens(&doc))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn qmd_index(name: &str) -> u32 {
        QMD_TOKEN_LEGEND.iter().position(|n| *n == name).unwrap() as u32
    }

    fn lsp_index(name: &str) -> u32 {
        legend_token_types()
            .iter()
            .position(|n| *n == name)
            .unwrap() as u32
    }

    #[test]
    fn mapping_is_total_over_the_monaco_legend() {
        for name in QMD_TOKEN_LEGEND {
            assert!(lsp_type_for(name).is_some(), "unmapped legend entry {name}");
        }
    }

    #[test]
    fn lsp_type_names_are_valid_vscode_identifiers() {
        // VS Code: `^\w+[-_\w]*$`.
        for t in legend_token_types() {
            let mut chars = t.chars();
            assert!(
                chars
                    .next()
                    .is_some_and(|c| c.is_alphanumeric() || c == '_')
            );
            assert!(
                chars.all(|c| c.is_alphanumeric() || c == '_' || c == '-'),
                "{t}"
            );
        }
    }

    #[test]
    fn legend_has_no_duplicates() {
        let types = legend_token_types();
        for (i, t) in types.iter().enumerate() {
            assert!(!types[..i].contains(t), "duplicate {t}");
        }
    }

    #[test]
    fn encode_delta_encodes_lines_and_starts() {
        let heading = qmd_index("qmd.markup.heading");
        let keyword = qmd_index("qmd.code.keyword");
        let tokens = [
            QmdToken {
                line: 0,
                character: 0,
                length: 2,
                token_type: heading,
                modifiers: 0,
            },
            QmdToken {
                line: 0,
                character: 2,
                length: 5,
                token_type: heading,
                modifiers: 0,
            },
            QmdToken {
                line: 3,
                character: 4,
                length: 3,
                token_type: keyword,
                modifiers: 0,
            },
            QmdToken {
                line: 3,
                character: 10,
                length: 1,
                token_type: keyword,
                modifiers: 0,
            },
        ];
        let data = encode(&tokens).data;
        let h = lsp_index("qmdHeading");
        let k = lsp_index("keyword");
        let tuples: Vec<_> = data
            .iter()
            .map(|t| (t.delta_line, t.delta_start, t.length, t.token_type))
            .collect();
        assert_eq!(
            tuples,
            vec![(0, 0, 2, h), (0, 2, 5, h), (3, 4, 3, k), (0, 6, 1, k)]
        );
    }

    #[test]
    fn encode_drops_unknown_types_and_empty_tokens() {
        let tokens = [
            QmdToken {
                line: 0,
                character: 0,
                length: 1,
                token_type: 9999,
                modifiers: 0,
            },
            QmdToken {
                line: 1,
                character: 0,
                length: 0,
                token_type: 0,
                modifiers: 0,
            },
            QmdToken {
                line: 2,
                character: 1,
                length: 3,
                token_type: 0,
                modifiers: 0,
            },
        ];
        let data = encode(&tokens).data;
        assert_eq!(data.len(), 1);
        assert_eq!((data[0].delta_line, data[0].delta_start), (2, 1));
    }

    #[test]
    fn tokens_for_text_colours_headings_and_code() {
        let text = "# Title\n\n```{python}\nimport os\n```\n";
        let data = tokens_for_text("file:///t.qmd", text).data;
        let types: Vec<u32> = data.iter().map(|t| t.token_type).collect();
        assert!(types.contains(&lsp_index("qmdHeading")), "{data:?}");
        assert!(types.contains(&lsp_index("keyword")), "{data:?}");
    }
}
