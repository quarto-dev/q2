#include "tree_sitter/parser.h"
#include "stdio.h"

#ifdef _MSC_VER
#define UNUSED __pragma(warning(suppress : 4101))
#else
#define UNUSED __attribute__((unused))
#endif

// For explanation of the tokens see grammar.js
//
// NB THESE NEED TO MATCH THE EXTERNS IN GRAMMAR.JS
typedef enum {
    ERROR,
    TRIGGER_ERROR,
    CODE_SPAN_START,
    CODE_SPAN_CLOSE,
    EMPHASIS_OPEN_STAR,
    EMPHASIS_OPEN_UNDERSCORE,
    EMPHASIS_CLOSE_STAR,
    EMPHASIS_CLOSE_UNDERSCORE,
    LAST_TOKEN_WHITESPACE,
    LAST_TOKEN_PUNCTUATION,
    STRIKEOUT_OPEN,
    STRIKEOUT_CLOSE,
    LATEX_SPAN_START,
    LATEX_SPAN_CLOSE,
    SINGLE_QUOTE_OPEN,
    SINGLE_QUOTE_CLOSE,
    DOUBLE_QUOTE_OPEN,
    DOUBLE_QUOTE_CLOSE,
    SUPERSCRIPT_OPEN,
    SUPERSCRIPT_CLOSE,
    SUBSCRIPT_OPEN,
    SUBSCRIPT_CLOSE,
    CITE_AUTHOR_IN_TEXT_WITH_OPEN_BRACKET,
    CITE_SUPPRESS_AUTHOR_WITH_OPEN_BRACKET,
    CITE_AUTHOR_IN_TEXT,
    CITE_SUPPRESS_AUTHOR,
    SHORTCODE_OPEN_ESCAPED,
    SHORTCODE_CLOSE_ESCAPED,
    SHORTCODE_OPEN,
    SHORTCODE_CLOSE,
    KEY_NAME_AND_EQUALS,
    UNCLOSED_SPAN,
    STRONG_EMPHASIS_OPEN_STAR,
    STRONG_EMPHASIS_CLOSE_STAR,
    STRONG_EMPHASIS_OPEN_UNDERSCORE,
    STRONG_EMPHASIS_CLOSE_UNDERSCORE,
    HTML_COMMENT,
} TokenType;

static bool is_lookahead_line_end(TSLexer *lexer) {
    return lexer->lookahead == '\n' || lexer->lookahead == '\r' ||
           lexer->eof(lexer);
}

static bool is_lookahead_whitespace(TSLexer *lexer) {
    return lexer->lookahead == ' ' || lexer->lookahead == '\t' ||
           is_lookahead_line_end(lexer);
}

// Convenience function to emit the error token. This is done to stop invalid
// parse branches. Specifically:
// 1. When encountering a newline after a line break that ended a paragraph, and
// no new block
//    has been opened.
// 2. When encountering a new block after a soft line break.
// 3. When a `$._trigger_error` token is valid, which is used to stop parse
// branches through
//    normal tree-sitter grammar rules.
//
// See also the `$._soft_line_break` and `$._paragraph_end_newline` tokens in
// grammar.js
static bool error(TSLexer *lexer) {
    lexer->result_symbol = ERROR;
    return true;
}

typedef struct {
    // Parser state flags
    uint8_t state;
    uint8_t code_span_delimiter_length;
    uint8_t latex_span_delimiter_length;
    // The number of characters remaining in the current emphasis delimiter
    // run.
    uint8_t num_emphasis_delimiters_left;

    // inside_shortcode stores the count of open shortcodes,
    // and is used to lex string literals differently from markdown Quoted
    // nodes
    uint8_t inside_shortcode;

    uint8_t inside_superscript;
    uint8_t inside_subscript;
    uint8_t inside_strikeout;
    uint8_t inside_single_quote;
    uint8_t inside_double_quote;
    uint8_t inside_latex_span;
    uint8_t inside_code_span;
} Scanner;

