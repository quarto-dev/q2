const common = require('../common/common');

// Every non-ASCII punctuation (`\p{P}`) or symbol (`\p{S}`) code point is
// prose content: Pandoc folds all of them into the surrounding `Str`, and q2
// reserves none of them as syntax (policy: it never will). ASCII punctuation
// is handled case by case below because most of it is qmd markup.
//
// This replaces hand-enumerated range tables for Po/Pc (bd-6kewx) and
// Sm/Sk/Sc, generated from a fixed Unicode version by the former
// scripts/unicode-ranges.py. Those tables drifted (Unicode 14+ Po, a hole at
// U+20B0) and never covered Ps/Pe, so `⟨`, `⌈`, `「`, `（` were parse errors
// (bd-angle-bracket-u27e8-parse-error-r6l55zmh). See
// claude-notes/plans/2026-09-25-unicode-punctuation-coverage.md.
//
// The `&&` class intersection is regex-syntax (Rust) syntax that JS's `u`
// flag rejects, which is why `pandoc_str` uses `RustRegex` rather than
// `new RegExp`.
const PANDOC_NON_ASCII_PUNCT_SYMBOL = "[[\\p{P}\\p{S}]&&[^\\x00-\\x7F]]";

const PANDOC_ALPHA_NUM = "0-9A-Za-z\\p{L}\\p{N}";

// ASCII punctuation and symbols that are plain text (not qmd markup).
const PANDOC_PUNCTUATION = "#%&()/:+=-";

// Smart quotes that are allowed in pandoc_str
// U+2018 = ' (left single quotation mark)
// U+2019 = ' (right single quotation mark / apostrophe)
// U+201A = ‚ (single low-9 quotation mark, German)
// U+201B = ‛ (single high-reversed-9 quotation mark)
// U+201C = " (left double quotation mark)
// U+201D = " (right double quotation mark)
// U+201E = „ (double low-9 quotation mark, German)
// U+201F = ‟ (double high-reversed-9 quotation mark)
// U+2039 = ‹ (single left-pointing angle quotation mark)
// U+203A = › (single right-pointing angle quotation mark)
// U+00AB = « (left-pointing double angle quotation mark / guillemet)
// U+00BB = » (right-pointing double angle quotation mark / guillemet)
const PANDOC_SMART_QUOTES = "\\u{2018}\\u{2019}\\u{201A}\\u{201B}\\u{201C}\\u{201D}\\u{201E}\\u{201F}\\u{2039}\\u{203A}\\u{00AB}\\u{00BB}";

// Everything after the opener line of a `:::`-fenced block: the body
// blocks, the optional `:::` closer, and the block close. Shared by every
// construct that starts with `$._fenced_div_start` (`pandoc_div`,
// `note_definition_fenced_block`, ...), which differ only in what follows
// `::: ` on the opener line. This is a JS helper, not a hidden rule, so
// each construct gets the sequence inlined and the parse table is the same
// as if it were written out by hand.
const fencedDivTail = ($) => seq(
    repeat($._block),
    optional(seq($._fenced_div_end, $._close_block, choice($._newline, $._eof))),
    $._block_close,
);

const regexBracket = (str) => `(?:${str})`;
const regexOr = (...groups) => regexBracket(groups.join("|"));

// Non-ASCII Unicode `White_Space=Yes` codepoints. Per Pandoc 3.9.0.2's
// `markdown` and `commonmark_x` readers, these are folded into the
// surrounding `Str` node — they are content, not whitespace separators
// or break markers. ASCII whitespace (U+0009, U+000A, U+000B, U+000C,
// U+000D, U+0020) is excluded because it has established meaning in
// the qmd grammar. U+0085 (NEXT LINE) is intentionally omitted: its
// behavior was not characterised in the Pandoc experiment.
//
// See claude-notes/plans/2026-04-30-unicode-whitespace-handling.md
// (beads bd-rmx3, bd-8oe4) for the policy and experiment record.
const PANDOC_NON_ASCII_WHITESPACE =
    "\\u{00A0}\\u{1680}\\u{2000}-\\u{200A}\\u{2028}\\u{2029}\\u{202F}\\u{205F}\\u{3000}";

// Combining marks (Mn nonspacing, Mc spacing, Me enclosing) plus format
// characters (Cf: zero width space U+200B, soft hyphen U+00AD, ZWNJ/ZWJ
// U+200C/U+200D, bidi marks and controls, word joiner U+2060, the MathML
// invisible operators U+2061–U+2064, U+FEFF, tag characters, …). Pandoc folds
// all of these into the surrounding `Str` verbatim: decomposed accents
// (`cafe` + U+0301), Indic vowel signs (`का` = U+0915 + U+093E), enclosing
// marks, ZWNJ/ZWJ between letters (Persian/Indic joining control) and a
// `&ZeroWidthSpace;` pasted as its raw codepoint are content, not markup.
// Without this class a bare mark in prose produced a parse ERROR
// (bd-96fswwce, marks) and so did every Cf character except ZWNJ/ZWJ
// (bd-wuiu1of7, GH #672) — the same bug family as bd-6kewx above. The qmd
// writer spells Cf characters as character references (`&ZeroWidthSpace;`,
// `&#x2064;`), so raw ones in source are accepted but not canonical. ZWJ
// inside emoji sequences is unaffected: EMOJI_REGEX matches those as a
// longer token, which wins.
const PANDOC_COMBINING_MARKS = "\\p{M}\\p{Cf}";

const startStrRegex = regexOr(
    "[" + PANDOC_NON_ASCII_WHITESPACE + PANDOC_ALPHA_NUM + PANDOC_SMART_QUOTES + "-]");
const afterUnderscoreRegex = "[" + PANDOC_ALPHA_NUM + "]";

// Thanks, Claude
const EMOJI_REGEX = "(\\p{Extended_Pictographic}(\\p{Emoji_Modifier}|\uFE0F)?(\u200D\\p{Extended_Pictographic}(\\p{Emoji_Modifier}|\uFE0F)?)*)";

// Keycap emojis: 0-9, # with variation selector and combining enclosing keycap
// Unicode TR51: https://www.unicode.org/reports/tr51/
// Note: * keycap emoji (*️⃣) conflicts with emphasis delimiter, use raw reader block instead
const KEYCAP_EMOJI_REGEX = "([0-9#]\\uFE0F?\\u20E3)";

