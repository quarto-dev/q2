/// <reference types="tree-sitter-cli/dsl" />

// TODO: we should remove all of these. qmd will not have support for extension choices.

module.exports.EXTENSION_DEFAULT = false;
module.exports.EXTENSION_GFM = false;
module.exports.EXTENSION_TASK_LIST = false; // process.env.EXTENSION_TASK_LIST || module.exports.EXTENSION_GFM || process.env.ALL_EXTENSIONS;
module.exports.EXTENSION_STRIKETHROUGH = true; // process.env.EXTENSION_STRIKETHROUGH || module.exports.EXTENSION_GFM || process.env.ALL_EXTENSIONS;
module.exports.EXTENSION_PIPE_TABLE = true; // process.env.EXTENSION_PIPE_TABLE || module.exports.EXTENSION_GFM || process.env.ALL_EXTENSIONS;
module.exports.EXTENSION_MINUS_METADATA = true;
module.exports.EXTENSION_PLUS_METADATA = false; // maybe we'll change this in the future
module.exports.EXTENSION_TAGS = false; // process.env.EXTENSION_TAGS || process.env.ALL_EXTENSIONS;
module.exports.EXTENSION_LATEX = true; // process.env.EXTENSION_LATEX || module.exports.EXTENSION_DEFAULT || process.env.ALL_EXTENSIONS;

const PUNCTUATION_CHARACTERS_REGEX = '!-/:-@\\[-`\\{-~';
const PUNCTUATION_CHARACTERS_ARRAY = [
    '!', '"', '#', '$', '%', '&', "'", '(', ')', '*', '+', ',', '-', '.', '/', ':', ';', '<',
    '=', '>', '?', '@', '[', '\\', ']', '^', '_', '`', '{', '|', '}', '~'
];

const PRECEDENCE_LEVEL_LINK = 10;

module.exports.PRECEDENCE_LEVEL_LINK = PRECEDENCE_LEVEL_LINK;

module.exports.PUNCTUATION_CHARACTERS_REGEX = PUNCTUATION_CHARACTERS_REGEX;