// Write the whole state of a Scanner to a byte buffer
static unsigned serialize(Scanner *s, char *buffer) {
    unsigned size = 0;
    buffer[size++] = (char)s->state;
    buffer[size++] = (char)s->code_span_delimiter_length;
    buffer[size++] = (char)s->latex_span_delimiter_length;
    buffer[size++] = (char)s->num_emphasis_delimiters_left;
    buffer[size++] = (char)s->inside_shortcode;
    buffer[size++] = (char)s->inside_superscript;
    buffer[size++] = (char)s->inside_subscript;
    buffer[size++] = (char)s->inside_strikeout;
    buffer[size++] = (char)s->inside_single_quote;
    buffer[size++] = (char)s->inside_double_quote;
    buffer[size++] = (char)s->inside_latex_span;
    buffer[size++] = (char)s->inside_code_span;
    return size;
}

// Read the whole state of a Scanner from a byte buffer
// `serizalize` and `deserialize` should be fully symmetric.
static void deserialize(Scanner *s, const char *buffer, unsigned length) {
    s->state = 0;
    s->code_span_delimiter_length = 0;
    s->latex_span_delimiter_length = 0;
    s->num_emphasis_delimiters_left = 0;
    s->inside_shortcode = 0;
    s->inside_superscript = 0;
    s->inside_subscript = 0;
    s->inside_strikeout = 0;
    s->inside_single_quote = 0;
    s->inside_double_quote = 0;
    s->inside_latex_span = 0;
    s->inside_code_span = 0;
    if (length > 0) {
        size_t size = 0;
        s->state = (uint8_t)buffer[size++];
        s->code_span_delimiter_length = (uint8_t)buffer[size++];
        s->latex_span_delimiter_length = (uint8_t)buffer[size++];
        s->num_emphasis_delimiters_left = (uint8_t)buffer[size++];
        s->inside_shortcode = (uint8_t)buffer[size++];
        s->inside_superscript = (uint8_t)buffer[size++];
        s->inside_subscript = (uint8_t)buffer[size++];
        s->inside_strikeout = (uint8_t)buffer[size++];
        s->inside_single_quote = (uint8_t)buffer[size++];
        s->inside_double_quote = (uint8_t)buffer[size++];
        s->inside_latex_span = (uint8_t)buffer[size++];
        s->inside_code_span = (uint8_t)buffer[size++];
    }
}

static bool parse_leaf_delimiter(TSLexer *lexer, uint8_t *delimiter_length,
                                 const bool *valid_symbols,
                                 const char delimiter,
                                 const TokenType open_token,
                                 const TokenType close_token,
                                 uint8_t *delimiter_state_field) {
    uint8_t level = 0;
    while (lexer->lookahead == delimiter) {
        lexer->advance(lexer, false);
        level++;
    }
    lexer->mark_end(lexer);
    if (level == *delimiter_length && valid_symbols[close_token]) {
        *delimiter_length = 0;
        *delimiter_state_field = 0;
        lexer->result_symbol = close_token;
        return true;
    }
    if (valid_symbols[open_token]) {
        // Parse ahead to check if there is a closing delimiter
        size_t close_level = 0;
        while (!lexer->eof(lexer)) {
            if (lexer->lookahead == delimiter) {
                close_level++;
            } else {
                if (close_level == level) {
                    // Found a matching delimiter
                    break;
                }
                close_level = 0;
            }
            lexer->advance(lexer, false);
        }
        if (close_level == level) {
            *delimiter_length = level;
            *delimiter_state_field = 1;
            lexer->result_symbol = open_token;
            return true;
        }
        if (valid_symbols[UNCLOSED_SPAN]) {
            lexer->result_symbol = UNCLOSED_SPAN;
            return true;
        }
    }
    return false;
}

static bool parse_backtick(Scanner *s, TSLexer *lexer,
                           const bool *valid_symbols) {
    return parse_leaf_delimiter(lexer, &s->code_span_delimiter_length,
                                valid_symbols, '`', CODE_SPAN_START,
                                CODE_SPAN_CLOSE, &s->inside_code_span);
}

static bool parse_dollar(Scanner *s, TSLexer *lexer,
                         const bool *valid_symbols) {
    return parse_leaf_delimiter(lexer, &s->latex_span_delimiter_length,
                                valid_symbols, '$', LATEX_SPAN_START,
                                LATEX_SPAN_CLOSE, &s->inside_latex_span);
}

