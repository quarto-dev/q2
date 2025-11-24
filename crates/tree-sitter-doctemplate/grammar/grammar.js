// deno-lint-ignore-file
/**
 * @file Tree-sitter grammar for Pandoc document templates
 * @author Posit, PBC
 * @license MIT
 *
 * Pandoc template syntax reference:
 * https://pandoc.org/MANUAL.html#templates
 *
 * Key syntax elements:
 * - Variables: $variable$ or ${variable}
 * - Conditionals: $if(variable)$...$endif$, $else$, $elseif(...)$
 * - Loops: $for(variable)$...$endfor$, $sep$
 * - Partials: $partial("filename")$
 * - Pipes: $variable | filter$
 * - Escaped dollar: $$
 */

function w($) {
  return optional($._whitespace);
}
// const w = ($) => ($ => optional($._whitespace)($));

module.exports = grammar({
  name: "doctemplate",

  rules: {
    // TODO: You will implement the grammar rules
    // This is a placeholder that parses any document as a single text node
    template: ($) => $._content,

    _content: ($) => repeat1($.template_element),

    // Plain text (anything not starting a template element)
    text: ($) => /[^$]+/,
    escaped_dollar: ($) => "$$",
    comment: ($) => /\$\-\-[^\n]+/,
    _whitespace: ($) => /[ \t]+/,
    variable_name: ($) => /[A-Za-z][A-Za-z0-9._-]*/,
    partial_array_separator: ($) => /[^$\]]+/,
    nesting: ($) => "$^$",

    pipe_left: ($) => seq("left", $._whitespace, 
        alias(/[0-9]+/, $.n), $._whitespace, 
        seq("\"", alias(/([^"]|\\")*/, $.leftborder), "\""), $._whitespace,
        seq("\"", alias(/([^"]|\\")*/, $.rightborder), "\"")),
    pipe_center: ($) => seq("center", $._whitespace, 
        alias(/[0-9]+/, $.n), $._whitespace, 
        seq("\"", alias(/([^"]|\\")*/, $.leftborder), "\""), $._whitespace,
        seq("\"", alias(/([^"]|\\")*/, $.rightborder), "\"")),
    pipe_right: ($) => seq("right", $._whitespace, 
        alias(/[0-9]+/, $.n), $._whitespace, 
        seq("\"", alias(/([^"]|\\")*/, $.leftborder), "\""), $._whitespace,
        seq("\"", alias(/([^"]|\\")*/, $.rightborder), "\"")),

    pipe: ($) => choice(
      alias("pairs", $.pipe_pairs),
      alias("first", $.pipe_first),
      alias("last", $.pipe_last),
      alias("rest", $.pipe_rest),
      alias("allbutlast", $.pipe_allbutlast),
      alias("uppercase", $.pipe_uppercase),
      alias("lowercase", $.pipe_lowercase),
      alias("length", $.pipe_length),
      alias("reverse", $.pipe_reverse),
      alias("chomp", $.pipe_chomp),
      alias("nowrap", $.pipe_nowrap),
      alias("alpha", $.pipe_alpha),
      alias("roman", $.pipe_roman),
      $.pipe_left,
      $.pipe_center,
      $.pipe_right
    ),

    partial_name: ($) => /[A-Za-z0-9/\\_.-]+/,
    partial: ($) => seq($.partial_name, "()"),

    literal_separator: ($) => /[^$\]]+/,

    _interpolation: ($) => choice(
      seq(w($), $.variable_name, repeat(seq("/", $.pipe)), w($), optional(seq("[", $.literal_separator, "]")), w($)),
      seq(w($), $.variable_name, seq(":", $.partial), optional(seq("[", $.literal_separator, "]")), repeat(seq("/", $.pipe)), w($)),
    ),

    interpolation: ($) => choice(
      seq("$",  $._interpolation, "$"),
      seq("${", $._interpolation, "}"),
    ),

    conditional_condition: ($) => seq("(", w($), $.variable_name, w($), ")"),
    _conditional_elseif_1: ($) => prec.right(seq("$",  w($), "elseif", w($), $.conditional_condition, w($), "$", $._content)),
    _conditional_elseif_2: ($) => prec.right(seq("${", w($), "elseif", w($), $.conditional_condition, w($), "}", $._content)),

    conditional: ($) => choice(
      seq(
        "$", w($), "if", w($), $.conditional_condition, w($), "$", 
        alias($._content, $.conditional_then), 
        repeat(alias($._conditional_elseif_1, $.conditional_elseif)),
        optional(seq("$", w($), "else", w($), "$", alias($._content, $.conditional_else))),
        "$endif$"
      ),
      seq(
        "${", w($), "if", w($), $.conditional_condition, w($), "}", 
        alias($._content, $.conditional_then), 
        repeat(alias($._conditional_elseif_2, $.conditional_elseif)),
        optional(seq("${", w($), "else", w($), "}", alias($._content, $.conditional_else))),
        "${", w($), "endif", w($), "}"
      )
    ),

    forloop: ($) => choice(
      seq(
        "$", w($), "for", w($), "(", alias($.variable_name, $.forloop_variable), ")", w($), "$",
        alias($._content, $.forloop_content),
        optional(seq("$", w($), "sep", w($), "$", alias($._content, $.forloop_separator))),
        "$", w($), "endfor", w($), "$"
      ),
      seq(
        "${", w($), "for", w($), "(", alias($.variable_name, $.forloop_variable), ")", w($), "}",
        alias($._content, $.forloop_content), 
        optional(seq("${", w($), "sep", w($), "}", alias($._content, $.forloop_separator))),
        "${", w($), "endfor", w($), "}"
      ),
    ),

    breakable_block: ($) => prec.right(seq(
      "$~$", $._content, "$~$")),

    template_element: ($) => choice(
      $.text,
      $.escaped_dollar,
      $.comment,
      $.interpolation,
      $.conditional,
      $.forloop,
      $.partial,
      $.breakable_block,
      $.nesting,
    ),
  },
});