const PANDOC_REGEX_STR =
        regexOr(
            "\\\\.",
            KEYCAP_EMOJI_REGEX,
            EMOJI_REGEX,
            "[" + PANDOC_PUNCTUATION + "]",
            PANDOC_NON_ASCII_PUNCT_SYMBOL,
            "[" + PANDOC_COMBINING_MARKS + "]",
            // A run of dots lexes as ONE token so that smart typography sees the
            // whole run. `apply_smart_typography` is applied per prose-str node
            // (deliberately — that is what keeps `a\.\.\.b` literal, since each
            // escaped dot arrives as its own backslash-plus-dot node), so a run
            // split across nodes can never be converted: each node holds a run of
            // one, which correctly stays literal, and `merge_strs` concatenates
            // them only afterwards.
            //
            // Without this alternative a dot run at *token start* fell through to
            // the single-character `[>.,;!?]` case below and emitted one node per
            // dot, so `the ... menu` never became an ellipsis while `a...b` did.
            // `-` never had this problem because it is in `startStrRegex`; `.` is
            // not, and adding it there would make `.class` a single token next
            // door to the attribute grammar. See bd-ellipsis-not-smart-48bv2pe6.
            //
            // Longest-match in the lexer means a lone `.` still matches as one
            // character, and `..` stays a literal two-dot run per Pandoc's
            // three-at-a-time rule.
            "[.]+",
            "[>.,;!?]",
            startStrRegex +
            regexOr(
                "[!,.;?" + PANDOC_NON_ASCII_WHITESPACE + PANDOC_ALPHA_NUM + PANDOC_SMART_QUOTES + PANDOC_COMBINING_MARKS + "-]",
                // "\\\\.",
                "['\\u{2018}\\u{2019}][\\p{L}\\p{N}]",
                regexBracket("[_]" + afterUnderscoreRegex)
            ) + "*");