static bool parse_single_quote(Scanner *s, TSLexer *lexer, const bool *valid_symbols) {
    lexer->advance(lexer, false);
    if (s->inside_single_quote > 0) {
        if (valid_symbols[SINGLE_QUOTE_CLOSE]) {
            lexer->result_symbol = SINGLE_QUOTE_CLOSE;
            s->inside_single_quote = 0;
            return true;
        }
        // HEY do we ever get here?
    }
    lexer->mark_end(lexer);
    if (valid_symbols[SINGLE_QUOTE_CLOSE]) {
        s->inside_single_quote = 0;
        lexer->result_symbol = SINGLE_QUOTE_CLOSE;
        return true;
    }
    if (valid_symbols[SINGLE_QUOTE_OPEN] && !is_lookahead_whitespace(lexer)) {
        s->inside_single_quote = 1;
        lexer->result_symbol = SINGLE_QUOTE_OPEN;
        return true;
    }
    return false;
}

static bool parse_double_quote(Scanner *s, TSLexer *lexer, const bool *valid_symbols) {
    lexer->advance(lexer, false);
    if (s->inside_double_quote > 0) {
        if (valid_symbols[DOUBLE_QUOTE_CLOSE]) {
            lexer->result_symbol = DOUBLE_QUOTE_CLOSE;
            s->inside_double_quote = 0;
            return true;
        }
        // HEY do we ever get here?
    }
    lexer->mark_end(lexer);
    if (valid_symbols[DOUBLE_QUOTE_CLOSE]) {
        s->inside_double_quote = 0;
        lexer->result_symbol = DOUBLE_QUOTE_CLOSE;
        return true;
    }
    if (valid_symbols[DOUBLE_QUOTE_OPEN]) {
        s->inside_double_quote = 1;
        lexer->result_symbol = DOUBLE_QUOTE_OPEN;
        return true;
    }
    return false;
}

static bool parse_caret(Scanner *s, TSLexer *lexer, const bool *valid_symbols) {
    lexer->advance(lexer, false);
    lexer->mark_end(lexer);
    if (lexer->lookahead == '[') {
        return false; // do not lex ^[ as superscript because that's a footnote and we need the token
    }
    if (s->inside_superscript > 0) {
        if (valid_symbols[SUPERSCRIPT_CLOSE]) {
            lexer->result_symbol = SUPERSCRIPT_CLOSE;
            s->inside_superscript = 0;
            return true;
        }
        // HEY do we ever get here?
    }
    if (valid_symbols[SUPERSCRIPT_CLOSE]) {
        s->inside_superscript = 0;
        lexer->result_symbol = SUPERSCRIPT_CLOSE;
        return true;
    }
    if (valid_symbols[SUPERSCRIPT_OPEN]) {
        s->inside_superscript = 1;
        lexer->result_symbol = SUPERSCRIPT_OPEN;
        return true;
    }
    return false;
}

static bool parse_strikeout(Scanner *s, TSLexer *lexer,
                             const bool *valid_symbols) {
    lexer->advance(lexer, false);
    if (s->inside_strikeout > 0) {
        if (valid_symbols[STRIKEOUT_CLOSE]) {
            lexer->result_symbol = STRIKEOUT_CLOSE;
            s->inside_strikeout = 0;
            return true;
        }
        // HEY do we ever get here?
    }
    lexer->mark_end(lexer);
    if (valid_symbols[STRIKEOUT_CLOSE]) {
        s->inside_strikeout = 0;
        lexer->result_symbol = STRIKEOUT_CLOSE;
        return true;
    }
    if (valid_symbols[STRIKEOUT_OPEN]) {
        s->inside_strikeout = 1;
        lexer->result_symbol = STRIKEOUT_OPEN;
        return true;
    }
    return false;
}

static bool parse_tilde(Scanner *s, TSLexer *lexer, const bool *valid_symbols) {
    lexer->advance(lexer, false);    
    if (lexer->lookahead == '~') {
        return parse_strikeout(s, lexer, valid_symbols);
    }
    if (s->inside_subscript > 0) {
        if (valid_symbols[SUBSCRIPT_CLOSE]) {
            lexer->result_symbol = SUBSCRIPT_CLOSE;
            s->inside_subscript = 0;
            return true;
        }
        // HEY do we ever get here?
    }
    if (valid_symbols[SUBSCRIPT_CLOSE]) {
        s->inside_subscript = 0;
        lexer->result_symbol = SUBSCRIPT_CLOSE;
        return true;
    }
    if (valid_symbols[SUBSCRIPT_OPEN]) {
        s->inside_subscript = 1;
        lexer->result_symbol = SUBSCRIPT_OPEN;
        return true;
    }
    return false;
}

