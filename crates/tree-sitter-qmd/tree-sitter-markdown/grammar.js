const common = require('../common/common');

// see tree-sitter-markdown/scripts/unicode-ranges.py
// Sm (Math Symbol) - excluding <, >, |, and ~
const PANDOC_VALID_MATH_SYMBOLS =
    "\u{002B}\u{003D}\u{00AC}\u{00B1}\u{00D7}\u{00F7}\u{03F6}\u{0606}-\u{0608}\u{2044}\u{2052}"
  + "\u{207A}-\u{207C}\u{208A}-\u{208C}\u{2118}\u{2140}-\u{2144}\u{214B}\u{2190}-\u{2194}\u{219A}-\u{219B}\u{21A0}\u{21A3}\u{21A6}"
  + "\u{21AE}\u{21CE}-\u{21CF}\u{21D2}\u{21D4}\u{21F4}-\u{22FF}\u{2320}-\u{2321}\u{237C}\u{239B}-\u{23B3}\u{23DC}-\u{23E1}\u{25B7}"
  + "\u{25C1}\u{25F8}-\u{25FF}\u{266F}\u{27C0}-\u{27C4}\u{27C7}-\u{27E5}\u{27F0}-\u{27FF}\u{2900}-\u{2982}\u{2999}-\u{29D7}\u{29DC}-\u{29FB}\u{29FE}-\u{2AFF}"
  + "\u{2B30}-\u{2B44}\u{2B47}-\u{2B4C}\u{FB29}\u{FE62}\u{FE64}-\u{FE66}\u{FF0B}\u{FF1C}-\u{FF1E}\u{FF5C}\u{FF5E}\u{FFE2}"
  + "\u{FFE9}-\u{FFEC}\u{1D6C1}\u{1D6DB}\u{1D6FB}\u{1D715}\u{1D735}\u{1D74F}\u{1D76F}\u{1D789}\u{1D7A9}"
  + "\u{1D7C3}\u{1EEF0}-\u{1EEF1}";

// Sk (Modifier Symbol) - excluding ^ and `
const PANDOC_VALID_MODIFIER_SYMBOLS =
    "\u{00A8}\u{00AF}\u{00B4}\u{00B8}\u{02C2}-\u{02C5}\u{02D2}-\u{02DF}\u{02E5}-\u{02EB}\u{02ED}\u{02EF}-\u{02FF}\u{0375}"
  + "\u{0384}-\u{0385}\u{0888}\u{1FBD}\u{1FBF}-\u{1FC1}\u{1FCD}-\u{1FCF}\u{1FDD}-\u{1FDF}\u{1FED}-\u{1FEF}\u{1FFD}-\u{1FFE}\u{309B}-\u{309C}\u{A700}-\u{A716}"
  + "\u{A720}-\u{A721}\u{A789}-\u{A78A}\u{AB5B}\u{AB6A}-\u{AB6B}\u{FBB2}-\u{FBC2}\u{FF3E}\u{FF40}\u{FFE3}\u{1F3FB}-\u{1F3FF}";

// Sc (Currency Symbol) - excluding $ and fullwidth variants
const PANDOC_VALID_CURRENCY_SYMBOLS =
    "\u{00A2}\u{00A3}\u{00A4}\u{00A5}\u{058F}\u{060B}\u{07FE}\u{07FF}\u{09F2}\u{09F3}\u{09FB}\u{0AF1}\u{0BF9}\u{0E3F}\u{17DB}"
  + "\u{20A0}\u{20A1}\u{20A2}\u{20A3}\u{20A4}\u{20A5}\u{20A6}\u{20A7}\u{20A8}\u{20A9}\u{20AA}\u{20AB}\u{20AC}\u{20AD}\u{20AE}\u{20AF}\u{20B1}\u{20B2}\u{20B3}\u{20B4}\u{20B5}\u{20B6}\u{20B7}\u{20B8}\u{20B9}\u{20BA}\u{20BB}\u{20BC}\u{20BD}\u{20BE}\u{20BF}\u{20C0}"
  + "\u{A838}\u{FDFC}\u{FE69}\u{FF04}\u{FFE0}\u{FFE1}\u{FFE5}\u{FFE6}"
  + "\u{11FDD}\u{11FDE}\u{11FDF}\u{11FE0}\u{1E2FF}\u{1ECB0}";