/** @type {Record<string, ($: GrammarSymbols<any>) => RuleOrLiteral>} */
module.exports.rules = {

    // A backslash escape. This can often be part of different nodes like link labels
    //
    // https://github.github.com/gfm/#backslash-escapes
    backslash_escape: $ => $._backslash_escape,
    _backslash_escape: $ => new RegExp('\\\\[' + PUNCTUATION_CHARACTERS_REGEX + ']'),

    // HTML entity and numeric character references.
    //
    // The regex for entity references are build from the html_entities.json file.
    //
    // https://github.github.com/gfm/#entity-and-numeric-character-references
    entity_reference: $ => html_entity_regex(),
    numeric_character_reference: $ => /&#([0-9]{1,7}|[xX][0-9a-fA-F]{1,6});/,

    link_destination: $ => prec.dynamic(PRECEDENCE_LEVEL_LINK, choice(
        seq('<', repeat(choice($._text_no_angle, $.backslash_escape, $.entity_reference, $.numeric_character_reference)), '>'),
        seq(
            choice( // first character is not a '<'
                $._word,
                punctuation_without($, ['<', '(', ')']),
                $.backslash_escape,
                $.entity_reference,
                $.numeric_character_reference,
                $._link_destination_parenthesis
            ),
            repeat(choice(
                $._word,
                punctuation_without($, ['(', ')']),
                $.backslash_escape,
                $.entity_reference,
                $.numeric_character_reference,
                $._link_destination_parenthesis
            )),
        )
    )),
    _link_destination_parenthesis: $ => seq('(', repeat(choice(
        $._word,
        punctuation_without($, ['(', ')']),
        $.backslash_escape,
        $.entity_reference,
        $.numeric_character_reference,
        $._link_destination_parenthesis
    )), ')'),
    _text_no_angle: $ => choice($._word, punctuation_without($, ['<', '>']), $._whitespace),
    link_title: $ => choice(
        seq('"', repeat(choice(
            $._word,
            punctuation_without($, ['"']),
            $._whitespace,
            $.backslash_escape,
            $.entity_reference,
            $.numeric_character_reference,
            seq($._soft_line_break, optional(seq($._soft_line_break, $._trigger_error)))
        )), '"'),
        seq("'", repeat(choice(
            $._word,
            punctuation_without($, ["'"]),
            $._whitespace,
            $.backslash_escape,
            $.entity_reference,
            $.numeric_character_reference,
            seq($._soft_line_break, optional(seq($._soft_line_break, $._trigger_error)))
        )), "'"),
        seq('(', repeat(choice(
            $._word,
            punctuation_without($, ['(', ')']),
            $._whitespace,
            $.backslash_escape,
            $.entity_reference,
            $.numeric_character_reference,
            seq($._soft_line_break, optional(seq($._soft_line_break, $._trigger_error)))
        )), ')'),
    ),

    _newline_token: $ => /\n|\r\n?/,

    // posit markdown extension: attributes
    _qmd_attribute: $ => choice(
      $.language_attribute,
      $.raw_attribute,
      $.commonmark_attribute
    ),
    // we also allow "<" in the raw specifier,
    // which is a Quarto-specific extension to allow "custom readers"
    raw_specifier: $ => /[<=][a-zA-Z_][a-zA-Z0-9_-]*/,
    _commonmark_whitespace: $ => /[ \t]+/,
    raw_attribute: $ => prec(2, seq(
      "{",
      optional($._commonmark_whitespace),
      $.raw_specifier,
      optional($._commonmark_whitespace),
      "}"
    )),
    commonmark_name: $ => token(prec(1, /[a-zA-Z_][a-zA-Z0-9_\-.]*/)),
    id_specifier: $ => /[#][a-zA-Z0-9_\-.]+/,
    // we're slightly more permissive here than pandoc, which
    // requires class specifiers to start with a letter
    class_specifier: $ => /[.][a-zA-Z0-9_\-][a-zA-Z0-9_\-.]*/,

    commonmark_attribute: $ => prec(3, prec.dynamic(2, seq(
      "{",
      optional($._commonmark_whitespace),
      repeat(seq($.id_specifier, optional($._commonmark_whitespace))),
      repeat(seq($.class_specifier, optional($._commonmark_whitespace))),
      repeat(seq(alias($._attribute, $.key_value_specifier), optional($._commonmark_whitespace))),
      "}"
    ))),
    language_attribute: $ => prec.dynamic(3, seq(
      "{",
      choice(
        alias($.commonmark_name, $.language),
        $.language_attribute
      ),
      "}"
    )),

    _attribute: $ => seq(
      // the choice here is an important hack that allows us to intentionally introduce a conflict that will be resolved later
      alias(choice($.key_value_key, $.commonmark_name), $.key_value_key),
      optional($._commonmark_whitespace), 
      '=', 
      optional($._commonmark_whitespace), 
      $.key_value_value),
    key_value_key: $ => /[a-zA-Z_][a-zA-Z0-9_\.:\-]*/,
    key_value_value: $ => choice(
        /[^ \t\r\n}"'=<>`]+/,
        seq("'", repeat(choice($._word, $._commonmark_whitespace, $._soft_line_break, "\\'", punctuation_without($, ["'"]))), "'"),
        seq('"', repeat(choice($._word, $._commonmark_whitespace, $._soft_line_break, "\\\"", punctuation_without($, ['"']))), '"'),
    ),

};

// Returns a rule that matches all characters that count as punctuation inside markdown, besides
// a list of excluded punctuation characters. Calling this function with a empty list as the second
// argument returns a rule that matches all punctuation.
function punctuation_without($, chars) {
    return seq(choice(...PUNCTUATION_CHARACTERS_ARRAY.filter(c => !chars.includes(c))), optional($._last_token_punctuation));
}

module.exports.punctuation_without = punctuation_without;

// Constructs a regex that matches all html entity references.
function html_entity_regex() {
    // A file with all html entities, should be kept up to date with
    // https://html.spec.whatwg.org/multipage/entities.json
    let html_entities = require("./html_entities.json");
    let s = '&(';
    s += Object.keys(html_entities).map(name => name.substring(1, name.length - 1)).join('|');
    s += ');';
    return new RegExp(s);
}