static bool parse_star(TSLexer *lexer, const bool *valid_symbols) {
    lexer->advance(lexer, false);
    if (lexer->lookahead == '*') {
        // strong emphasis
        lexer->advance(lexer, false);

        if (valid_symbols[STRONG_EMPHASIS_CLOSE_STAR]) {
            lexer->result_symbol = STRONG_EMPHASIS_CLOSE_STAR;
            return true;
        }
        if (valid_symbols[STRONG_EMPHASIS_OPEN_STAR]) {
            lexer->result_symbol = STRONG_EMPHASIS_OPEN_STAR;
            return true;
        }
        return false;
    }

    if (valid_symbols[EMPHASIS_CLOSE_STAR]) {
        lexer->result_symbol = EMPHASIS_CLOSE_STAR;
        return true;
    }
    if (valid_symbols[EMPHASIS_OPEN_STAR]) {
        lexer->result_symbol = EMPHASIS_OPEN_STAR;
        return true;
    }
    return false;
}

static bool parse_underscore(TSLexer *lexer,
                             const bool *valid_symbols) {
    lexer->advance(lexer, false);
    if (lexer->lookahead == '_') {
        // strong emphasis
        lexer->advance(lexer, false);

        if (valid_symbols[STRONG_EMPHASIS_CLOSE_UNDERSCORE]) {
            lexer->result_symbol = STRONG_EMPHASIS_CLOSE_UNDERSCORE;
            return true;
        }
        if (valid_symbols[STRONG_EMPHASIS_OPEN_UNDERSCORE]) {
            lexer->result_symbol = STRONG_EMPHASIS_OPEN_UNDERSCORE;
            return true;
        }
        return false;
    }

    if (valid_symbols[EMPHASIS_CLOSE_UNDERSCORE]) {
        lexer->result_symbol = EMPHASIS_CLOSE_UNDERSCORE;
        return true;
    }
    if (valid_symbols[EMPHASIS_OPEN_UNDERSCORE]) {
        lexer->result_symbol = EMPHASIS_OPEN_UNDERSCORE;
        return true;
    }
    return false;
}

static bool parse_cite_author_in_text(Scanner *_, TSLexer *lexer,
                                      const bool *valid_symbols) {
    lexer->advance(lexer, false);
    if (lexer->lookahead == '{' && valid_symbols[CITE_AUTHOR_IN_TEXT_WITH_OPEN_BRACKET]) {
        lexer->advance(lexer, false);
        // We have an opening bracket, so we can parse the author in text with
        // brackets.
        lexer->result_symbol = CITE_AUTHOR_IN_TEXT_WITH_OPEN_BRACKET;
        lexer->mark_end(lexer);
        return true;
    } else if (valid_symbols[CITE_AUTHOR_IN_TEXT]) {
        lexer->result_symbol = CITE_AUTHOR_IN_TEXT;
        lexer->mark_end(lexer);
        return true;
    }
    return false;
}

static bool parse_cite_suppress_author(Scanner *_, TSLexer *lexer,
                                       const bool *valid_symbols) {
    lexer->advance(lexer, false);
    if (lexer->lookahead == '@') {
        lexer->advance(lexer, false);
        if (lexer->lookahead == '{' && valid_symbols[CITE_SUPPRESS_AUTHOR_WITH_OPEN_BRACKET]) {
            lexer->advance(lexer, false);
            lexer->result_symbol = CITE_SUPPRESS_AUTHOR_WITH_OPEN_BRACKET;
            lexer->mark_end(lexer);
            return true;
        } else if (valid_symbols[CITE_SUPPRESS_AUTHOR]) {
            lexer->result_symbol = CITE_SUPPRESS_AUTHOR;
            lexer->mark_end(lexer);
            return true;
        }
    }
    return false;
}

