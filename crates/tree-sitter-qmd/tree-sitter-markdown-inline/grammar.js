// This grammar only concerns the inline structure of Quarto's dialect of markdown (QMD).
//
// For more information see README.md

const common = require('../common/common');

// key_value defs are in common.js

// Levels used for dynmic precedence. Ideally
// n * PRECEDENCE_LEVEL_EMPHASIS > PRECEDENCE_LEVEL_LINK for any n, so maybe the
// maginuted of these values should be increased in the future
const PRECEDENCE_LEVEL_EMPHASIS = 1;
const PRECEDENCE_LEVEL_LINK = 10;

// Punctuation characters as specified in
// https://github.github.com/gfm/#ascii-punctuation-character
//
// the characters on the right side of the addition are not
// GFM punctuation characters, but are needed in QMD
const PUNCTUATION_CHARACTERS_REGEX = '!-/:-@\\[-`\\{-~' + '\'^';


// !!!
// Notice the call to `add_inline_rules` which generates some additional rules related to parsing
// inline contents in different contexts.
// !!!
module.exports = grammar(add_inline_rules({
    name: 'markdown_inline',

    externals: $ => [
        // NB THESE NEED TO MATCH THE ENUM IN SCANNER.C
        //
        // An `$._error` token is never valid  and gets emmited to kill invalid parse branches. Concretely
        // this is used to decide wether a newline closes a paragraph and together and it gets emitted
        // when trying to parse the `$._trigger_error` token in `$.link_title`.
        $._error,
        $._trigger_error,

        // Opening and closing delimiters for code spans. These are sequences of one or more backticks.
        // An opening token does not mean the text after has to be a code span if there is no closing token
        $._code_span_start,
        $._code_span_close,

        // Opening and closing delimiters for emphasis.
        $._emphasis_open_star,
        $._emphasis_open_underscore,
        $._emphasis_close_star,
        $._emphasis_close_underscore,

        // For emphasis we need to tell the parser if the last character was a whitespace (or the
        // beginning of a line) or a punctuation. These tokens never actually get emitted.
        $._last_token_whitespace,
        $._last_token_punctuation,

        $._strikeout_open,
        $._strikeout_close,

        // Opening and closing delimiters for latex. These are sequences of one or more dollar signs.
        // An opening token does not mean the text after has to be latex if there is no closing token
        $._latex_span_start,
        $._latex_span_close,

        $._single_quote_open,
        $._single_quote_close,
        $._double_quote_open,
        $._double_quote_close,
        $._superscript_open,
        $._superscript_close,
        $._subscript_open,
        $._subscript_close,

        $._cite_author_in_text_with_open_bracket,
        $._cite_suppress_author_with_open_bracket,
        $._cite_author_in_text,
        $._cite_suppress_author,

        $._shortcode_open_escaped,
        $._shortcode_close_escaped,
        $._shortcode_open,
        $._shortcode_close,

        // Token emmited when encountering opening delimiters for a leaf span
        // e.g. a code span, that does not have a matching closing span
        $._unclosed_span
    ],
    precedences: $ => [
        [$._strong_emphasis_star_no_link, $._inline_element_no_star_no_link],
        [$._strong_emphasis_underscore_no_link, $._inline_element_no_underscore_no_link],
        [$.hard_line_break, $._whitespace],
        [$.hard_line_break, $._text_base],
    ],
    // More conflicts are defined in `add_inline_rules`
    conflicts: $ => [

        [$._link_text_non_empty, $._inline_element_no_tilde],
        [$._link_text, $._inline_element_no_tilde],
        [$.link_destination, $.link_title],
        [$._link_destination_parenthesis, $.link_title],
        [$._shortcode_value, $.shortcode_keyword_param],
    ],
    extras: $ => [],

    rules: {
        inline: $ => seq(optional($._last_token_whitespace), $._inline),

        ...common.rules,

        // A lot of inlines are defined in `add_inline_rules`, including:
        //
        // * collections of inlines
        // * emphasis
        // * textual content
        //
        // This is done to reduce code duplication, as some inlines need to be parsed differently
        // depending on the context. For example inlines in ATX headings may not contain newlines.

        code_span: $ => prec.right(seq(
            alias($._code_span_start, $.code_span_delimiter),
            alias(repeat(choice($._code_span_text_base, $._soft_line_break)), $.code_content),
            alias($._code_span_close, $.code_span_delimiter),
            optional($._qmd_attribute)
        )),

        // QMD CHANGE: we call this a latex_span because we need a display math mode
        // in the block grammar and that will be called latex_block
        latex_span: $ => seq(
            alias($._latex_span_start, $.latex_span_delimiter),
            alias(repeat(choice(/[^$\n\\]+/, '[', ']', /[\\]./, $._soft_line_break, $.backslash_escape)), $.latex_content),
            alias($._latex_span_close, $.latex_span_delimiter),
        ),

        insert: $ => seq(
            prec(3, alias(/\[\+\+[ ]*/, $.insert_delimiter)),
            repeat($._inline_element),
            prec(3, alias(/[ ]*\]/, $.insert_delimiter)),
        ),

        delete: $ => seq(
            prec(3, alias(/\[\-\-[ ]*/, $.delete_delimiter)),
            repeat($._inline_element),
            prec(3, alias(/[ ]*\]/, $.delete_delimiter)),
        ),

        highlight: $ => seq(
            prec(3, alias(/\[\!\![ ]*/, $.highlight_delimiter)),
            repeat($._inline_element),
            prec(3, alias(/[ ]*\]/, $.highlight_delimiter)),
        ),

        edit_comment: $ => seq(
            prec(3, alias(/\[>>[ ]*/, $.edit_comment_delimiter)),
            repeat($._inline_element),
            prec(3, alias(/[ ]*\]/, $.edit_comment_delimiter)),
        ),
        
        superscript: $ => seq(
            alias($._superscript_open, $.superscript_delimiter),
            repeat($._inline_element),
            alias($._superscript_close, $.superscript_delimiter),
        ),

        subscript: $ => seq(
            alias($._subscript_open, $.subscript_delimiter),
            repeat($._inline_element),
            alias($._subscript_close, $.subscript_delimiter),
        ),

        strikeout: $ => seq(
            alias($._strikeout_open, $.strikeout_delimiter),
            repeat($._inline_element),
            alias($._strikeout_close, $.strikeout_delimiter),
        ),

        quoted_span: $ => choice(
            prec.right(seq(
                alias($._single_quote_open, $.single_quoted_span_delimiter),
                repeat($._inline_element),
                alias($._single_quote_close, $.single_quoted_span_delimiter),
            )),
            prec.right(seq(
                alias($._double_quote_open, $.double_quoted_span_delimiter),
                repeat($._inline_element),
                alias($._double_quote_close, $.double_quoted_span_delimiter),
            )),
        ),

        inline_note: $ => prec(2, seq(
            alias(seq("^[", optional($._last_token_punctuation)), $.inline_note_delimiter),
            repeat($._inline_element),
            alias(seq("]", optional($._last_token_punctuation)), $.inline_note_delimiter),
        )),
        /*
            From https://pandoc.org/demo/example33/8.20-citation-syntax.html:

            Unless a citation key starts with a letter, digit, or _, and contains only 
            alphanumerics and single internal punctuation characters (:.#$%&-+?<>~/), 
            it must be surrounded by curly braces, which are not considered part of the key.
        */

        // citations are impossible to parse in a context-free manner, so we parse
        // them as terminal nodes and then use a post-processing step taking advantage
        // of the inline_link syntax
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
                alias(new RegExp('[0-9A-Za-z_]+([:.#$%&-+?<>~/][0-9A-Za-z_]+)*'), $.citation_id_author_in_text)
            ),
            seq(alias($._cite_suppress_author, $.citation_delimiter),
                alias(new RegExp('[0-9A-Za-z_]+([:.#$%&-+?<>~/][0-9A-Za-z_]+)*'), $.citation_id_suppress_author)
            ),
        ),

        // shortcodes
        shortcode_escaped: $ => seq(
            alias($._shortcode_open_escaped, $.shortcode_delimiter), // "{{{<",
            $._whitespace,
            $.shortcode_name,
            repeat(seq($._whitespace, $._shortcode_value)),

            repeat(seq($._whitespace, $.shortcode_keyword_param)),
            $._whitespace,
            alias($._shortcode_close_escaped, $.shortcode_delimiter), //">}}}",
        ),

        shortcode: $ => seq(
            alias($._shortcode_open, $.shortcode_delimiter), // "{{<",
            $._whitespace,
            $.shortcode_name,
            repeat(seq($._whitespace, $._shortcode_value)),

            repeat(seq($._whitespace, $.shortcode_keyword_param)),
            $._whitespace,

            alias($._shortcode_close, $.shortcode_delimiter), //">}}",
        ),

        _shortcode_value: $ => choice($.shortcode_name, $.shortcode_naked_string, $.shortcode_string, $.shortcode, $.shortcode_number, $.shortcode_boolean),

        shortcode_name: $ => token(prec(1, new RustRegex("[a-zA-Z_][a-zA-Z0-9_-]*"))),

        // it's extremely convenient to have a naked string that can encode complicated URLs,
        // specifically those with query parameters.
        // Those URLs have an equals sign in the string. 
        // 
        // But we don't want to allow every equals sign in there, because it would interfere with
        // the parsing of shortcode keyword parameters.
        // The solution is to allow a equals signs on naked strings, but only if they come
        // after we've seen a question mark.
        // 
        // This works by a combination of
        //   - longest parse rule
        //   - question marks are not allowed in shortcode key names
        //   - URLs with query parameters have both question marks and equals signs

        shortcode_naked_string: $ => 
            choice(token(prec(1, /(?:[A-Za-z0-9_\-.~:/?#\]@!$&()*+,;]|\[)+/)),
                   token(prec(1, /(?:[A-Za-z0-9_\-.~:/?#\]@!$&()*+,;]|\[)+[?](?:[A-Za-z0-9_\-.~:/?#\]@!$&()*+,;?=]|\[)+/))),

        // shortcode_string: $ => new RegExp("[a-zA-Z_][a-zA-Z0-9_-]*"),
        shortcode_string: $ => choice(
            /'(?:([\\].)|[^'\\\n])*'/,
            /"(?:([\\].)|[^"\\\n])*"/,
        ),
        // // shortcode numbers are numbers as JSON sees them
        // // https://stackoverflow.com/a/13340826
        shortcode_number: $ => token(prec(3, /-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?/)),

        // // shortcode booleans are true or false
        shortcode_boolean: $ => choice(token(prec(2, "true")), token(prec(2, "false"))),

        shortcode_keyword_param: $ => prec.left(prec(2, seq($.shortcode_name, optional($._whitespace), "=", optional($._whitespace), $._shortcode_value))),

        note_reference: $ => seq(
            alias('[^', $.note_reference_delimiter),
            alias(choice($.shortcode_name, $.shortcode_number, $.shortcode_boolean), $.note_reference_id),
            alias(']', $.note_reference_delimiter)),

        // Different kinds of links:
        // * inline links (https://github.github.com/gfm/#inline-link)
        // * full reference links (https://github.github.com/gfm/#full-reference-link)
        // * collapsed reference links (https://github.github.com/gfm/#collapsed-reference-link)
        // * shortcut links (https://github.github.com/gfm/#shortcut-reference-link)
        //
        // Dynamic precedence is distributed as granular as possible to help the parser decide
        // while parsing which branch is the most important.
        //
        // https://github.github.com/gfm/#links
        _link_text: $ => prec.dynamic(PRECEDENCE_LEVEL_LINK, prec.right(choice(
            seq($._link_text_non_empty, optional($._qmd_attribute)),
            seq('[', ']', optional($._qmd_attribute)),
        ))),
        _link_text_non_empty: $ => prec(2, seq('[', alias($._inline, $.link_text), ']')),
        // shortcut_link: $ => prec.dynamic(PRECEDENCE_LEVEL_LINK, $._link_text_non_empty),
        inline_link: $ => prec.dynamic(PRECEDENCE_LEVEL_LINK, prec.right(seq(
            $._link_text,
            optional(seq(
              '(',
              repeat(choice($._whitespace, $._soft_line_break)),
              optional(seq(
                  choice(
                      seq(
                          $.link_destination,
                          optional(seq(
                              repeat1(choice($._whitespace, $._soft_line_break)),
                              $.link_title
                          ))
                      ),
                      $.link_title,
                  ),
                  repeat(choice($._whitespace, $._soft_line_break)),
              )),
              ')',
            )),
            optional($._qmd_attribute)
        ))),

        // Images work exactly like links with a '!' added in front.
        //
        // https://github.github.com/gfm/#images
        image: $ => choice(
            $._image_inline_link,
        ),
        _image_inline_link: $ => prec.dynamic(3 * PRECEDENCE_LEVEL_LINK, prec.right(seq(
            $._image_description,
            '(',
            repeat(choice($._whitespace, $._soft_line_break)),
            optional(seq(
                choice(
                    seq(
                        $.link_destination,
                        optional(seq(
                            repeat1(choice($._whitespace, $._soft_line_break)),
                            $.link_title
                        ))
                    ),
                    $.link_title,
                ),
                repeat(choice($._whitespace, $._soft_line_break)),
            )),
            ')',
            optional($._qmd_attribute)
        ))),
        _image_description: $ => prec.dynamic(3 * PRECEDENCE_LEVEL_LINK, choice($._image_description_non_empty, seq('!', '[', prec(1, ']')))),
        _image_description_non_empty: $ => seq('!', '[', alias($._inline, $.image_description), prec(1, ']')),

        // Autolinks. Uri autolinks actually accept protocolls of arbitrary length which does not
        // align with the spec. This is because the binary for the grammar gets to large if done
        // otherwise as tree-sitters code generation is not very concise for this type of regex.
        //
        // Email autolinks do not match every valid email (emails normally should not be parsed
        // using regexes), but this is how they are defined in the spec.
        //
        // https://github.github.com/gfm/#autolinks
        uri_autolink: $ => /<[a-zA-Z][a-zA-Z0-9+\.\-][a-zA-Z0-9+\.\-]*:[^ \t\r\n<>]*>/,
        email_autolink: $ =>
            /<[a-zA-Z0-9.!#$%&'*+/=?^_`{|}~-]+@[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(?:\.[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)*>/,

        // A hard line break.
        //
        // https://github.github.com/gfm/#hard-line-breaks
        hard_line_break: $ => seq(choice('\\', $._whitespace_ge_2), $._soft_line_break),
        _text: $ => choice($._word, common.punctuation_without($, []), $._whitespace),

        // Whitespace is divided into single whitespaces and multiple whitespaces as wee need this
        // information for hard line breaks.
        _whitespace_ge_2: $ => /\t| [ \t]+/,
        _whitespace: $ => seq(choice($._whitespace_ge_2, / /), optional($._last_token_whitespace)),

        // Other than whitespace we tokenize into strings of digits, punctuation characters
        // (handled by `common.punctuation_without`) and strings of any other characters. This way the
        // lexer does not have to many different states, which makes it a lot easier to make
        // conflicts work.
        _word: $ => choice($._word_no_digit, $._digits),
        _word_no_digit: $ => new RegExp('[^' + PUNCTUATION_CHARACTERS_REGEX + ' \\t\\n\\r0-9]+(_+[^' + PUNCTUATION_CHARACTERS_REGEX + ' \\t\\n\\r0-9]+)*'),
        _digits: $ => /[0-9][0-9_]*/,
        _soft_line_break: $ => seq($._newline_token, optional($._last_token_whitespace)),

        _inline_base: $ => prec.right(repeat1(choice(
            $.image,
            alias($._soft_line_break, $.soft_line_break),
            $.backslash_escape,
            $.hard_line_break,
            $.uri_autolink,
            $.email_autolink,
            $.entity_reference,
            $.numeric_character_reference,
            $.latex_span,
            $.code_span,
            $.quoted_span,
            $.inline_note,
            $.superscript,
            $.strikeout,
            $.subscript,
            $.citation,
            $.shortcode,
            $.shortcode_escaped,
            $.note_reference,
            $.commonmark_attribute,
            $.insert,
            $.delete,
            $.highlight,
            $.edit_comment,

            alias($._text_base, $.text_base),
            $._unclosed_span,
        ))),
        _text_base: $ => prec.right(choice(
            $._word,
            // parse en-dashes, em-dashes and ellipses into a single token
            "--",
            "---",
            "...",
            common.punctuation_without($, ['[', '{', '}', ']', "@"]),
            $._whitespace,
        )),
        _code_span_text_base: $ => prec.right(choice(
            $._word,
            common.punctuation_without($, []),
            $._whitespace,
        )),

        ...(common.EXTENSION_TAGS ? {
            tag: $ => /#[0-9]*[a-zA-Z_\-\/][a-zA-Z_\-\/0-9]*/,
        } : {}),

    },
}));

// This function adds some extra inline rules. This is done to reduce code duplication, as some
// rules may not contain newlines, characters like '*' and '_', ... depending on the context.
//
// This is by far the most ugly part of this code and should be cleaned up.
function add_inline_rules(grammar) {
    let conflicts = [];
    function add_conflict(rules) {
        conflicts.push(rules);
    }
    for (let link of [true, false]) {
        let suffix_link = link ? "" : "_no_link";
        for (let delimiter of [false, "star", "underscore", "tilde"]) {
            let suffix_delimiter = delimiter ? "_no_" + delimiter : "";
            let suffix = suffix_delimiter + suffix_link;
            grammar.rules["_inline_element" + suffix] = $ => {
                let elements = [
                    $._inline_base,
                    alias($['_emphasis_star' + suffix_link], $.emphasis),
                    alias($['_strong_emphasis_star' + suffix_link], $.strong_emphasis),
                    alias($['_emphasis_underscore' + suffix_link], $.emphasis),
                    alias($['_strong_emphasis_underscore' + suffix_link], $.strong_emphasis),
                ];
                if (delimiter !== "star") {
                    elements.push($._emphasis_open_star);
                }
                if (delimiter !== "underscore") {
                    elements.push($._emphasis_open_underscore);
                }
                if (link) {
                    elements = elements.concat([
                        $.inline_link,
                    ]);
                }
                return choice(...elements);
            };
            grammar.rules["_inline" + suffix] = $ => repeat1($["_inline_element" + suffix]);
            if (delimiter !== "star") {
                add_conflict(['_emphasis_star' + suffix_link, '_inline_element' + suffix_delimiter + suffix_link]);
                add_conflict(['_emphasis_star' + suffix_link, '_strong_emphasis_star' + suffix_link, '_inline_element' + suffix_delimiter + suffix_link]);
            }
            if ((delimiter == 'star' || delimiter == 'underscore') && (suffix_link !== '_no_link')) {
                add_conflict(['_strong_emphasis_' + delimiter + suffix_link, '_inline_element_no_' + delimiter]);
            }
            if (delimiter !== "underscore") {
                add_conflict(['_emphasis_underscore' + suffix_link, '_inline_element' + suffix_delimiter + suffix_link]);
                add_conflict(['_emphasis_underscore' + suffix_link, '_strong_emphasis_underscore' + suffix_link, '_inline_element' + suffix_delimiter + suffix_link]);
            }
        }

        grammar.rules['_emphasis_star' + suffix_link] = $ => prec.dynamic(PRECEDENCE_LEVEL_EMPHASIS, seq(alias($._emphasis_open_star, $.emphasis_delimiter), optional($._last_token_punctuation), $['_inline' + '_no_star' + suffix_link], alias($._emphasis_close_star, $.emphasis_delimiter)));
        grammar.rules['_strong_emphasis_star' + suffix_link] = $ => prec.dynamic(2 * PRECEDENCE_LEVEL_EMPHASIS, seq(alias($._emphasis_open_star, $.emphasis_delimiter), $['_emphasis_star' + suffix_link], alias($._emphasis_close_star, $.emphasis_delimiter)));
        grammar.rules['_emphasis_underscore' + suffix_link] = $ => prec.dynamic(PRECEDENCE_LEVEL_EMPHASIS, seq(alias($._emphasis_open_underscore, $.emphasis_delimiter), optional($._last_token_punctuation), $['_inline' + '_no_underscore' + suffix_link], alias($._emphasis_close_underscore, $.emphasis_delimiter)));
        grammar.rules['_strong_emphasis_underscore' + suffix_link] = $ => prec.dynamic(2 * PRECEDENCE_LEVEL_EMPHASIS, seq(alias($._emphasis_open_underscore, $.emphasis_delimiter), $['_emphasis_underscore' + suffix_link], alias($._emphasis_close_underscore, $.emphasis_delimiter)));
    }

    let old = grammar.conflicts
    grammar.conflicts = $ => {
        let cs = old($);
        for (let conflict of conflicts) {
            let c = [];
            for (let rule of conflict) {
                c.push($[rule]);
            }
            cs.push(c);
        }
        return cs;
    }


    return grammar;
}