// DESIGN INVARIANT — no `conflicts:`.
//
// This grammar intentionally declares NO `conflicts:` rules, so tree-sitter
// generates a deterministic LR parser. tree-sitter's GLR nondeterministic
// multiple-stack mechanism is therefore only ever exercised during *error
// recovery*, never for benign grammar ambiguity. Lexing ambiguities are
// resolved deterministically by the external scanner / token precedence, not
// by GLR.
//
// Downstream code relies on this: the qmd reader detects parse errors via
// `ParseState::has_error` (the parser's "all stack versions are in error"
// flag, surfaced through the `parse_with_options` progress callback) instead
// of the old per-lex logger. Because there is no speculative branching outside
// error recovery, `has_error` corresponds to a genuine parse error rather than
// a speculative branch that will be pruned. See
// `crates/tree-sitter-qmd/bindings/rust/parser.rs::MarkdownTree::had_parse_error`
// and bd-b7eb7. If you ever add `conflicts:`, revisit that error detection.
module.exports = grammar({
    name: 'markdown',

    rules: {
        ///////////////////////////////////////////////////////////////////////////////////////////
        // document

        document: $ => seq(
            optional(alias($.minus_metadata, $.metadata)),
            alias(prec.right(repeat($._block_not_section)), $.section),
            repeat($.section),
        ),

        // YAML metadata block (`---` ... `---`). All four delimiting tokens
        // are external: the scanner opens a block only on a `---` line that
        // is followed by a non-blank line and, somewhere below, by a closing
        // line — exactly `---` at column 0, optionally followed by blanks —
        // and it recognises that closing line only at a line start, so a
        // `---` inside a value is body text. The body is exposed as the
        // `yaml` child so consumers take the YAML's exact range from the
        // tree rather than re-scanning the text (bd-mjo6ao32, GH #671); it
        // is also the node a YAML injection query would target. The closing
        // delimiter's line break is consumed like any other block's.
        minus_metadata: $ => seq(
            $._minus_metadata_start,
            $._minus_metadata_open_newline,
            field('body', alias($._minus_metadata_body, $.yaml)),
            $._minus_metadata_end,
            choice($._newline, $._eof),
        ),

        ///////////////////////////////////////////////////////////////////////////////////////////
        // BLOCK STRUCTURE

        // All blocks. Every block contains a trailing newline.
        _block: $ => choice(
            $._block_not_section,
            $.section,
        ),
        _block_not_section: $ => prec.right(choice(
            $.pandoc_paragraph,
            $.pandoc_block_quote,
            $.pandoc_list,
            $.pandoc_code_block,
            $.pandoc_div,
            $.pandoc_horizontal_rule,
            $.pipe_table,
            $.grid_table,
            $.caption,

            prec(-1, alias($.minus_metadata, $.metadata)),

            $.note_definition_fenced_block,
            $.editorial_div,
            $.inline_ref_def,

            $._soft_line_break,
            $._newline
        )),
        section: $ => choice($._section1, $._section2, $._section3, $._section4, $._section5, $._section6),
        _section1: $ => prec.right(seq(
            alias($._atx_heading1, $.atx_heading),
            repeat(choice(
                alias(choice($._section6, $._section5, $._section4, $._section3, $._section2), $.section),
                $._block_not_section
            ))
        )),
        _section2: $ => prec.right(seq(
            alias($._atx_heading2, $.atx_heading),
            repeat(choice(
                alias(choice($._section6, $._section5, $._section4, $._section3), $.section),
                $._block_not_section
            ))
        )),
        _section3: $ => prec.right(seq(
            alias($._atx_heading3, $.atx_heading),
            repeat(choice(
                alias(choice($._section6, $._section5, $._section4), $.section),
                $._block_not_section
            ))
        )),
        _section4: $ => prec.right(seq(
            alias($._atx_heading4, $.atx_heading),
            repeat(choice(
                alias(choice($._section6, $._section5), $.section),
                $._block_not_section
            ))
        )),
        _section5: $ => prec.right(seq(
            alias($._atx_heading5, $.atx_heading),
            repeat(choice(
                alias($._section6, $.section),
                $._block_not_section
            ))
        )),
        _section6: $ => prec.right(seq(
            alias($._atx_heading6, $.atx_heading),
            repeat($._block_not_section)
        )),

        ///////////////////////////////////////////////////////////////////////////////////////////
        // LEAF BLOCKS

        // An ATX heading. This is currently handled by the external scanner but maybe could be
        // parsed using normal tree-sitter rules.
        //
        // https://github.github.com/gfm/#atx-headings
        _atx_heading1: $ => prec(1, seq(
            $.atx_h1_marker,
            optional($._atx_heading_content),
            choice($._newline, $._eof)
        )),
        _atx_heading2: $ => prec(1, seq(
            $.atx_h2_marker,
            optional($._atx_heading_content),
            choice($._newline, $._eof)
        )),
        _atx_heading3: $ => prec(1, seq(
            $.atx_h3_marker,
            optional($._atx_heading_content),
            choice($._newline, $._eof)
        )),
        _atx_heading4: $ => prec(1, seq(
            $.atx_h4_marker,
            optional($._atx_heading_content),
            choice($._newline, $._eof)
        )),
        _atx_heading5: $ => prec(1, seq(
            $.atx_h5_marker,
            optional($._atx_heading_content),
            choice($._newline, $._eof)
        )),
        _atx_heading6: $ => prec(1, seq(
            $.atx_h6_marker,
            optional($._atx_heading_content),
            choice($._newline, $._eof)
        )),
        _atx_heading_content: $ => prec(1, seq(
            optional($._whitespace),
            $._inlines, 
        )),
        pandoc_horizontal_rule: $ => seq($._thematic_break, choice($._newline, $._eof)),

        pandoc_paragraph: $ => seq(
            optional($._inline_whitespace),
            $._inlines, 
            choice($._newline, $._eof)
        ),

        inline_ref_def: $ => seq(
            $.ref_id_specifier,
            $._whitespace,
            $.pandoc_paragraph),

        // ideally caption would _only_ be a field in the pipe table, but
        // it would make parsing the blank lines hard. So we allow it
        // anywhere where we have blocks and then lift it into pipe_tables.
        // This is the same principle we use for attributes in headings and equations.

        caption: $ => seq(
            $._caption_start,
            $._inline_whitespace,
            $._inlines,
            choice($._newline, $._eof)
        ),

        ///////////////////////////////////////////////////////////////////////////////////////////
        // pipe tables
        
        pipe_table: $ => prec.right(seq(
            $._pipe_table_start,
            alias($.pipe_table_row, $.pipe_table_header),
            $._newline,
            $.pipe_table_delimiter_row,
            repeat(seq($._pipe_table_newline, optional($.pipe_table_row))),
            optional(seq($._pipe_table_newline, $.caption)),
            choice($._newline, $._eof),
        )),

        _pipe_table_newline: $ => seq(
            $._pipe_table_line_ending,
            optional($.block_continuation)
        ),

        pipe_table_delimiter_row: $ => seq(
            optional(seq(
                optional($._whitespace),
                $._pipe_table_delimiter,
            )),
            repeat1(prec.right(seq(
                optional($._whitespace),
                $.pipe_table_delimiter_cell,
                optional($._whitespace),
                $._pipe_table_delimiter,
            ))),
            optional($._whitespace),
            optional(seq(
                $.pipe_table_delimiter_cell,
                optional($._whitespace)
            )),
        ),

        pipe_table_delimiter_cell: $ => seq(
            optional(alias(':', $.pipe_table_align_left)),
            repeat1('-'),
            optional(alias(':', $.pipe_table_align_right)),
        ),

        pipe_table_row: $ => prec(2, seq(
            optional(seq(
                optional($._whitespace),
                $._pipe_table_delimiter,
            )),
            choice(
                seq(
                    repeat1(prec(2, prec.right(seq(
                        choice(
                            seq(
                                optional($._whitespace),
                                $.pipe_table_cell,
                                optional($._whitespace)
                            ),
                            alias($._whitespace, $.pipe_table_cell)
                        ),
                        $._pipe_table_delimiter,
                    )))),
                    optional($._whitespace),
                    optional(seq(
                        $.pipe_table_cell,
                        optional($._whitespace)
                    )),
                ),
                seq(
                    optional($._whitespace),
                    $.pipe_table_cell,
                    optional($._whitespace)
                )
            ),
        )),

        pipe_table_cell: $ => $._line_with_maybe_spaces,

        
        ///////////////////////////////////////////////////////////////////////////////////////////
        // inline nodes

        entity_reference: $ => common.html_entity_regex(),
        numeric_character_reference: $ => token(prec(2, /&#([0-9]{1,7}|[xX][0-9a-fA-F]{1,6});/)),

        _inlines: $ => prec.right(seq(
            $._line,
            repeat(seq(alias($._soft_line_break, $.pandoc_soft_break), $._line))
        )),


        pandoc_span: $ => prec.right(seq(
            '[',
            optional($._inline_whitespace),
            optional(alias($._inlines, $.content)),
            choice(
                $.target,
                /[ \t]*[\]]/,
            ),
            optional(alias($._pandoc_attr_specifier, $.attribute_specifier))
        )),

        pandoc_image: $ => prec.right(seq(
            '![',
            optional($._inline_whitespace),
            optional(alias($._inlines, $.content)),
            choice(
                $.target,
                /[ \t]*[\]]/,
            ),
            optional(alias($._pandoc_attr_specifier, $.attribute_specifier))
        )),

        target: $ => seq(
            /[ \t]*[\]][(]/, 
            optional($._inline_whitespace),
            alias(repeat(choice(/[^ {\t)]|(\\.)+/, $.shortcode)), $.url),
            optional(seq($._inline_whitespace, alias($._commonmark_double_quote_string, $.title))),
            ')'
        ),

        pandoc_math: $ => seq(
            '$',
            /[^$ \t\n\r]([ \t]*[^$ \t\n\r]+|\\\$)*/,
            // bd-ilv8p: allow inline math to span multiple lines. Each
            // line's content must still satisfy the "no whitespace
            // adjacent to a delimiter" rule, so we model multi-line
            // math as one-or-more line segments joined by
            // _soft_line_break (which itself consumes the line ending
            // plus any block-continuation prefix — `> `, list indent,
            // etc.). The break is aliased to pandoc_soft_break so the
            // post-processor can find its byte range and strip the
            // gutter when assembling the InlineMath text. The regex
            // also now excludes \r as a pre-existing CRLF correctness
            // fix.
            repeat(seq(
                alias($._soft_line_break, $.pandoc_soft_break),
                /[^$ \t\n\r]([ \t]*[^$ \t\n\r]+|\\\$)*/
            )),
            '$',
        ),

        pandoc_display_math: $ => seq(
            '$$',
            /([^$]|[$][^$]|\\\$)+/,
            '$$'
        ),

        pandoc_code_span: $ => prec.right(seq(
            alias($._code_span_start, $.code_span_delimiter),
            // this is a goofy construction but it lets the external scanner in to
            // do add the code_span_code token
            alias(repeat1(choice(
                    /[^`\n\r]+/,
                    // bd-nycn85a8: backtick runs inside the span are emitted
                    // whole by the external scanner (any run whose length
                    // differs from the delimiter). Never match single
                    // backticks here: that split runs and let a later
                    // fragment close the span early.
                    $._code_span_backtick_run,
                    // bd-ilv8p: line breaks inside content. The scanner's
                    // parse_code_span look-ahead allows the opener to commit
                    // across newlines (up to a blank line); _soft_line_break
                    // is _soft_line_ending + optional(block_continuation), so
                    // the `> ` / list-indent gutter is consumed structurally
                    // here rather than appearing in the text segments.
                    alias($._soft_line_break, $.pandoc_soft_break)
                )), $.content),
            alias($._code_span_close, $.code_span_delimiter),
            optional($.attribute_specifier)
        )),

        pandoc_single_quote: $ => prec.right(seq(
            alias($._single_quote_span_open, $.single_quote),
            optional(alias($._inlines, $.content)),
            alias($._single_quote_span_close, $.single_quote),
        )),

        pandoc_double_quote: $ => prec.right(seq(
            alias($._double_quote_span_open, $.double_quote),
            optional(alias($._inlines, $.content)),
            alias($._double_quote_span_close, $.double_quote),
        )),

        insert: $ => prec.right(seq(
            prec(3, alias($._insert_span_start, $.insert_delimiter)),
            optional($._inline_whitespace),
            optional(alias($._inlines, $.content)),
            prec(3, alias(/[ ]*\]/, $.insert_delimiter)),
            optional(alias($._pandoc_attr_specifier, $.attribute_specifier))
        )),

        delete: $ => prec.right(seq(
            prec(3, alias($._delete_span_start, $.delete_delimiter)),
            optional($._inline_whitespace),
            optional(alias($._inlines, $.content)),
            prec(3, alias(/[ ]*\]/, $.delete_delimiter)),
            optional(alias($._pandoc_attr_specifier, $.attribute_specifier))
        )),

        edit_comment: $ => prec.right(seq(
            prec(3, alias($._edit_comment_span_start, $.edit_comment_delimiter)),
            optional($._inline_whitespace),
            optional(alias($._inlines, $.content)),
            prec(3, alias(/[ ]*\]/, $.edit_comment_delimiter)),
            optional(alias($._pandoc_attr_specifier, $.attribute_specifier))
        )),

        highlight: $ => prec.right(seq(
            prec(3, alias($._highlight_span_start, $.highlight_delimiter)),
            optional($._inline_whitespace),
            optional(alias($._inlines, $.content)),
            prec(3, alias(/[ ]*\]/, $.highlight_delimiter)),
            optional(alias($._pandoc_attr_specifier, $.attribute_specifier))
        )),
       
        attribute_specifier: $ => seq(
            '{',
            optional($._attr_ws),
            optional(choice(
                $.raw_specifier, // =aslkjfdasd
                $.language_specifier, // python
                $.commonmark_specifier,
                // #id
                // .class
                // #id .class
                // key=value
                // NOT: python .class
                alias($._commonmark_specifier_start_with_class, $.commonmark_specifier),
                alias($._commonmark_specifier_start_with_kv, $.commonmark_specifier)
            )),
            '}'
        ),

        _pandoc_attr_specifier: $ => seq(
            '{',
            optional($._attr_ws),
            optional(choice(
                $.unnumbered_specifier,
                $.commonmark_specifier,
                alias($._commonmark_specifier_start_with_class, $.commonmark_specifier),
                alias($._commonmark_specifier_start_with_kv, $.commonmark_specifier)
            )),
            '}'
        ),

        unnumbered_specifier: $ => "-",

        language_specifier: $ => choice(
            seq($._language_specifier_token,
                optional(choice(
                    $.commonmark_specifier,
                    seq(optional($._inline_whitespace), alias($._commonmark_specifier_start_with_class, $.commonmark_specifier)),
                    seq(optional($._inline_whitespace), alias($._commonmark_specifier_start_with_kv, $.commonmark_specifier))
                ))),
            seq('{', $.language_specifier, '}')
        ),

        commonmark_specifier: $ => prec.right(seq(
            optional($._inline_whitespace),
            alias(/[#][._A-Za-z0-9-]+/, $.attribute_id),
            optional(
                seq($._attr_ws,
                    choice(
                        $._commonmark_specifier_start_with_class,
                        $._commonmark_specifier_start_with_kv))),
            optional($._attr_ws),
        )),

        _commonmark_specifier_start_with_class: $ => prec.right(seq(
            alias(/[.][A-Za-z][A-Za-z0-9_.-]*/, $.attribute_class),
            optional(repeat(seq($._attr_ws, alias(/[.][A-Za-z][A-Za-z0-9_-]*/, $.attribute_class)))),
            optional(seq($._attr_ws, $._commonmark_specifier_start_with_kv)),
            optional($._attr_ws),
        )),

        _commonmark_specifier_start_with_kv: $ => prec.right(seq(
            alias($._commonmark_key_value_specifier, $.key_value_specifier),
            optional(repeat(seq(optional($._attr_ws), alias($._commonmark_key_value_specifier, $.key_value_specifier)))),
            optional($._attr_ws)
        )),

        _commonmark_key_value_specifier: $ => seq(
            alias($._key_specifier_token, $.key_value_key),
            optional($._inline_whitespace),
            '=',
            optional($._inline_whitespace),
            alias(choice($._value_specifier_token, $._commonmark_single_quote_string, "''", '""', $._commonmark_double_quote_string), $.key_value_value)
        ),

        _commonmark_naked_value: $ => /[A-Za-z0-9_-]+/,
        // A fenced div's bare info string (`::: note`). Unlike code-block
        // info strings it may not start with `-`: after `::: `, a leading
        // `-` is reserved for the block delete mark (`::: --`), and
        // `::: --foo` / `::: ---` are errors (Q-2-53) rather than classes.
        _div_info_string: $ => /[A-Za-z0-9_][A-Za-z0-9_-]*/,
        _commonmark_single_quote_string: $ => seq(/[']/, choice(/([^ ']|\\')/, $.shortcode), repeat(choice(/[^']/, /\\'/, $.shortcode)), /[']/),
        _commonmark_double_quote_string: $ => seq(/["]/, choice(/([^ "]|\\")/, $.shortcode), repeat(choice(/[^"]/, /\\"/, $.shortcode)), /["]/),

        _line: $ => prec.right(seq($._inline_element, repeat(seq(optional(alias($._whitespace, $.pandoc_space)), $._inline_element)))),
        _line_with_maybe_spaces: $ => prec.right(repeat1(choice(alias($._whitespace, $.pandoc_space), $._inline_element))),

        _inline_element: $ => choice(
            $.pandoc_str, 
            $.pandoc_span,
            $.pandoc_math,
            $.pandoc_display_math,
            $.pandoc_code_span,
            $.pandoc_image,
            $.pandoc_single_quote,
            $.pandoc_double_quote,

            alias($._html_comment, $.comment),

            $.highlight,
            $.insert,
            $.delete,
            $.edit_comment,

            $.shortcode,
            $.shortcode_escaped,

            $.citation,
            $.inline_note,

            $.pandoc_superscript,
            $.pandoc_subscript,
            $.pandoc_strikeout,

            $.pandoc_emph,
            $.pandoc_strong,

            $.entity_reference,
            $.numeric_character_reference,
            $.inline_note_reference,

            alias($._autolink, $.autolink),

            $.html_element,
            alias($._pandoc_line_break, $.pandoc_line_break),
            alias($._pandoc_attr_specifier, $.attribute_specifier),
        ),

        _shortcode_sep: $ => choice($._whitespace, $._soft_line_break, seq($._soft_line_break, $._whitespace), seq($._whitespace, $._soft_line_break)),

        // shortcodes
        shortcode_escaped: $ => seq(
            alias($._shortcode_open_escaped, $.shortcode_delimiter), // "{{{<",
            $._shortcode_sep,
            $.shortcode_name,
            repeat(seq($._shortcode_sep, $._shortcode_value)),
            repeat(seq($._shortcode_sep, alias($._commonmark_key_value_specifier, $.key_value_specifier))),
            $._shortcode_sep,
            alias($._shortcode_close_escaped, $.shortcode_delimiter), //">}}}",
        ),

        shortcode: $ => seq(
            alias($._shortcode_open, $.shortcode_delimiter), // "{{<",
            $._shortcode_sep,
            $.shortcode_name,
            repeat(seq($._shortcode_sep, $._shortcode_value)),
            repeat(seq($._shortcode_sep, alias($._shortcode_key_value_specifier, $.key_value_specifier))),
            $._shortcode_sep,
            alias($._shortcode_close, $.shortcode_delimiter), //">}}",
        ),

        _shortcode_value: $ => choice($.shortcode_name, alias($._language_specifier_token, $.shortcode_naked_string), $.shortcode_naked_string, $.shortcode_string, $.shortcode, $.shortcode_number),

        _shortcode_key_value_specifier: $ => seq(
            alias($._key_specifier_token, $.key_value_key),
            optional($._inline_whitespace),
            '=',
            optional($._inline_whitespace),
            alias($._shortcode_value, $.key_value_value)
        ),

        shortcode_name: $ => token(prec(1, new RustRegex("[a-zA-Z_][a-zA-Z0-9_-]*"))),

        // Anything that is not whitespace, a shortcode/attr delimiter, or `=`
        // (the key/value separator), plus `\`-escape pairs. Quotes are excluded
        // only at the first character, where they would open a quoted string;
        // interior apostrophes are content (`don't`). Deliberately a blocklist:
        // an allowlist made every unenumerated character — all of Unicode
        // included — a fatal parse error that dropped the whole document
        // (bd-shortcode-escaped-gt-fatal-2u79bqp1, bd-shortcode-naked-value-nonascii-47fzbmow).
        // `=` stays excluded: admitting it would make `a=b` ambiguous with the
        // key/value production and could silently reinterpret existing
        // shortcodes. Bare-`=` parity gap is bd-kx25ovmh.
        // NOTE: this is not the only producer of `shortcode_naked_string` —
        // the external `_language_specifier_token` (scanner.c:2159) is aliased
        // to the same node kind and decides letter-initial arguments first.
        shortcode_naked_string: $ =>
            choice(token(prec(1, new RustRegex("(?:[^ \\t\\n\\r'\"<>{}=\\\\]|\\\\.)(?:[^ \\t\\n\\r<>{}=\\\\]|\\\\.)*"))),
                   token(prec(1, /(?:[A-Za-z0-9_.~:/?#\]@!$%&()+,;-]|\[)+[?](?:[A-Za-z0-9_.~:/?#\]@!%$&()+,;?=-]|\[)+/))),

        shortcode_string: $ => choice(
            $._commonmark_single_quote_string,
            $._commonmark_double_quote_string,
        ),
        // // shortcode numbers are numbers as JSON sees them
        // // https://stackoverflow.com/a/13340826
        shortcode_number: $ => token(prec(3, /-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?/)),
      
        /*
            From https://pandoc.org/demo/example33/8.20-citation-syntax.html:

            Unless a citation key starts with a letter, digit, or _, and contains only 
            alphanumerics and single internal punctuation characters (:.#$%&-+?<>~/), 
            it must be surrounded by curly braces, which are not considered part of the key.

            citations are impossible to parse in a context-free manner, so we parse
            them as terminal nodes and then use a post-processing step taking advantage
            of the inline_link syntax
        */

        citation: $ => choice(
            seq(alias($._cite_author_in_text_with_open_bracket, $.citation_delimiter),
                alias(new RegExp('[^\\s\\n}]+'), $.citation_id_author_in_text),
                alias("}", $.citation_delimiter),
            ),
            seq(alias($._cite_suppress_author_with_open_bracket, $.citation_delimiter),
                alias(new RegExp('[^\\s\\n}]+'), $.citation_id_suppress_author),
                alias("}", $.citation_delimiter),
            ),
            seq(alias($._cite_author_in_text, $.citation_delimiter),
                alias(new RegExp('[0-9A-Za-z_]+([:.#$%&+?<>~/-][0-9A-Za-z_]+)*'), $.citation_id_author_in_text)
            ),
            seq(alias($._cite_suppress_author, $.citation_delimiter),
                alias(new RegExp('[0-9A-Za-z_]+([:.#$%&+?<>~/-][0-9A-Za-z_]+)*'), $.citation_id_suppress_author)
            ),
        ),

        inline_note: $ => seq(
            alias($._inline_note_start_token, $.inline_note_delimiter),
            optional($._inline_whitespace),
            $._inlines,
            optional($._inline_whitespace),
            alias(/[\t ]*[\]]/, $.inline_note_delimiter),
        ),

        pandoc_superscript: $ => seq(
            alias($._superscript_open, $.superscript_delimiter),
            $._inlines,
            alias($._superscript_close, $.superscript_delimiter),
        ),

        pandoc_subscript: $ => seq(
            alias($._subscript_open, $.subscript_delimiter),
            $._inlines,
            alias($._subscript_close, $.subscript_delimiter),
        ),

        pandoc_strikeout: $ => seq(
            alias($._strikeout_open, $.strikeout_delimiter),
            $._inlines,
            alias($._strikeout_close, $.strikeout_delimiter),
        ),

        pandoc_emph: $ => choice(seq(
            alias($._emphasis_open_star, $.emphasis_delimiter),
            $._inlines,
            alias($._emphasis_close_star, $.emphasis_delimiter),
        ), seq(
            alias($._emphasis_open_underscore, $.emphasis_delimiter),
            $._inlines,
            alias($._emphasis_close_underscore, $.emphasis_delimiter),
        )),

        pandoc_strong: $ => choice(seq(
            alias($._strong_emphasis_open_star, $.strong_emphasis_delimiter),
            $._inlines,
            alias($._strong_emphasis_close_star, $.strong_emphasis_delimiter),
        ), seq(
            alias($._strong_emphasis_open_underscore, $.strong_emphasis_delimiter),
            $._inlines,
            alias($._strong_emphasis_close_underscore, $.strong_emphasis_delimiter),
        )),

        // Things that are parsed directly as a pandoc str. `$._pandoc_literal_str`
        // (bd-j9cf) is emitted by the external scanner when a bare '<' has no
        // HTML construct interpretation; it becomes part of a pandoc_str node
        // so downstream consumers see it as a normal Str.
        pandoc_str: $ => choice(new RustRegex(PANDOC_REGEX_STR), '|', $._pandoc_literal_str),

        // CONTAINER BLOCKS

        ///////////////////////////////////////////////////////////////////////////////////////////
        // A block quote. This is the most basic example of a container block handled by the
        // external scanner.
        //
        // https://github.github.com/gfm/#block-quotes
        pandoc_block_quote: $ => seq(
            alias($._block_quote_start, $.block_quote_marker),
            optional($.block_continuation),
            repeat($._block),
            $._block_close,
            optional($.block_continuation)
        ),

        ///////////////////////////////////////////////////////////////////////////////////////////
        // A list. This grammar does not differentiate between loose and tight lists for efficiency
        // reasons.
        //
        // Lists can only contain list items with list markers of the same type. List items are
        // handled by the external scanner.
        //
        // https://github.github.com/gfm/#lists
        pandoc_list: $ => prec.right(choice(
            $._list_plus,
            $._list_minus,
            $._list_star,
            $._list_dot,
            $._list_parenthesis,
            $._list_example
        )),
        _list_plus: $ => prec.right(repeat1(alias($._list_item_plus, $.list_item))),
        _list_minus: $ => prec.right(repeat1(alias($._list_item_minus, $.list_item))),
        _list_star: $ => prec.right(repeat1(alias($._list_item_star, $.list_item))),
        _list_dot: $ => prec.right(repeat1(alias($._list_item_dot, $.list_item))),
        _list_parenthesis: $ => prec.right(repeat1(alias($._list_item_parenthesis, $.list_item))),
        _list_example: $ => prec.right(repeat1(alias($._list_item_example, $.list_item))),
        // Some list items can not interrupt a paragraph and are marked as such by the external
        // scanner.
        list_marker_plus: $ => choice($._list_marker_plus, $._list_marker_plus_dont_interrupt),
        list_marker_minus: $ => choice($._list_marker_minus, $._list_marker_minus_dont_interrupt),
        list_marker_star: $ => choice($._list_marker_star, $._list_marker_star_dont_interrupt),
        list_marker_dot: $ => choice($._list_marker_dot, $._list_marker_dot_dont_interrupt),
        list_marker_parenthesis: $ => choice($._list_marker_parenthesis, $._list_marker_parenthesis_dont_interrupt),
        list_marker_example: $ => choice($._list_marker_example, $._list_marker_example_dont_interrupt),
        _list_item_plus: $ => choice(
            seq(
                $.list_marker_plus,
                optional($.block_continuation),
                $._list_item_content,
                $._block_close,
                optional($.block_continuation)
            ),
            seq(
                $.list_marker_plus,
                optional($._blank_line),
                $._block_close,
                optional($.block_continuation)
            ),
        ),
        _list_item_minus: $ => choice(
            seq(
                $.list_marker_minus,
                optional($.block_continuation),
                $._list_item_content,
                $._block_close,
                optional($.block_continuation)
            ),
            seq(
                $.list_marker_minus,
                optional($._blank_line),
                $._block_close,
                optional($.block_continuation)
            ),
        ),
        _list_item_star: $ => choice(
            // Normal case: list item with content
            seq(
                $.list_marker_star,
                optional($.block_continuation),
                $._list_item_content,
                $._block_close,
                optional($.block_continuation)
            ),
            // Empty case: list item with no content
            seq(
                $.list_marker_star,
                optional($._blank_line),
                $._block_close,
                optional($.block_continuation)
            ),
        ),
        _list_item_dot: $ => choice(
            seq(
                $.list_marker_dot,
                optional($.block_continuation),
                $._list_item_content,
                $._block_close,
                optional($.block_continuation)
            ),
            seq(
                $.list_marker_dot,
                optional($._blank_line),
                $._block_close,
                optional($.block_continuation)
            ),
        ),
        _list_item_parenthesis: $ => choice(
            seq(
                $.list_marker_parenthesis,
                optional($.block_continuation),
                $._list_item_content,
                $._block_close,
                optional($.block_continuation)
            ),
            seq(
                $.list_marker_parenthesis,
                optional($._blank_line),
                $._block_close,
                optional($.block_continuation)
            ),
        ),
        _list_item_example: $ => choice(
            seq(
                $.list_marker_example,
                optional($.block_continuation),
                $._list_item_content,
                $._block_close,
                optional($.block_continuation)
            ),
            seq(
                $.list_marker_example,
                optional($._blank_line),
                $._block_close,
                optional($.block_continuation)
            ),
        ),
        // List items are closed after two consecutive blank lines
        _list_item_content: $ => prec.left(choice(
            seq(
                $._blank_line,
                $._blank_line,
                $._close_block,
                optional($.block_continuation)
            ),
            repeat1($._block),
            // GFM task-list item: `[ ]` / `[x]` / `[X]` immediately after the
            // list marker, then the item's paragraph. The required trailing
            // whitespace is part of the marker token — without it, `[x](url)`
            // would lex as a marker and could never backtrack to the inline
            // `[` interpretation; with it, `[x](`/`[xx]` fail the token and
            // parse as inline spans/links.
            prec(1, seq(
                choice($.task_list_marker_checked, $.task_list_marker_unchecked),
                $.pandoc_paragraph,
                repeat($._block),
            )),
        )),

        task_list_marker_checked: $ => token(prec(1, /\[[xX]\][ \t]/)),
        task_list_marker_unchecked: $ => token(prec(1, /\[[ \t]\][ \t]/)),

        ///////////////////////////////////////////////////////////////////////////////////////////
        // A fenced code block. Fenced code blocks are mainly handled by the external scanner. In
        // case of backtick code blocks the external scanner also checks that the info string is
        // proper.
        //
        // https://github.github.com/gfm/#fenced-code-blocks
        pandoc_code_block: $ => prec.right(choice(
            seq(
                alias($._fenced_code_block_start_backtick, $.fenced_code_block_delimiter),
                optional($._whitespace),
                optional(choice(alias($._commonmark_naked_value, $.info_string), $.attribute_specifier)),
                $._newline,
                optional($.code_fence_content),
                optional(seq(alias($._fenced_code_block_end_backtick, $.fenced_code_block_delimiter), $._close_block, choice($._newline, $._eof))),
                $._block_close,
            ),
        )),
        code_fence_content: $ => repeat1(choice($._newline, $._code_line)),
        _code_line:         $ => /[^\n]+/,
        

        ///////////////////////////////////////////////////////////////////////////////////////////
        // fenced divs

        pandoc_div: $ => seq(
          $._fenced_div_start,
          optional($._whitespace),
          choice(alias($._div_info_string, $.info_string), alias($._pandoc_attr_specifier, $.attribute_specifier)),
          $._newline,
          fencedDivTail($),
        ),

        ///////////////////////////////////////////////////////////////////////////////////////////
        // qmd extension: a fenced block for note definitions:

        /// ::: ^note
        /// this is a longer note
        /// 
        /// many paras even
        /// :::

        note_definition_fenced_block: $ => seq(
            $._fenced_div_start,
            $._whitespace,
            $.fenced_div_note_id,
            $._newline,
            fencedDivTail($),
        ),

        ///////////////////////////////////////////////////////////////////////////////////////////
        // qmd extension: block-level editorial marks, the block counterparts of
        // the inline `[++ ...]`, `[-- ...]`, `[>> ...]` and `[!! ...]` marks:

        /// ::: -- {author="cs"}
        /// delete all of these.
        ///
        /// paragraphs.
        /// :::

        // The marker names the kind; the delimiter node names match the
        // inline marks'. The scanner only emits a marker when the doubled
        // character is followed by whitespace, a line ending or `{`, so
        // `::: --foo` never gets here.
        editorial_div: $ => seq(
            $._fenced_div_start,
            $._whitespace,
            choice(
                alias($._fenced_div_insert_marker, $.insert_delimiter),
                alias($._fenced_div_delete_marker, $.delete_delimiter),
                alias($._fenced_div_edit_comment_marker, $.edit_comment_delimiter),
                alias($._fenced_div_highlight_marker, $.highlight_delimiter),
            ),
            optional(seq(
                optional($._whitespace),
                alias($._pandoc_attr_specifier, $.attribute_specifier),
            )),
            $._newline,
            fencedDivTail($),
        ),

        ///////////////////////////////////////////////////////////////////////////////////////////
        // Newlines as in the spec. Parsing a newline triggers the matching process by making
        // the external parser emit a `$._line_ending`.

        // A blank line including the following newline.
        // https://github.github.com/gfm/#blank-lines
        _blank_line: $ => seq(
            $._blank_line_start, 
            choice($._newline, $._eof)
        ),

        _newline: $ => seq(
            $._line_ending,
            optional($.block_continuation)
        ),

        // prec.right: a _whitespace after _soft_line_ending could also
        // be parsed OUTSIDE this rule (e.g. _attr_ws / _shortcode_sep
        // build seq(_soft_line_break, _whitespace) shapes). Prefer
        // absorbing it here — both readings are separators, and
        // absorption is what keeps stray continuation indentation out
        // of the inline stream.
        _soft_line_break: $ => prec.right(seq(
            $._soft_line_ending,
            optional($.block_continuation),
            // bd-indented-continuation-parse-error-j7be7kuc: a
            // continuation line's leading indentation is not always
            // consumed by the scanner. When a SOFT_LINE_ENDING gate
            // peek judges an indented line "prose", the peeked path
            // deliberately skips mark_end (the lexer cannot rewind to
            // post-indent/pre-delimiter), so any indentation beyond
            // what block_continuation claims reaches the parser as a
            // _whitespace token. Absorb it here — pandoc strips
            // continuation-line leading whitespace, so it contributes
            // nothing to the inline stream (the whole seq is aliased
            // to pandoc_soft_break -> a single SoftBreak).
            optional($._whitespace)
        )),


        _inline_whitespace: $ => prec(-1, choice($._whitespace, $._soft_line_break)),
        // Like _inline_whitespace, but matches a run of whitespace/soft-line-breaks
        // so attribute lists can span multiple lines with leading indent on
        // continuation lines (e.g. ![](x.png){\n  .a\n  .b\n}).
        _attr_ws: $ => prec(-1, repeat1(choice($._whitespace, $._soft_line_break))),
        _whitespace: $ => /[ \t]+/,
        _linebreak: $ => /[\r\n]+/,
    },

    externals: $ => [
        // QMD CHANGES NOTE:
        // Do not change anything here, even if these external tokens are not used in the grammar.
        // they need to match the external c scanner.

        // Block structure gets parsed as follows: After every newline (`$._line_ending`) we try to match
        // as many open blocks as possible. For example if the last line was part of a block quote we look
        // for a `>` at the beginning of the next line. We emit a `$.block_continuation` for each matched
        // block. For this process the external scanner keeps a stack of currently open blocks.
        //
        // If we are not able to match all blocks that does not necessarily mean that all unmatched blocks
        // have to be closed. It could also mean that the line is a lazy continuation line
        // (https://github.github.com/gfm/#lazy-continuation-line
        
        // If a block does get closed (because it was not matched or because some closing token was
        // encountered) we emit a `$._block_close` token

        $._line_ending, // this token does not contain the actual newline characters. see `$._newline`
        $._soft_line_ending,
        $._block_close,
        $.block_continuation,

        // Tokens signifying the start of a block. Blocks that do not need a `$._block_close` because they
        // always span one line are marked as such.

        $._block_quote_start,
        $.atx_h1_marker, // atx headings do not need a `$._block_close`
        $.atx_h2_marker,
        $.atx_h3_marker,
        $.atx_h4_marker,
        $.atx_h5_marker,
        $.atx_h6_marker,
        $._thematic_break, // thematic breaks do not need a `$._block_close`
        $._list_marker_minus,
        $._list_marker_plus,
        $._list_marker_star,
        $._list_marker_parenthesis,
        $._list_marker_dot,
        $._list_marker_minus_dont_interrupt, // list items that do not interrupt an ongoing paragraph
        $._list_marker_plus_dont_interrupt,
        $._list_marker_star_dont_interrupt,
        $._list_marker_parenthesis_dont_interrupt,
        $._list_marker_dot_dont_interrupt,
        $._list_marker_example,
        $._list_marker_example_dont_interrupt,
        $._fenced_code_block_start_backtick,
        $._blank_line_start, // Does not contain the newline characters. Blank lines do not need a `$._block_close`

        // Special tokens for block structure

        // Closing backticks for a fenced code block. They are used to trigger a `$._close_block`
        // which in turn will trigger a `$._block_close` at the beginning the following line.
        $._fenced_code_block_end_backtick,

        // Similarly this is used if the closing of a block is not decided by the external parser.
        // A `$._block_close` will be emitted at the beginning of the next line. Notice that a
        // `$._block_close` can also get emitted if the parent block closes.
        $._close_block,

        // An `$._error` token is never valid  and gets emmited to kill invalid parse branches. Concretely
        // this is used to decide wether a newline closes a paragraph and together and it gets emitted
        // when trying to parse the `$._trigger_error` token in `$.link_title`.
        $._error,
        $._trigger_error,
        $._eof,

        // YAML metadata block, as four tokens; see the `minus_metadata` rule
        // and the matching section of scanner.c.
        $._minus_metadata_start,
        $._minus_metadata_open_newline,
        $._minus_metadata_body,
        $._minus_metadata_end,

        $._pipe_table_start,
        $._pipe_table_line_ending,

        $._fenced_div_start,
        $._fenced_div_end,

        $.ref_id_specifier,
        $.fenced_div_note_id,
        // block-level editorial marks; see `editorial_div`
        $._fenced_div_insert_marker,
        $._fenced_div_delete_marker,
        $._fenced_div_edit_comment_marker,
        $._fenced_div_highlight_marker,

        // code span delimiters for parsing pipe table cells
        $._code_span_start,
        $._code_span_close,
        $._code_span_backtick_run,

        // latex span delimiters for parsing pipe table cells
        $._latex_span_start,
        $._latex_span_close,

        // HTML comment token
        $._html_comment,

        // raw specifiers
        $.raw_specifier, // no leading underscore because it is needed in common.js without it.

        // autolinks
        $._autolink,

        $._language_specifier_token, // external so we can do negative lookahead assertions.
        $._key_specifier_token,
        $._value_specifier_token, // external so we can emit it only when allowed

        $._highlight_span_start,
        $._insert_span_start,
        $._delete_span_start,
        $._edit_comment_span_start,

        $._single_quote_span_open,
        $._single_quote_span_close,
        $._double_quote_span_open,
        $._double_quote_span_close,

        $._shortcode_open_escaped,
        $._shortcode_close_escaped,
        $._shortcode_open,
        $._shortcode_close,

        $._cite_author_in_text_with_open_bracket,
        $._cite_suppress_author_with_open_bracket,
        $._cite_author_in_text,
        $._cite_suppress_author,

        $._strikeout_open,
        $._strikeout_close,
        $._subscript_open,
        $._subscript_close,
        $._superscript_open,
        $._superscript_close,
        $._inline_note_start_token,

        $._strong_emphasis_open_star,
        $._strong_emphasis_close_star,
        $._strong_emphasis_open_underscore,
        $._strong_emphasis_close_underscore,
        $._emphasis_open_star,
        $._emphasis_close_star,
        $._emphasis_open_underscore,
        $._emphasis_close_underscore,
        
        $.inline_note_reference, // we just send this token directly through

        $.html_element, // best-effort lexing of HTML elements simply for error reporting.

        // "This is literal text after all." Emitted by the scanner for:
        // - bd-j9cf: a single '<' that is not the start of an HTML construct
        //   (element, autolink, comment, raw-specifier), see
        //   parse_open_angle_brace;
        // - bd-star-as-str-qigl02pz: a `*` or `_` run that CommonMark's
        //   flanking rules say cannot open (followed by whitespace) and
        //   cannot close (preceded by whitespace / line start), and a `~` or
        //   `^` whose closer does not appear before the next whitespace
        //   (Pandoc's sub/superscript rule), see parse_star,
        //   parse_thematic_break_underscore, parse_tilde, parse_caret.
        // Consumed as a choice inside `pandoc_str` so the AST shape stays
        // uniform. Note the scanner consumes the whitespace in front of the
        // token, so the node may start with spaces; treesitter.rs splits
        // those back out into a Space inline.
        $._pandoc_literal_str,

        $._pipe_table_delimiter, // so we can distinguish between pipe table | and pandoc_str |

        $._pandoc_line_break, // we need to do this in the external lexer to avoid eating the actual newline.

        // KNOWN LIMITATION: QMD does not support triple-asterisk strong+emph
        // (`***foo***`). The scanner emits this token when it sees `***`
        // immediately followed by non-whitespace content so the parser can
        // raise the user-facing error Q-2-32 ("Triple star emphasis disallowed",
        // see crates/pampa/resources/error-corpus/Q-2-32.json) suggesting the
        // workaround `**_foo_**`. Emission site: scanner.c, EMIT_TOKEN(TRIPLE_STAR).
        // See CONTRIBUTING.md "Known limitations" for the full list.
        $._triple_star_error,

        // KNOWN LIMITATION: QMD does not support Pandoc-style grid tables.
        // The scanner emits a single multi-line GRID_TABLE token that spans
        // the entire grid table (block-quote prefixes stripped) so pampa
        // can surface a structured diagnostic carrying the captured text.
        // Emission site: scanner.c, EMIT_TOKEN(GRID_TABLE) via
        // parse_grid_table_after_first_plus().
        $.grid_table,

        // KNOWN LIMITATION: QMD does not support CommonMark 4-space indented
        // code blocks. The scanner emits this token when leftover indentation
        // (after block-quote / list-item matchers consume their share) is >= 4
        // at a block-start position, so the parser can raise the user-facing
        // error Q-2-35 ("Indented code blocks are not supported",
        // see crates/pampa/resources/error-corpus/Q-2-35.json) suggesting a
        // fenced code block. Emission site: scanner.c,
        // EMIT_TOKEN(INDENTED_CODE_BLOCK_DISALLOWED).
        // See CONTRIBUTING.md "Known limitations" for the full list.
        $._indented_code_block_error,

        // Pipe-table × caption disambiguation (issue #206).
        // The literal `:` first-token of `caption` collides with `:::` at the
        // start of a line that follows a pipe table row: the parser shifts the
        // first `:` as caption-start and then errors on the second `:`. To kill
        // the ambiguity, the scanner emits this token only when `:` is followed
        // by inline whitespace (space, tab, newline, EOF) — NOT another `:` —
        // so `:::` no longer matches caption-start. Emission site: scanner.c,
        // EMIT_TOKEN(CAPTION_START) inside parse_fenced_div_marker.
        $._caption_start,
    ],
    precedences: $ => [],
    extras: $ => [],
});