static bool parse_shortcode_open(Scanner *s, TSLexer *lexer,
                                 const bool *valid_symbols) {
    lexer->advance(lexer, false);
    if (lexer->lookahead != '{') return false;
    lexer->advance(lexer, false);
    if (lexer->lookahead == '<' && valid_symbols[SHORTCODE_OPEN]) {
        lexer->advance(lexer, false);
        lexer->result_symbol = SHORTCODE_OPEN;
        lexer->mark_end(lexer);
        s->inside_shortcode++;
        return true;
    } else if (lexer->lookahead == '{') {
        lexer->advance(lexer, false);
        if (lexer->lookahead == '<' && valid_symbols[SHORTCODE_OPEN_ESCAPED]) {
            lexer->advance(lexer, false);
            lexer->result_symbol = SHORTCODE_OPEN_ESCAPED;
            lexer->mark_end(lexer);
            s->inside_shortcode++;
            return true;
        }
    }
    return false;
}

static bool parse_shortcode_close(Scanner *s, TSLexer *lexer,
                                  const bool *valid_symbols) {
    lexer->advance(lexer, false);
    if (lexer->lookahead != '}') return false;
    lexer->advance(lexer, false);
    if (lexer->lookahead != '}') return false;
    lexer->advance(lexer, false);
    if (lexer->lookahead == '}' && valid_symbols[SHORTCODE_CLOSE_ESCAPED]) {
        lexer->advance(lexer, false);
        lexer->result_symbol = SHORTCODE_CLOSE_ESCAPED;
        lexer->mark_end(lexer);
        s->inside_shortcode--;
        return true;
    } else if (valid_symbols[SHORTCODE_CLOSE]) {
        lexer->result_symbol = SHORTCODE_CLOSE;
        lexer->mark_end(lexer);
        s->inside_shortcode--;
        return true;
    }
    return false;
}

// Helper: check if character is valid for start of identifier [a-zA-Z_]
static inline bool is_identifier_start(int32_t c) {
    return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || c == '_';
}

// Helper: check if character is valid for identifier continuation [a-zA-Z0-9_-]
static inline bool is_identifier_char(int32_t c) {
    return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') ||
           (c >= '0' && c <= '9') || c == '_' || c == '-';
}

// Parse key_name_and_equals token: identifier [whitespace] =
// This eliminates ambiguity between positional args and keyword params
// Only attempts to parse when valid_symbols[KEY_NAME_AND_EQUALS] is true
static bool parse_key_name_and_equals(UNUSED Scanner *s, TSLexer *lexer,
                                      const bool *valid_symbols) {
    if (!valid_symbols[KEY_NAME_AND_EQUALS]) return false;

    // Must start with identifier start char
    if (!is_identifier_start(lexer->lookahead)) return false;

    // Consume identifier: [a-zA-Z_][a-zA-Z0-9_-]*
    lexer->advance(lexer, false);
    while (is_identifier_char(lexer->lookahead)) {
        lexer->advance(lexer, false);
    }

    // Skip optional whitespace
    while (lexer->lookahead == ' ' || lexer->lookahead == '\t') {
        lexer->advance(lexer, false);
    }

    // Must be followed by '='
    if (lexer->lookahead != '=') return false;

    // Consume the '='
    lexer->advance(lexer, false);
    lexer->mark_end(lexer);
    lexer->result_symbol = KEY_NAME_AND_EQUALS;
    return true;
}

// Parse HTML comment: <!-- ... -->
// This consumes everything from <!-- to --> atomically, including
// newlines and any markdown syntax inside.
static bool parse_html_comment(TSLexer *lexer, const bool *valid_symbols) {
    if (!valid_symbols[HTML_COMMENT]) {
        return false;
    }

    // Current position should be '<'
    if (lexer->lookahead != '<') {
        return false;
    }
    lexer->advance(lexer, false);

    if (lexer->lookahead != '!') {
        return false;
    }
    lexer->advance(lexer, false);

    if (lexer->lookahead != '-') {
        return false;
    }
    lexer->advance(lexer, false);

    if (lexer->lookahead != '-') {
        return false;
    }
    lexer->advance(lexer, false);

    // Now consume everything until we find '-->'
    while (!lexer->eof(lexer)) {
        if (lexer->lookahead == '-') {
            lexer->advance(lexer, false);
            if (lexer->lookahead == '-') {
                lexer->advance(lexer, false);
                if (lexer->lookahead == '>') {
                    lexer->advance(lexer, false);
                    lexer->mark_end(lexer);
                    lexer->result_symbol = HTML_COMMENT;
                    return true;
                }
                // Not the end, continue consuming
            }
            // Continue consuming
        } else {
            lexer->advance(lexer, false);
        }
    }

    // Unclosed comment - consumed until EOF
    lexer->mark_end(lexer);
    lexer->result_symbol = HTML_COMMENT;
    return true;
}