// So (Other Symbol) - can use as-is, no exclusions needed
// Combined
const PANDOC_VALID_SYMBOLS =
    PANDOC_VALID_MATH_SYMBOLS +
    PANDOC_VALID_MODIFIER_SYMBOLS +
    "\\p{So}" +
    PANDOC_VALID_CURRENCY_SYMBOLS;

const PANDOC_ALPHA_NUM = "0-9A-Za-z\\p{L}\\p{N}";
const PANDOC_PUNCTUATION = "\\p{Pd}#%&()/:+\\u{2026}";
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

const regexBracket = (str) => `(?:${str})`;
const regexOr = (...groups) => regexBracket(groups.join("|"));

const startStrRegex = regexOr(
    "[\\u{00A0}" + PANDOC_ALPHA_NUM + PANDOC_SMART_QUOTES + "-]",
    "[" + PANDOC_VALID_SYMBOLS + "]"); 
const afterUnderscoreRegex = "[" + PANDOC_ALPHA_NUM + "]";

// Thanks, Claude
const EMOJI_REGEX = "(\\p{Extended_Pictographic}(\\p{Emoji_Modifier}|\uFE0F)?(\u200D\\p{Extended_Pictographic}(\\p{Emoji_Modifier}|\uFE0F)?)*)";

const PANDOC_REGEX_STR =
        regexOr(
            "\\\\.",
            EMOJI_REGEX,
            "[" + PANDOC_PUNCTUATION + "]",
            startStrRegex +
            regexOr(
                "[!,.;?\\u{00A0}" + PANDOC_ALPHA_NUM + PANDOC_SMART_QUOTES + "-]",
                // "\\\\.",
                "[" + PANDOC_VALID_SYMBOLS + "]",
                "['\\u{2018}\\u{2019}][\\p{L}\\p{N}]",
                regexBracket("[_]" + afterUnderscoreRegex)
            ) + "*");

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
            $.caption,

            prec(-1, alias($.minus_metadata, $.metadata)),

            $.note_definition_fenced_block,
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
            ":",
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
            /[^$ \t\n]([ \t]*[^$ \t\n]+|\\\$)*/,
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
                    /[^`]+/,
                    /[`]/
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
            optional(choice(
                $.raw_specifier,
                $.language_specifier,
                $.commonmark_specifier,
                alias($._commonmark_specifier_start_with_class, $.commonmark_specifier),
                alias($._commonmark_specifier_start_with_kv, $.commonmark_specifier)
            )),
            '}'
        ),

        _pandoc_attr_specifier: $ => seq(
            '{',
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
            $._language_specifier_token,
            seq('{', $.language_specifier, '}')
        ),

        commonmark_specifier: $ => prec.right(seq(
            optional($._inline_whitespace),
            alias(/[#][._A-Za-z0-9-]+/, $.attribute_id),
            optional(
                seq($._inline_whitespace, 
                    choice(
                        $._commonmark_specifier_start_with_class, 
                        $._commonmark_specifier_start_with_kv))),
            optional($._inline_whitespace),
        )),

        _commonmark_specifier_start_with_class: $ => prec.right(seq(
            alias(/[.][A-Za-z][A-Za-z0-9_.-]*/, $.attribute_class),
            optional(repeat(seq($._inline_whitespace, alias(/[.][A-Za-z][A-Za-z0-9_-]*/, $.attribute_class)))),
            optional(seq($._inline_whitespace, $._commonmark_specifier_start_with_kv)),
        )),

        _commonmark_specifier_start_with_kv: $ => prec.right(seq(
            alias($._commonmark_key_value_specifier, $.key_value_specifier),
            optional(repeat(seq(optional($._inline_whitespace), alias($._commonmark_key_value_specifier, $.key_value_specifier)))),
            optional($._inline_whitespace)
        )),

        _commonmark_key_value_specifier: $ => seq(
            alias($._key_specifier_token, $.key_value_key),
            optional($._inline_whitespace),
            '=',
            optional($._inline_whitespace),
            alias(choice($._value_specifier_token, $._commonmark_single_quote_string, "''", '""', $._commonmark_double_quote_string), $.key_value_value)
        ),

        _commonmark_naked_value: $ => /[A-Za-z0-9_-]+/,
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

            $._prose_punctuation,
            $.html_element,
            alias($._pandoc_line_break, $.pandoc_line_break),
            alias($._pandoc_attr_specifier, $.attribute_specifier),
        ),

        // shortcodes
        shortcode_escaped: $ => seq(
            alias($._shortcode_open_escaped, $.shortcode_delimiter), // "{{{<",
            $._whitespace,
            $.shortcode_name,
            repeat(seq($._whitespace, $._shortcode_value)),

            repeat(seq($._whitespace, alias($._commonmark_key_value_specifier, $.key_value_specifier))),
            $._whitespace,
            alias($._shortcode_close_escaped, $.shortcode_delimiter), //">}}}",
        ),

        shortcode: $ => seq(
            alias($._shortcode_open, $.shortcode_delimiter), // "{{<",
            $._whitespace,
            $.shortcode_name,
            repeat(seq($._whitespace, $._shortcode_value)),

            repeat(seq($._whitespace, alias($._shortcode_key_value_specifier, $.key_value_specifier))),
            $._whitespace,

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

        // we want these to allow :, /, etc to make it possible to put URLs as naked strings
        shortcode_naked_string: $ => 
            choice(token(prec(1, /(?:[A-Za-z0-9_.~:/?#\]@!$%&()+,;-]|\[)+/)),
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

        inline_note: $ => prec(2, seq(
            alias($._inline_note_start_token, $.inline_note_delimiter),
            optional($._inline_whitespace),
            $._inlines,
            optional($._inline_whitespace),
            alias(/[\t ]*[\]]/, $.inline_note_delimiter),
        )),

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

        // Things that are parsed directly as a pandoc str
        pandoc_str: $ => choice(new RegExp(PANDOC_REGEX_STR, 'u'), '|'),
        _prose_punctuation: $ => alias(/[.,;!?]+/, $.pandoc_str),

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
        _list_item_plus: $ => seq(
            $.list_marker_plus,
            optional($.block_continuation),
            $._list_item_content,
            $._block_close,
            optional($.block_continuation)
        ),
        _list_item_minus: $ => seq(
            $.list_marker_minus,
            optional($.block_continuation),
            $._list_item_content,
            $._block_close,
            optional($.block_continuation)
        ),
        _list_item_star: $ => seq(
            $.list_marker_star,
            optional($.block_continuation),
            $._list_item_content,
            $._block_close,
            optional($.block_continuation)
        ),
        _list_item_dot: $ => seq(
            $.list_marker_dot,
            optional($.block_continuation),
            $._list_item_content,
            $._block_close,
            optional($.block_continuation)
        ),
        _list_item_parenthesis: $ => seq(
            $.list_marker_parenthesis,
            optional($.block_continuation),
            $._list_item_content,
            $._block_close,
            optional($.block_continuation)
        ),
        _list_item_example: $ => seq(
            $.list_marker_example,
            optional($.block_continuation),
            $._list_item_content,
            $._block_close,
            optional($.block_continuation)
        ),
        // List items are closed after two consecutive blank lines
        _list_item_content: $ => choice(
            prec(1, seq(
                $._blank_line,
                $._blank_line,
                $._close_block,
                optional($.block_continuation)
            )),
            repeat1($._block),
        ),

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
          choice(alias($._commonmark_naked_value, $.info_string), alias($._pandoc_attr_specifier, $.attribute_specifier)),
          $._newline,
          repeat($._block),
          optional(seq($._fenced_div_end, $._close_block, choice($._newline, $._eof))),
          $._block_close,
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
            repeat($._block),
            optional(seq($._fenced_div_end, $._close_block, choice($._newline, $._eof))),
            $._block_close,
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

        _soft_line_break: $ => seq(
            $._soft_line_ending,
            optional($.block_continuation)
        ),

        // pandoc_line_break: $ => seq(/\\/, choice($._newline, $._eof)),

        _inline_whitespace: $ => choice($._whitespace, $._soft_line_break),
        _whitespace: $ => /[ \t]+/,
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

        $.minus_metadata,

        $._pipe_table_start,
        $._pipe_table_line_ending,

        $._fenced_div_start,
        $._fenced_div_end,

        $.ref_id_specifier,
        $.fenced_div_note_id,

        // code span delimiters for parsing pipe table cells
        $._code_span_start,
        $._code_span_close,

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

        $._pipe_table_delimiter, // so we can distinguish between pipe table | and pandoc_str |

        $._pandoc_line_break, // we need to do this in the external lexer to avoid eating the actual newline.
    ],
    precedences: $ => [],
    extras: $ => [],
});