static bool scan(Scanner *s, TSLexer *lexer, const bool *valid_symbols) {
    // A normal tree-sitter rule decided that the current branch is invalid and
    // now "requests" an error to stop the branch
    if (valid_symbols[TRIGGER_ERROR]) {
        return error(lexer);
    }

    // Decide which tokens to consider based on the first non-whitespace
    // character
    switch (lexer->lookahead) {
        case '<':
            // Check for HTML comment
            return parse_html_comment(lexer, valid_symbols);
        case '{':
            return parse_shortcode_open(s, lexer, valid_symbols);
        case '>':
            return parse_shortcode_close(s, lexer, valid_symbols);
        case '@':
            return parse_cite_author_in_text(s, lexer, valid_symbols);
        case '-':
            return parse_cite_suppress_author(s, lexer, valid_symbols);
        case '^':
            return parse_caret(s, lexer, valid_symbols);
        case '`':
            // A backtick could mark the beginning or ending of a code span or a
            // fenced code block.
            return parse_backtick(s, lexer, valid_symbols);
        case '$':
            return parse_dollar(s, lexer, valid_symbols);
        case '*':
            // A star could either mark the beginning or ending of emphasis, a
            // list item or thematic break. This code is similar to the code for
            // '_' and '+'.
            return parse_star(lexer, valid_symbols);
        case '_':
            return parse_underscore(lexer, valid_symbols);
        case '~':
            return parse_tilde(s, lexer, valid_symbols);
    }

    // we only parse single and double quotes if we are not inside a shortcode
    // because those are used for string literals in shortcodes.
    //
    // If we are inside a shortcode, we parse single and double quotes as
    // delimiters of string immediates, instead of normal markdown single and
    // double quotes.
    //
    // this shortcode immediate parsing happens at grammar.js
    if (!s->inside_shortcode && (valid_symbols[LAST_TOKEN_WHITESPACE] || s->inside_single_quote) && lexer->lookahead == '\'') {
        return parse_single_quote(s, lexer, valid_symbols);
    }
    if (!s->inside_shortcode && (valid_symbols[LAST_TOKEN_WHITESPACE] || s->inside_double_quote) && lexer->lookahead == '"') {
        return parse_double_quote(s, lexer, valid_symbols);
    }

    // Try to parse key_name_and_equals when inside shortcode and lookahead is identifier start
    // This must be checked only when the grammar can accept this token (valid_symbols guard)
    if (s->inside_shortcode && is_identifier_start(lexer->lookahead)) {
        return parse_key_name_and_equals(s, lexer, valid_symbols);
    }

    return false;
}

void *tree_sitter_markdown_inline_external_scanner_create() {
    Scanner *s = (Scanner *)malloc(sizeof(Scanner));
    deserialize(s, NULL, 0);
    return s;
}

bool tree_sitter_markdown_inline_external_scanner_scan(
    void *payload, TSLexer *lexer, const bool *valid_symbols) {
    Scanner *scanner = (Scanner *)payload;
    return scan(scanner, lexer, valid_symbols);
}

unsigned tree_sitter_markdown_inline_external_scanner_serialize(void *payload,
                                                                char *buffer) {
    Scanner *scanner = (Scanner *)payload;
    return serialize(scanner, buffer);
}

void tree_sitter_markdown_inline_external_scanner_deserialize(void *payload,
                                                              const char *buffer,
                                                              unsigned length) {
    Scanner *scanner = (Scanner *)payload;
    deserialize(scanner, buffer, length);
}

void tree_sitter_markdown_inline_external_scanner_destroy(void *payload) {
    Scanner *scanner = (Scanner *)payload;
    free(scanner);
}
