#include "tree_sitter/parser.h"
#include <assert.h>
#include <ctype.h>
#include <string.h>
#include <wctype.h>

// set this define to turn on debugging printouts
#define SCAN_DEBUG 1

#ifdef SCAN_DEBUG
#define DEBUG_HERE printf("At line %d\n", __LINE__);
#endif

#ifdef SCAN_DEBUG
#include <stdio.h>
#endif

// For explanation of the tokens see grammar.js
typedef enum {
    LINE_ENDING,
    SOFT_LINE_ENDING,
    BLOCK_CLOSE,
    BLOCK_CONTINUATION,
    BLOCK_QUOTE_START,
    INDENTED_CHUNK_START,
    ATX_H1_MARKER,
    ATX_H2_MARKER,
    ATX_H3_MARKER,
    ATX_H4_MARKER,
    ATX_H5_MARKER,
    ATX_H6_MARKER,
    SETEXT_H1_UNDERLINE,
    SETEXT_H2_UNDERLINE,
    THEMATIC_BREAK,
    LIST_MARKER_MINUS,
    LIST_MARKER_PLUS,
    LIST_MARKER_STAR,
    LIST_MARKER_PARENTHESIS,
    LIST_MARKER_DOT,
    LIST_MARKER_MINUS_DONT_INTERRUPT,
    LIST_MARKER_PLUS_DONT_INTERRUPT,
    LIST_MARKER_STAR_DONT_INTERRUPT,
    LIST_MARKER_PARENTHESIS_DONT_INTERRUPT,
    LIST_MARKER_DOT_DONT_INTERRUPT,
    LIST_MARKER_EXAMPLE,
    LIST_MARKER_EXAMPLE_DONT_INTERRUPT,
    FENCED_CODE_BLOCK_START_BACKTICK,
    FENCED_CODE_BLOCK_START_TILDE,
    BLANK_LINE_START,
    FENCED_CODE_BLOCK_END_BACKTICK,
    FENCED_CODE_BLOCK_END_TILDE,
    CLOSE_BLOCK,
    NO_INDENTED_CHUNK,
    ERROR,
    TRIGGER_ERROR,
    TOKEN_EOF,
    MINUS_METADATA,
    PLUS_METADATA,
    PIPE_TABLE_START,
    PIPE_TABLE_LINE_ENDING,
    FENCED_DIV_START,
    FENCED_DIV_END,
    REF_ID_SPECIFIER,
    FENCED_DIV_NOTE_ID,

    // special tokens to trigger serialization to track in-display-math mode
    DISPLAY_MATH_STATE_TRACK_MARKER,
    INLINE_MATH_STATE_TRACK_MARKER,
    // code span delimiters for parsing pipe table cells
    CODE_SPAN_START,
    CODE_SPAN_CLOSE,
    // latex span delimiters for parsing pipe table cells
    LATEX_SPAN_START,
    LATEX_SPAN_CLOSE,
    // HTML comment token
    HTML_COMMENT,
    RAW_SPECIFIER,
    AUTOLINK,
    LANGUAGE_SPECIFIER,
    KEY_SPECIFIER,
    NAKED_VALUE_SPECIFIER,

    // now all the tokens from the inline scanner since we're doing it all here
    // SPAN_START

    HIGHLIGHT_SPAN_START,
    INSERT_SPAN_START,
    DELETE_SPAN_START,
    COMMENT_SPAN_START,

    SINGLE_QUOTE_OPEN,
    SINGLE_QUOTE_CLOSE,
    DOUBLE_QUOTE_OPEN,
    DOUBLE_QUOTE_CLOSE,

    SHORTCODE_OPEN_ESCAPED,
    SHORTCODE_CLOSE_ESCAPED,
    SHORTCODE_OPEN,
    SHORTCODE_CLOSE,

    CITE_AUTHOR_IN_TEXT_WITH_OPEN_BRACKET,
    CITE_SUPPRESS_AUTHOR_WITH_OPEN_BRACKET,
    CITE_AUTHOR_IN_TEXT,
    CITE_SUPPRESS_AUTHOR,
} TokenType;

#ifdef SCAN_DEBUG

static char* token_names[] = {
    "LINE_ENDING",
    "SOFT_LINE_ENDING",
    "BLOCK_CLOSE",
    "BLOCK_CONTINUATION",
    "BLOCK_QUOTE_START",
    "INDENTED_CHUNK_START",
    "ATX_H1_MARKER",
    "ATX_H2_MARKER",
    "ATX_H3_MARKER",
    "ATX_H4_MARKER",
    "ATX_H5_MARKER",
    "ATX_H6_MARKER",
    "SETEXT_H1_UNDERLINE",
    "SETEXT_H2_UNDERLINE",
    "THEMATIC_BREAK",
    "LIST_MARKER_MINUS",
    "LIST_MARKER_PLUS",
    "LIST_MARKER_STAR",
    "LIST_MARKER_PARENTHESIS",
    "LIST_MARKER_DOT",
    "LIST_MARKER_MINUS_DONT_INTERRUPT",
    "LIST_MARKER_PLUS_DONT_INTERRUPT",
    "LIST_MARKER_STAR_DONT_INTERRUPT",
    "LIST_MARKER_PARENTHESIS_DONT_INTERRUPT",
    "LIST_MARKER_DOT_DONT_INTERRUPT",
    "LIST_MARKER_EXAMPLE",
    "LIST_MARKER_EXAMPLE_DONT_INTERRUPT",
    "FENCED_CODE_BLOCK_START_BACKTICK",
    "FENCED_CODE_BLOCK_START_TILDE",
    "BLANK_LINE_START",
    "FENCED_CODE_BLOCK_END_BACKTICK",
    "FENCED_CODE_BLOCK_END_TILDE",
    "CLOSE_BLOCK",
    "NO_INDENTED_CHUNK",
    "ERROR",
    "TRIGGER_ERROR",
    "TOKEN_EOF",
    "MINUS_METADATA",
    "PLUS_METADATA",
    "PIPE_TABLE_START",
    "PIPE_TABLE_LINE_ENDING",
    "FENCED_DIV_START",
    "FENCED_DIV_END",
    "REF_ID_SPECIFIER",
    "FENCED_DIV_NOTE_ID",
    // special tokens to trigger serialization to track in-display-math mode
    "DISPLAY_MATH_STATE_TRACK_MARKER",
    "INLINE_MATH_STATE_TRACK_MARKER",
    // code span delimiters for parsing pipe table cells
    "CODE_SPAN_START",
    "CODE_SPAN_CLOSE",
    // latex span delimiters for parsing pipe table cells
    "LATEX_SPAN_START",
    "LATEX_SPAN_CLOSE",
    // HTML comment token
    "HTML_COMMENT",
    "RAW_SPECIFIER",
    "AUTOLINK",
    "LANGUAGE_SPECIFIER",
    "KEY_SPECIFIER",
    "NAKED_VALUE_SPECIFIER",

    "HIGHLIGHT_SPAN_START",
    "INSERT_SPAN_START",
    "DELETE_SPAN_START",
    "COMMENT_SPAN_START",

    "SINGLE_QUOTE_OPEN",
    "SINGLE_QUOTE_CLOSE",
    "DOUBLE_QUOTE_OPEN",
    "DOUBLE_QUOTE_CLOSE",

    "SHORTCODE_OPEN_ESCAPED",
    "SHORTCODE_CLOSE_ESCAPED",
    "SHORTCODE_OPEN",
    "SHORTCODE_CLOSE",

    "CITE_AUTHOR_IN_TEXT_WITH_OPEN_BRACKET",
    "CITE_SUPPRESS_AUTHOR_WITH_OPEN_BRACKET",
    "CITE_AUTHOR_IN_TEXT",
    "CITE_SUPPRESS_AUTHOR",
};

#endif

// Description of a block on the block stack.
//
// LIST_ITEM is a list item with minimal indentation (content begins at indent
// level 2) while LIST_ITEM_MAX_INDENTATION represents a list item with maximal
// indentation without being considered a indented code block.
//
// ANONYMOUS represents any block that whose close is not handled by the
// external s.
typedef enum {
    BLOCK_QUOTE,
    INDENTED_CODE_BLOCK,
    LIST_ITEM,
    LIST_ITEM_1_INDENTATION,
    LIST_ITEM_2_INDENTATION,
    LIST_ITEM_3_INDENTATION,
    LIST_ITEM_4_INDENTATION,
    LIST_ITEM_5_INDENTATION,
    LIST_ITEM_6_INDENTATION,
    LIST_ITEM_7_INDENTATION,
    LIST_ITEM_8_INDENTATION,
    LIST_ITEM_9_INDENTATION,
    LIST_ITEM_10_INDENTATION,
    LIST_ITEM_11_INDENTATION,
    LIST_ITEM_12_INDENTATION,
    LIST_ITEM_13_INDENTATION,
    LIST_ITEM_14_INDENTATION,
    LIST_ITEM_MAX_INDENTATION,
    FENCED_CODE_BLOCK,
    ANONYMOUS,
    FENCED_DIV,
} Block;

// Determines if a character is punctuation as defined by the markdown spec.
static bool is_punctuation(char chr) {
    return (chr >= '!' && chr <= '/') || (chr >= ':' && chr <= '@') ||
           (chr >= '[' && chr <= '`') || (chr >= '{' && chr <= '~');
}

// Returns the indentation level which lines of a list item should have at
// minimum. Should only be called with blocks for which `is_list_item` returns
// true.
static uint8_t list_item_indentation(Block block) {
    return (uint8_t)(block - LIST_ITEM + 2);
}

// For explanation of the tokens see grammar.js
static const bool display_math_paragraph_interrupt_symbols[] = {
    false, // LINE_ENDING,
    false, // SOFT_LINE_ENDING,
    false, // BLOCK_CLOSE,
    false, // BLOCK_CONTINUATION,
    true,  // BLOCK_QUOTE_START,
    false, // INDENTED_CHUNK_START,
    true,  // ATX_H1_MARKER,
    true,  // ATX_H2_MARKER,
    true,  // ATX_H3_MARKER,
    true,  // ATX_H4_MARKER,
    true,  // ATX_H5_MARKER,
    true,  // ATX_H6_MARKER,
    true,  // SETEXT_H1_UNDERLINE,
    true,  // SETEXT_H2_UNDERLINE,
    true,  // THEMATIC_BREAK,
    false,  // LIST_MARKER_MINUS,
    false,  // LIST_MARKER_PLUS,
    false,  // LIST_MARKER_STAR,
    false,  // LIST_MARKER_PARENTHESIS,
    false,  // LIST_MARKER_DOT,
    false, // LIST_MARKER_MINUS_DONT_INTERRUPT,
    false, // LIST_MARKER_PLUS_DONT_INTERRUPT,
    false, // LIST_MARKER_STAR_DONT_INTERRUPT,
    false, // LIST_MARKER_PARENTHESIS_DONT_INTERRUPT,
    false, // LIST_MARKER_DOT_DONT_INTERRUPT,
    false, // LIST_MARKER_EXAMPLE,
    false, // LIST_MARKER_EXAMPLE_DONT_INTERRUPT,
    true,  // FENCED_CODE_BLOCK_START_BACKTICK,
    true,  // FENCED_CODE_BLOCK_START_TILDE,
    true,  // BLANK_LINE_START,
    false, // FENCED_CODE_BLOCK_END_BACKTICK,
    false, // FENCED_CODE_BLOCK_END_TILDE,
    false, // CLOSE_BLOCK,
    false, // NO_INDENTED_CHUNK,
    false, // ERROR,
    false, // TRIGGER_ERROR,
    false, // EOF,
    false, // MINUS_METADATA,
    false, // PLUS_METADATA,
    true,  // PIPE_TABLE_START,
    false, // PIPE_TABLE_LINE_ENDING,
    true,  // FENCED_DIV_START,
    true,  // FENCED_DIV_END,
    false, // REF_ID_SPECIFIER,
    false, // FENCED_DIV_NOTE_ID,
    false, // DISPLAY_MATH_STATE_TRACK_MARKER
    false, // INLINE_MATH_STATE_TRACK_MARKER
    false, // CODE_SPAN_START
    false, // CODE_SPAN_CLOSE
    false, // LATEX_SPAN_START
    false, // LATEX_SPAN_CLOSE
    false, // HTML_COMMENT
    false, // RAW_SPECIFIER
    false, // AUTOLINK
    false, // LANGUAGE_SPECIFIER
    false, // KEY_SPECIFIER
    false, // NAKED_VALUE_SPECIFIER
    false, // HIGHLIGHT_SPAN_START
    false, // INSERT_SPAN_START
    false, // DELETE_SPAN_START
    false, // COMMENT_SPAN_START
    false, // SINGLE_QUOTE_OPEN
    false, // SINGLE_QUOTE_CLOSE
    false, // DOUBLE_QUOTE_OPEN
    false, // DOUBLE_QUOTE_CLOSE
    false, // SHORTCODE_OPEN_ESCAPED,
    false, // SHORTCODE_CLOSE_ESCAPED,
    false, // SHORTCODE_OPEN,
    false, // SHORTCODE_CLOSE,
    false, // CITE_AUTHOR_IN_TEXT_WITH_OPEN_BRACKET,
    false, // CITE_SUPPRESS_AUTHOR_WITH_OPEN_BRACKET,
    false, // CITE_AUTHOR_IN_TEXT,
    false, // CITE_SUPPRESS_AUTHOR,
};

static const bool paragraph_interrupt_symbols[] = {
    false, // LINE_ENDING,
    false, // SOFT_LINE_ENDING,
    false, // BLOCK_CLOSE,
    false, // BLOCK_CONTINUATION,
    true,  // BLOCK_QUOTE_START,
    false, // INDENTED_CHUNK_START,
    true,  // ATX_H1_MARKER,
    true,  // ATX_H2_MARKER,
    true,  // ATX_H3_MARKER,
    true,  // ATX_H4_MARKER,
    true,  // ATX_H5_MARKER,
    true,  // ATX_H6_MARKER,
    true,  // SETEXT_H1_UNDERLINE,
    true,  // SETEXT_H2_UNDERLINE,
    true,  // THEMATIC_BREAK,
    true,  // LIST_MARKER_MINUS,
    true,  // LIST_MARKER_PLUS,
    true,  // LIST_MARKER_STAR,
    true,  // LIST_MARKER_PARENTHESIS,
    true,  // LIST_MARKER_DOT,
    false, // LIST_MARKER_MINUS_DONT_INTERRUPT,
    false, // LIST_MARKER_PLUS_DONT_INTERRUPT,
    false, // LIST_MARKER_STAR_DONT_INTERRUPT,
    false, // LIST_MARKER_PARENTHESIS_DONT_INTERRUPT,
    false, // LIST_MARKER_DOT_DONT_INTERRUPT,
    true,  // LIST_MARKER_EXAMPLE,
    false, // LIST_MARKER_EXAMPLE_DONT_INTERRUPT,
    true,  // FENCED_CODE_BLOCK_START_BACKTICK,
    true,  // FENCED_CODE_BLOCK_START_TILDE,
    true,  // BLANK_LINE_START,
    false, // FENCED_CODE_BLOCK_END_BACKTICK,
    false, // FENCED_CODE_BLOCK_END_TILDE,
    false, // CLOSE_BLOCK,
    false, // NO_INDENTED_CHUNK,
    false, // ERROR,
    false, // TRIGGER_ERROR,
    false, // EOF,
    false, // MINUS_METADATA,
    false, // PLUS_METADATA,
    true,  // PIPE_TABLE_START,
    false, // PIPE_TABLE_LINE_ENDING,
    true,  // FENCED_DIV_START,
    true,  // FENCED_DIV_END,
    false, // REF_ID_SPECIFIER,
    false, // FENCED_DIV_NOTE_ID,
    false, // DISPLAY_MATH_STATE_TRACK_MARKER
    false, // INLINE_MATH_STATE_TRACK_MARKER
    false, // CODE_SPAN_START
    false, // CODE_SPAN_CLOSE
    false, // LATEX_SPAN_START
    false, // LATEX_SPAN_CLOSE
    false, // HTML_COMMENT
    false, // RAW_SPECIFIER
    false, // AUTOLINK
    false, // LANGUAGE_SPECIFIER
    false, // KEY_SPECIFIER
    false, // NAKED_VALUE_SPECIFIER
    false, // HIGHLIGHT_SPAN_START
    false, // INSERT_SPAN_START
    false, // DELETE_SPAN_START
    false, // COMMENT_SPAN_START
    false, // SINGLE_QUOTE_OPEN
    false, // SINGLE_QUOTE_CLOSE
    false, // DOUBLE_QUOTE_OPEN
    false, // DOUBLE_QUOTE_CLOSE
    false, // SHORTCODE_OPEN_ESCAPED,
    false, // SHORTCODE_CLOSE_ESCAPED,
    false, // SHORTCODE_OPEN,
    false, // SHORTCODE_CLOSE,
    false, // CITE_AUTHOR_IN_TEXT_WITH_OPEN_BRACKET,
    false, // CITE_SUPPRESS_AUTHOR_WITH_OPEN_BRACKET,
    false, // CITE_AUTHOR_IN_TEXT,
    false, // CITE_SUPPRESS_AUTHOR,
};

// State bitflags used with `Scanner.state`

// Currently matching (at the beginning of a line)
static const uint8_t STATE_MATCHING = 0x1 << 0;
// Last line break was inside a paragraph
static const uint8_t STATE_WAS_SOFT_LINE_BREAK = 0x1 << 1;
// Block should be closed after next line break
static const uint8_t STATE_CLOSE_BLOCK = 0x1 << 4;
// Currently inside display math ($$...$$)
static const uint8_t STATE_IN_DISPLAY_MATH = 0x1 << 5;

static size_t roundup_32(size_t x) {
    x--;

    x |= x >> 1;
    x |= x >> 2;
    x |= x >> 4;
    x |= x >> 8;
    x |= x >> 16;

    x++;

    return x;
}

typedef struct {
    unsigned own_size;
    // Size of the serialized state of the scanner.
    // This is used to determine if we're too close to hitting
    // tree-sitter's maximum serialized size limit of 1024 bytes,
    // defined in tree-sitter's repo in lib/src/parser.h

    // A stack of open blocks in the current parse state
    struct {
        size_t size;
        size_t capacity;
        Block *items;
    } open_blocks;

    // Parser state flags
    uint8_t state;
    // Number of blocks that have been matched so far. Only changes during
    // matching and is reset after every line ending
    uint8_t matched;
    // Consumed but "unused" indentation. Sometimes a tab needs to be "split" to
    // be used in multiple tokens.
    uint8_t indentation;
    // The current column. Used to decide how many spaces a tab should equal
    uint8_t column;
    // The delimiter length of the currently open fenced code block
    uint8_t fenced_code_block_delimiter_length;
    // The delimiter length of the currently open code span (for pipe table cells)
    uint8_t code_span_delimiter_length;
    // Whether we're inside a code span (for pipe table cells)
    uint8_t inside_code_span;
    // The delimiter length of the currently open latex span (for pipe table cells)
    uint8_t latex_span_delimiter_length;
    // Whether we're inside a latex span (for pipe table cells)
    uint8_t inside_latex_span;

    bool simulate;
} Scanner;

static bool can_push_block(Scanner *s) {
    // the serialization state size is equal
    // to sizeof(Scanner) + sizeof(Block) * open_blocks.size
    // If this grows over 75% of the maximum serialized size limit
    // then we refuse to push blocks further, and purposefully fail to scan.
    // This is to prevent the scanner from growing too large and hitting
    // tree-sitter's maximum serialized size limit of 1024 bytes.
    size_t serialized_size = sizeof(Scanner) + sizeof(Block) * s->open_blocks.size;
    size_t max_serialized_size = 1024;
    size_t max_serialized_size_limit = (max_serialized_size * 3) / 4;
    return serialized_size < max_serialized_size_limit;
}

static void push_block(Scanner *s, Block b) {
    if (s->open_blocks.size == s->open_blocks.capacity) {
        s->open_blocks.capacity =
            s->open_blocks.capacity ? s->open_blocks.capacity << 1 : 8;
        void *tmp = realloc(s->open_blocks.items,
                            sizeof(Block) * s->open_blocks.capacity);
        assert(tmp != NULL);
        s->open_blocks.items = tmp;
    }

    s->open_blocks.items[s->open_blocks.size++] = b;
}

static inline Block pop_block(Scanner *s) {
    return s->open_blocks.items[--s->open_blocks.size];
}

// Write the whole state of a Scanner to a byte buffer
static unsigned serialize(Scanner *s, char *buffer) {
    unsigned size = 0;
    for (size_t i = 0; i < sizeof(unsigned); i++) {
        buffer[size++] = '\0';
    }
    buffer[size++] = (char)s->state;
    buffer[size++] = (char)s->matched;
    buffer[size++] = (char)s->indentation;
    buffer[size++] = (char)s->column;
    buffer[size++] = (char)s->fenced_code_block_delimiter_length;
    buffer[size++] = (char)s->code_span_delimiter_length;
    buffer[size++] = (char)s->inside_code_span;
    buffer[size++] = (char)s->latex_span_delimiter_length;
    buffer[size++] = (char)s->inside_latex_span;
    size_t blocks_count = s->open_blocks.size;
    if (blocks_count > 0) {
        memcpy(&buffer[size], s->open_blocks.items,
               blocks_count * sizeof(Block));
        size += blocks_count * sizeof(Block);
    }
    s->own_size = size;
    return size;
}

// Read the whole state of a Scanner from a byte buffer
// `serizalize` and `deserialize` should be fully symmetric.
static void deserialize(Scanner *s, const char *buffer, unsigned length) {
    s->own_size = 0;
    s->open_blocks.size = 0;
    s->open_blocks.capacity = 0;
    s->state = 0;
    s->matched = 0;
    s->indentation = 0;
    s->column = 0;
    s->fenced_code_block_delimiter_length = 0;
    s->code_span_delimiter_length = 0;
    s->inside_code_span = 0;
    s->latex_span_delimiter_length = 0;
    s->inside_latex_span = 0;
    if (length > 0) {
        size_t size = 0;
        s->own_size = length;
        size += sizeof(unsigned);
        s->state = (uint8_t)buffer[size++];
        s->matched = (uint8_t)buffer[size++];
        s->indentation = (uint8_t)buffer[size++];
        s->column = (uint8_t)buffer[size++];
        s->fenced_code_block_delimiter_length = (uint8_t)buffer[size++];
        s->code_span_delimiter_length = (uint8_t)buffer[size++];
        s->inside_code_span = (uint8_t)buffer[size++];
        s->latex_span_delimiter_length = (uint8_t)buffer[size++];
        s->inside_latex_span = (uint8_t)buffer[size++];
        size_t blocks_size = length - size;
        if (blocks_size > 0) {
            size_t blocks_count = blocks_size / sizeof(Block);

            // ensure open blocks has enough room
            if (s->open_blocks.capacity < blocks_count) {
              size_t capacity = roundup_32(blocks_count);
              void *tmp = realloc(s->open_blocks.items,
                            sizeof(Block) * capacity);
              assert(tmp != NULL);
              s->open_blocks.items = tmp;
              s->open_blocks.capacity = capacity;
            }
            memcpy(s->open_blocks.items, &buffer[size], blocks_size);
            s->open_blocks.size = blocks_count;
        }
    }
}

static void mark_end(Scanner *s, TSLexer *lexer) {
    if (!s->simulate) {
        lexer->mark_end(lexer);
    }
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
// 4. When the scanner is asked to push a block but is too close to the
//    maximum serialized size limit of 1024 bytes.
//
// See also the `$._soft_line_break` and `$._paragraph_end_newline` tokens in
// grammar.js
static bool error(TSLexer *lexer) {
    lexer->result_symbol = ERROR;
    return true;
}

// Advance the lexer one character
// Also keeps track of the current column, counting tabs as spaces with tab stop
// 4 See https://github.github.com/gfm/#tabs
static size_t advance(Scanner *s, TSLexer *lexer) {
    size_t size = 1;
    if (lexer->lookahead == '\t') {
        size = 4 - s->column;
        s->column = 0;
    } else {
        s->column = (s->column + 1) % 4;
    }
    lexer->advance(lexer, false);
    return size;
}

// Try to match the given block, i.e. consume all tokens that belong to the
// block. These are
// 1. indentation for list items and indented code blocks
// 2. '>' for block quotes
// Returns true if the block is matched and false otherwise
static bool match(Scanner *s, TSLexer *lexer, Block block) {
    switch (block) {
        case INDENTED_CODE_BLOCK:
            while (s->indentation < 4) {
                if (lexer->lookahead == ' ' || lexer->lookahead == '\t') {
                    s->indentation += advance(s, lexer);
                } else {
                    break;
                }
            }
            if (s->indentation >= 4 && lexer->lookahead != '\n' &&
                lexer->lookahead != '\r') {
                s->indentation -= 4;
                return true;
            }
            break;
        case LIST_ITEM:
        case LIST_ITEM_1_INDENTATION:
        case LIST_ITEM_2_INDENTATION:
        case LIST_ITEM_3_INDENTATION:
        case LIST_ITEM_4_INDENTATION:
        case LIST_ITEM_5_INDENTATION:
        case LIST_ITEM_6_INDENTATION:
        case LIST_ITEM_7_INDENTATION:
        case LIST_ITEM_8_INDENTATION:
        case LIST_ITEM_9_INDENTATION:
        case LIST_ITEM_10_INDENTATION:
        case LIST_ITEM_11_INDENTATION:
        case LIST_ITEM_12_INDENTATION:
        case LIST_ITEM_13_INDENTATION:
        case LIST_ITEM_14_INDENTATION:
        case LIST_ITEM_MAX_INDENTATION:
            while (s->indentation < list_item_indentation(block)) {
                if (lexer->lookahead == ' ' || lexer->lookahead == '\t') {
                    s->indentation += advance(s, lexer);
                } else {
                    break;
                }
            }
            if (s->indentation >= list_item_indentation(block)) {
                s->indentation -= list_item_indentation(block);
                return true;
            }
            if (lexer->lookahead == '\n' || lexer->lookahead == '\r') {
                s->indentation = 0;
                return true;
            }
            break;
        case BLOCK_QUOTE:
            while (lexer->lookahead == ' ' || lexer->lookahead == '\t') {
                s->indentation += advance(s, lexer);
            }
            if (lexer->lookahead == '>') {
                advance(s, lexer);
                s->indentation = 0;
                if (lexer->lookahead == ' ' || lexer->lookahead == '\t') {
                    s->indentation += advance(s, lexer) - 1;
                }
                return true;
            }
            break;
        case FENCED_DIV:
        case FENCED_CODE_BLOCK:
        case ANONYMOUS:
            return true;
    }
    return false;
}

static bool parse_fenced_div_note_id(Scanner *s, TSLexer *lexer,
                                      const bool *valid_symbols);

static bool parse_fenced_div_marker(Scanner *s, TSLexer *lexer,
                                    const bool *valid_symbols) {
    uint8_t level = 0;
    while (lexer->lookahead == ':') {
        advance(s, lexer);
        level++;
    }
    mark_end(s, lexer);
    if (level < 3) {
        return false;
    }

    // if this is a valid start of a fenced div marker, then it must be
    // followed by whitespace and any other non-whitespace character
    // (a curly brace indicates an attribute, anything else indicates
    // an infostring)
    //
    // otherwise, it can only be a valid marker for the end of a fenced div

    while (!lexer->eof(lexer) &&
        (lexer->lookahead == ' ' || lexer->lookahead == '\t')) {
        advance(s, lexer);
    }
    if (lexer->eof(lexer) || lexer->lookahead == '\n' || lexer->lookahead == '\r') {
        if (valid_symbols[FENCED_DIV_END]) {
            lexer->result_symbol = FENCED_DIV_END;
            return true;
        }
    }
    if (!lexer->eof(lexer)) {
        if (valid_symbols[FENCED_DIV_START]) {
            lexer->result_symbol = FENCED_DIV_START;
            if (!s->simulate) {
                if (!can_push_block(s)) {
                    return error(lexer);
                }
                push_block(s, FENCED_DIV);
            }
            return true;
        }
    }
    return false;
}

static bool parse_fenced_code_block(Scanner *s, const char delimiter,
                                    TSLexer *lexer, const bool *valid_symbols) {
    // count the number of backticks
    uint8_t level = 0;
    while (lexer->lookahead == delimiter) {
        advance(s, lexer);
        level++;
    }
    mark_end(s, lexer);

    // we might need to open a code span at the start of a paragraph
    if (valid_symbols[CODE_SPAN_START] && delimiter == '`' && level < 3) {
        lexer->result_symbol = CODE_SPAN_START;
        s->code_span_delimiter_length = level;
        s->inside_code_span = 1;
        return true;
    }
    // If this is able to close a fenced code block then that is the only valid
    // interpretation. It can only close a fenced code block if the number of
    // backticks is at least the number of backticks of the opening delimiter.
    // Also it cannot be indented more than 3 spaces.
    if ((delimiter == '`' ? valid_symbols[FENCED_CODE_BLOCK_END_BACKTICK]
                          : valid_symbols[FENCED_CODE_BLOCK_END_TILDE]) &&
        s->indentation < 4 && level >= s->fenced_code_block_delimiter_length) {
        while (lexer->lookahead == ' ' || lexer->lookahead == '\t') {
            advance(s, lexer);
        }
        if (lexer->lookahead == '\n' || lexer->lookahead == '\r') {
            s->fenced_code_block_delimiter_length = 0;
            lexer->result_symbol = delimiter == '`'
                                       ? FENCED_CODE_BLOCK_END_BACKTICK
                                       : FENCED_CODE_BLOCK_END_TILDE;
            return true;
        }
    }
    // If this could be the start of a fenced code block, check if the info
    // string contains any backticks.
    if ((delimiter == '`' ? valid_symbols[FENCED_CODE_BLOCK_START_BACKTICK]
                          : valid_symbols[FENCED_CODE_BLOCK_START_TILDE]) &&
        level >= 3) {
        bool info_string_has_backtick = false;
        if (delimiter == '`') {
            while (lexer->lookahead != '\n' && lexer->lookahead != '\r' &&
                   !lexer->eof(lexer)) {
                if (lexer->lookahead == '`') {
                    info_string_has_backtick = true;
                    break;
                }
                advance(s, lexer);
            }
        }
        // If it does not then choose to interpret this as the start of a fenced
        // code block.
        if (!info_string_has_backtick) {
            lexer->result_symbol = delimiter == '`'
                                       ? FENCED_CODE_BLOCK_START_BACKTICK
                                       : FENCED_CODE_BLOCK_START_TILDE;
            if (!s->simulate) {
                if (!can_push_block(s)) {
                    return error(lexer);
                }
                push_block(s, FENCED_CODE_BLOCK);
            }
            // Remember the length of the delimiter for later, since we need it
            // to decide whether a sequence of backticks can close the block.
            s->fenced_code_block_delimiter_length = level;
            s->indentation = 0;
            return true;
        }
    }
    return false;
}

static bool parse_star(Scanner *s, TSLexer *lexer, const bool *valid_symbols) {
    advance(s, lexer);
    mark_end(s, lexer);
    // Otherwise count the number of stars permitting whitespaces between them.
    size_t star_count = 1;
    // Also remember how many stars there are before the first whitespace...
    // ...and how many spaces follow the first star.
    uint8_t extra_indentation = 0;
    for (;;) {
        if (lexer->lookahead == '*') {
            if (star_count == 1 && extra_indentation >= 1 &&
                valid_symbols[LIST_MARKER_STAR]) {
                // If we get to this point then the token has to be at least
                // this long. We need to call `mark_end` here in case we decide
                // later that this is a list item.
                mark_end(s, lexer);
            }
            star_count++;
            advance(s, lexer);
        } else if (lexer->lookahead == ' ' || lexer->lookahead == '\t') {
            if (star_count == 1) {
                extra_indentation += advance(s, lexer);
            } else {
                advance(s, lexer);
            }
        } else {
            break;
        }
    }
    bool line_end = lexer->lookahead == '\n' || lexer->lookahead == '\r';
    bool dont_interrupt = false;
    if (star_count == 1 && line_end) {
        extra_indentation = 1;
        // line is empty so don't interrupt paragraphs if this is a list marker
        dont_interrupt = s->matched == s->open_blocks.size;
    }
    // If there were at least 3 stars then this could be a thematic break
    bool thematic_break = star_count >= 3 && line_end;
    // If there was a star and at least one space after that star then this
    // could be a list marker.
    bool list_marker_star = star_count >= 1 && extra_indentation >= 1;
    if (valid_symbols[THEMATIC_BREAK] && thematic_break && s->indentation < 4) {
        // If a thematic break is valid then it takes precedence
        lexer->result_symbol = THEMATIC_BREAK;
        mark_end(s, lexer);
        s->indentation = 0;
        return true;
    }
    if ((dont_interrupt ? valid_symbols[LIST_MARKER_STAR_DONT_INTERRUPT]
                        : valid_symbols[LIST_MARKER_STAR]) &&
        list_marker_star) {
        // List markers take precedence over emphasis markers
        // If star_count > 1 then we already called mark_end at the right point.
        // Otherwise the token should go until this point.
        if (star_count == 1) {
            mark_end(s, lexer);
        }
        // Not counting one space...
        extra_indentation--;
        // ... check if the list item begins with an indented code block
        if (extra_indentation <= 3) {
            // If not then calculate the indentation level of the list item
            // content as indentation of list marker + indentation after list
            // marker - 1
            extra_indentation += s->indentation;
            s->indentation = 0;
        } else {
            // Otherwise the indentation level is just the indentation of the
            // list marker. We keep the indentation after the list marker for
            // later blocks.
            uint8_t temp = s->indentation;
            s->indentation = extra_indentation;
            extra_indentation = temp;
        }
        if (!s->simulate) {
            if (!can_push_block(s)) {
                return error(lexer);
            }
            push_block(s, (Block)(LIST_ITEM + extra_indentation));
        }
        lexer->result_symbol =
            dont_interrupt ? LIST_MARKER_STAR_DONT_INTERRUPT : LIST_MARKER_STAR;
        return true;
    }
    return false;
}

static bool parse_thematic_break_underscore(Scanner *s, TSLexer *lexer,
                                            const bool *valid_symbols) {
    advance(s, lexer);
    mark_end(s, lexer);
    size_t underscore_count = 1;
    for (;;) {
        if (lexer->lookahead == '_') {
            underscore_count++;
            advance(s, lexer);
        } else if (lexer->lookahead == ' ' || lexer->lookahead == '\t') {
            advance(s, lexer);
        } else {
            break;
        }
    }
    bool line_end = lexer->lookahead == '\n' || lexer->lookahead == '\r';
    if (underscore_count >= 3 && line_end && valid_symbols[THEMATIC_BREAK]) {
        lexer->result_symbol = THEMATIC_BREAK;
        mark_end(s, lexer);
        s->indentation = 0;
        return true;
    }
    return false;
}

static bool parse_block_quote(Scanner *s, TSLexer *lexer,
                              const bool *valid_symbols) {
    if (valid_symbols[BLOCK_QUOTE_START]) {
        advance(s, lexer);
        s->indentation = 0;
        if (lexer->lookahead == ' ' || lexer->lookahead == '\t') {
            s->indentation += advance(s, lexer) - 1;
        }
        lexer->result_symbol = BLOCK_QUOTE_START;
        if (!s->simulate) {
            if (!can_push_block(s)) {
                return error(lexer);
            }
            push_block(s, BLOCK_QUOTE);
        }
        return true;
    }
    return false;
}

static bool parse_atx_heading(Scanner *s, TSLexer *lexer,
                              const bool *valid_symbols) {
    if (valid_symbols[ATX_H1_MARKER] && s->indentation <= 3) {
        mark_end(s, lexer);
        uint16_t level = 0;
        while (lexer->lookahead == '#' && level <= 6) {
            advance(s, lexer);
            level++;
        }
        if (level <= 6 &&
            (lexer->lookahead == ' ' || lexer->lookahead == '\t' ||
             lexer->lookahead == '\n' || lexer->lookahead == '\r')) {
            lexer->result_symbol = ATX_H1_MARKER + (level - 1);
            s->indentation = 0;
            mark_end(s, lexer);
            return true;
        }
    }
    return false;
}

static bool parse_setext_underline(Scanner *s, TSLexer *lexer,
                                   const bool *valid_symbols) {
    if (valid_symbols[SETEXT_H1_UNDERLINE] &&
        s->matched == s->open_blocks.size) {
        mark_end(s, lexer);
        while (lexer->lookahead == '=') {
            advance(s, lexer);
        }
        while (lexer->lookahead == ' ' || lexer->lookahead == '\t') {
            advance(s, lexer);
        }
        if (lexer->lookahead == '\n' || lexer->lookahead == '\r') {
            lexer->result_symbol = SETEXT_H1_UNDERLINE;
            mark_end(s, lexer);
            return true;
        }
    }
    return false;
}

static bool parse_plus(Scanner *s, TSLexer *lexer, const bool *valid_symbols) {
    if (s->indentation <= 3 &&
        (valid_symbols[LIST_MARKER_PLUS] ||
         valid_symbols[LIST_MARKER_PLUS_DONT_INTERRUPT] ||
         valid_symbols[PLUS_METADATA])) {
        advance(s, lexer);
        if (valid_symbols[PLUS_METADATA] && lexer->lookahead == '+') {
            advance(s, lexer);
            if (lexer->lookahead != '+') {
                return false;
            }
            advance(s, lexer);
            while (lexer->lookahead == ' ' || lexer->lookahead == '\t') {
                advance(s, lexer);
            }
            if (lexer->lookahead != '\n' && lexer->lookahead != '\r') {
                return false;
            }
            for (;;) {
                // advance over newline
                if (lexer->lookahead == '\r') {
                    advance(s, lexer);
                    if (lexer->lookahead == '\n') {
                        advance(s, lexer);
                    }
                } else {
                    advance(s, lexer);
                }
                // check for pluses
                size_t plus_count = 0;
                while (lexer->lookahead == '+') {
                    plus_count++;
                    advance(s, lexer);
                }
                if (plus_count == 3) {
                    // if exactly 3 check if next symbol (after eventual
                    // whitespace) is newline
                    while (lexer->lookahead == ' ' ||
                           lexer->lookahead == '\t') {
                        advance(s, lexer);
                    }
                    if (lexer->lookahead == '\r' || lexer->lookahead == '\n') {
                        // if so also consume newline
                        if (lexer->lookahead == '\r') {
                            advance(s, lexer);
                            if (lexer->lookahead == '\n') {
                                advance(s, lexer);
                            }
                        } else {
                            advance(s, lexer);
                        }
                        mark_end(s, lexer);
                        lexer->result_symbol = PLUS_METADATA;
                        return true;
                    }
                }
                // otherwise consume rest of line
                while (lexer->lookahead != '\n' && lexer->lookahead != '\r' &&
                       !lexer->eof(lexer)) {
                    advance(s, lexer);
                }
                // if end of file is reached, then this is not metadata
                if (lexer->eof(lexer)) {
                    break;
                }
            }
        } else {
            uint8_t extra_indentation = 0;
            while (lexer->lookahead == ' ' || lexer->lookahead == '\t') {
                extra_indentation += advance(s, lexer);
            }
            bool dont_interrupt = false;
            if (lexer->lookahead == '\r' || lexer->lookahead == '\n') {
                extra_indentation = 1;
                dont_interrupt = true;
            }
            dont_interrupt =
                dont_interrupt && s->matched == s->open_blocks.size;
            if (extra_indentation >= 1 &&
                (dont_interrupt ? valid_symbols[LIST_MARKER_PLUS_DONT_INTERRUPT]
                                : valid_symbols[LIST_MARKER_PLUS])) {
                lexer->result_symbol = dont_interrupt
                                           ? LIST_MARKER_PLUS_DONT_INTERRUPT
                                           : LIST_MARKER_PLUS;
                extra_indentation--;
                if (extra_indentation <= 3) {
                    extra_indentation += s->indentation;
                    s->indentation = 0;
                } else {
                    uint8_t temp = s->indentation;
                    s->indentation = extra_indentation;
                    extra_indentation = temp;
                }
                if (!s->simulate) {
                    if (!can_push_block(s)) {
                        return error(lexer);
                    }
                    push_block(s, (Block)(LIST_ITEM + extra_indentation));
                }
                return true;
            }
        }
    }
    return false;
}

static bool parse_ordered_list_marker(Scanner *s, TSLexer *lexer,
                                      const bool *valid_symbols) {
    if (s->indentation <= 3 &&
        (valid_symbols[LIST_MARKER_PARENTHESIS] ||
         valid_symbols[LIST_MARKER_DOT] ||
         valid_symbols[LIST_MARKER_PARENTHESIS_DONT_INTERRUPT] ||
         valid_symbols[LIST_MARKER_DOT_DONT_INTERRUPT])) {
        size_t digits = 1;
        bool dont_interrupt = lexer->lookahead != '1';
        advance(s, lexer);
        while (iswdigit(lexer->lookahead)) {
            dont_interrupt = true;
            digits++;
            advance(s, lexer);
        }
        if (digits >= 1 && digits <= 9) {
            bool dot = false;
            bool parenthesis = false;
            if (lexer->lookahead == '.') {
                advance(s, lexer);
                dot = true;
            } else if (lexer->lookahead == ')') {
                advance(s, lexer);
                parenthesis = true;
            }
            if (dot || parenthesis) {
                uint8_t extra_indentation = 0;
                while (lexer->lookahead == ' ' || lexer->lookahead == '\t') {
                    extra_indentation += advance(s, lexer);
                }
                bool line_end =
                    lexer->lookahead == '\n' || lexer->lookahead == '\r';
                if (line_end) {
                    extra_indentation = 1;
                    dont_interrupt = true;
                }
                dont_interrupt =
                    dont_interrupt && s->matched == s->open_blocks.size;
                if (extra_indentation >= 1 &&
                    (dot ? (dont_interrupt
                                ? valid_symbols[LIST_MARKER_DOT_DONT_INTERRUPT]
                                : valid_symbols[LIST_MARKER_DOT])
                         : (dont_interrupt
                                ? valid_symbols
                                      [LIST_MARKER_PARENTHESIS_DONT_INTERRUPT]
                                : valid_symbols[LIST_MARKER_PARENTHESIS]))) {
                    lexer->result_symbol =
                        dot ? LIST_MARKER_DOT : LIST_MARKER_PARENTHESIS;
                    extra_indentation--;
                    if (extra_indentation <= 3) {
                        extra_indentation += s->indentation;
                        s->indentation = 0;
                    } else {
                        uint8_t temp = s->indentation;
                        s->indentation = extra_indentation;
                        extra_indentation = temp;
                    }
                    if (!s->simulate) {
                        if (!can_push_block(s)) {
                            return error(lexer);
                        }
                        push_block(
                            s, (Block)(LIST_ITEM + extra_indentation + digits));
                    }
                    return true;
                }
            }
        }
    }
    return false;
}

static bool parse_example_list_marker(Scanner *s, TSLexer *lexer,
                                       const bool *valid_symbols) {
    if (s->indentation <= 3 &&
        (valid_symbols[LIST_MARKER_EXAMPLE] ||
         valid_symbols[LIST_MARKER_EXAMPLE_DONT_INTERRUPT])) {
        // Must be (@)
        if (lexer->lookahead != '(') {
            return false;
        }
        advance(s, lexer);
        if (lexer->lookahead != '@') {
            return false;
        }
        advance(s, lexer);
        if (lexer->lookahead != ')') {
            return false;
        }
        advance(s, lexer);

        uint8_t extra_indentation = 0;
        while (lexer->lookahead == ' ' || lexer->lookahead == '\t') {
            extra_indentation += advance(s, lexer);
        }
        bool line_end = lexer->lookahead == '\n' || lexer->lookahead == '\r';
        bool dont_interrupt = false;
        if (line_end) {
            extra_indentation = 1;
            dont_interrupt = true;
        }
        dont_interrupt = dont_interrupt && s->matched == s->open_blocks.size;
        if (extra_indentation >= 1 &&
            (dont_interrupt ? valid_symbols[LIST_MARKER_EXAMPLE_DONT_INTERRUPT]
                            : valid_symbols[LIST_MARKER_EXAMPLE])) {
            lexer->result_symbol = dont_interrupt
                                       ? LIST_MARKER_EXAMPLE_DONT_INTERRUPT
                                       : LIST_MARKER_EXAMPLE;
            extra_indentation--;
            if (extra_indentation <= 3) {
                extra_indentation += s->indentation;
                s->indentation = 0;
            } else {
                uint8_t temp = s->indentation;
                s->indentation = extra_indentation;
                extra_indentation = temp;
            }
            if (!s->simulate) {
                if (!can_push_block(s)) {
                    return error(lexer);
                }
                // Use 3 as the indentation offset (length of "(@)")
                push_block(s, (Block)(LIST_ITEM + extra_indentation + 3));
            }
            return true;
        }
    }
    return false;
}

static bool parse_cite_suppress_author(Scanner *_, TSLexer *lexer,
                                       const bool *valid_symbols) {
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

static bool parse_minus(Scanner *s, TSLexer *lexer, const bool *valid_symbols) {
    if (s->indentation <= 3 &&
        (valid_symbols[LIST_MARKER_MINUS] ||
         valid_symbols[LIST_MARKER_MINUS_DONT_INTERRUPT] ||
         valid_symbols[SETEXT_H2_UNDERLINE] || valid_symbols[THEMATIC_BREAK] ||
         valid_symbols[CITE_SUPPRESS_AUTHOR_WITH_OPEN_BRACKET] || 
         valid_symbols[MINUS_METADATA])) {
        mark_end(s, lexer);
        bool whitespace_after_minus = false;
        bool minus_after_whitespace = false;
        size_t minus_count = 0;
        uint8_t extra_indentation = 0;

        for (;;) {
            if (lexer->lookahead == '-') {
                if (minus_count == 1 && extra_indentation >= 1) {
                    mark_end(s, lexer);
                }
                minus_count++;
                advance(s, lexer);
                minus_after_whitespace = whitespace_after_minus;
            } else if (lexer->lookahead == ' ' || lexer->lookahead == '\t') {
                if (minus_count == 1) {
                    extra_indentation += advance(s, lexer);
                } else {
                    advance(s, lexer);
                }
                whitespace_after_minus = true;
            } else {
                break;
            }
        }
        bool line_end = lexer->lookahead == '\n' || lexer->lookahead == '\r';
        bool dont_interrupt = false;
        if (minus_count == 1 && line_end) {
            extra_indentation = 1;
            dont_interrupt = true;
        }
        dont_interrupt = dont_interrupt && s->matched == s->open_blocks.size;
        bool thematic_break = minus_count >= 3 && line_end;
        bool underline =
            minus_count >= 1 && !minus_after_whitespace && line_end &&
            s->matched ==
                s->open_blocks
                    .size; // setext heading can not break lazy continuation
        bool list_marker_minus = minus_count >= 1 && extra_indentation >= 1;
        bool success = false;
        if (valid_symbols[SETEXT_H2_UNDERLINE] && underline) {
            lexer->result_symbol = SETEXT_H2_UNDERLINE;
            mark_end(s, lexer);
            s->indentation = 0;
            success = true;
        } else if (valid_symbols[THEMATIC_BREAK] &&
                   thematic_break) { // underline is false if list_marker_minus
                                     // is true
            lexer->result_symbol = THEMATIC_BREAK;
            mark_end(s, lexer);
            s->indentation = 0;
            success = true;
        } else if ((dont_interrupt
                        ? valid_symbols[LIST_MARKER_MINUS_DONT_INTERRUPT]
                        : valid_symbols[LIST_MARKER_MINUS]) &&
                   list_marker_minus) {
            if (minus_count == 1) {
                mark_end(s, lexer);
            }
            extra_indentation--;
            if (extra_indentation <= 3) {
                extra_indentation += s->indentation;
                s->indentation = 0;
            } else {
                uint8_t temp = s->indentation;
                s->indentation = extra_indentation;
                extra_indentation = temp;
            }
            if (!s->simulate) {
                if (!can_push_block(s)) {
                    return error(lexer);
                }
                push_block(s, (Block)(LIST_ITEM + extra_indentation));
            }
            lexer->result_symbol = dont_interrupt
                                       ? LIST_MARKER_MINUS_DONT_INTERRUPT
                                       : LIST_MARKER_MINUS;
            return true;
        }
        if (minus_count == 3 && (!minus_after_whitespace) && line_end &&
            valid_symbols[MINUS_METADATA]) {
            // Before we start scanning for metadata, peek ahead to check if there's
            // a blank line after the opening ---. If so, this is a horizontal rule.
            // We need to do this without consuming input.

            // Current position: right after the three minuses, at the newline
            // We need to check: is the character after this newline another newline?
            // We can do this by advancing, checking, then either continuing or bailing

            // Advance over the newline to peek at next line
            if (lexer->lookahead == '\r') {
                advance(s, lexer);
                if (lexer->lookahead == '\n') {
                    advance(s, lexer);
                }
            } else if (lexer->lookahead == '\n') {
                advance(s, lexer);
            }

            // Check if we're at another newline (blank line)
            bool is_blank_line = (lexer->lookahead == '\r' || lexer->lookahead == '\n');

            if (is_blank_line) {
                // This is a horizontal rule, not metadata
                // Don't try to parse as metadata.
                // The THEMATIC_BREAK handler should have already been tried.
                // Don't return false here - instead, skip the metadata parsing
                // and let the normal flow continue (which will check 'success' variable)
            } else {

            // Not a blank line, continue with metadata scanning
            // Note: we've already advanced past the first newline above
            bool first_iteration = true;
            for (;;) {
                // On subsequent iterations, advance over the newline
                if (!first_iteration) {
                    if (lexer->lookahead == '\r') {
                        advance(s, lexer);
                        if (lexer->lookahead == '\n') {
                            advance(s, lexer);
                        }
                    } else {
                        advance(s, lexer);
                    }
                }
                first_iteration = false;

                // check for minuses
                minus_count = 0;
                while (lexer->lookahead == '-') {
                    minus_count++;
                    advance(s, lexer);
                }
                if (minus_count == 3) {
                    // if exactly 3 check if next symbol (after eventual
                    // whitespace) is newline
                    while (lexer->lookahead == ' ' ||
                           lexer->lookahead == '\t') {
                        advance(s, lexer);
                    }
                    if (lexer->lookahead == '\r' || lexer->lookahead == '\n') {
                        // if so also consume newline
                        if (lexer->lookahead == '\r') {
                            advance(s, lexer);
                            if (lexer->lookahead == '\n') {
                                advance(s, lexer);
                            }
                        } else {
                            advance(s, lexer);
                        }
                        mark_end(s, lexer);
                        lexer->result_symbol = MINUS_METADATA;
                        return true;
                    }
                }
                // otherwise consume rest of line
                while (lexer->lookahead != '\n' && lexer->lookahead != '\r' &&
                       !lexer->eof(lexer)) {
                    advance(s, lexer);
                }
                // if end of file is reached, then this is not metadata
                if (lexer->eof(lexer)) {
                    break;
                }
            }
            } // end of else block for metadata scanning
        } else if (minus_count == 1 && valid_symbols[CITE_SUPPRESS_AUTHOR_WITH_OPEN_BRACKET]) {
            return parse_cite_suppress_author(s, lexer, valid_symbols);
        }
        if (success) {
            return true;
        }
    }
    return false;
}

static bool parse_pipe_table(Scanner *s, TSLexer *lexer,
                             const bool *valid_symbols) {

    // unused
    (void)(valid_symbols);

    // PIPE_TABLE_START is zero width
    mark_end(s, lexer);
    // count number of cells
    size_t cell_count = 0;
    // also remember if we see starting and ending pipes, as empty headers have
    // to have both
    bool starting_pipe = false;
    bool ending_pipe = false;
    bool empty = true;
    if (lexer->lookahead == '|') {
        starting_pipe = true;
        advance(s, lexer);
    }
    while (lexer->lookahead != '\r' && lexer->lookahead != '\n' &&
           !lexer->eof(lexer)) {
        if (lexer->lookahead == '|') {
            cell_count++;
            ending_pipe = true;
            advance(s, lexer);
        } else {
            if (lexer->lookahead != ' ' && lexer->lookahead != '\t') {
                ending_pipe = false;
            }
            if (lexer->lookahead == '\\') {
                advance(s, lexer);
                if (is_punctuation((char)lexer->lookahead)) {
                    advance(s, lexer);
                }
            } else {
                advance(s, lexer);
            }
        }
    }
    if (empty && cell_count == 0 && !(starting_pipe && ending_pipe)) {
        return false;
    }
    if (!ending_pipe) {
        cell_count++;
    }

    // check the following line for a delimiter row
    // parse a newline
    if (lexer->lookahead == '\n') {
        advance(s, lexer);
    } else if (lexer->lookahead == '\r') {
        advance(s, lexer);
        if (lexer->lookahead == '\n') {
            advance(s, lexer);
        }
    } else {
        return false;
    }
    s->indentation = 0;
    s->column = 0;
    for (;;) {
        if (lexer->lookahead == ' ' || lexer->lookahead == '\t') {
            s->indentation += advance(s, lexer);
        } else {
            break;
        }
    }
    s->simulate = true;
    uint8_t matched_temp = 0;
    while (matched_temp < (uint8_t)s->open_blocks.size) {
        if (match(s, lexer, s->open_blocks.items[matched_temp])) {
            matched_temp++;
        } else {
            return false;
        }
    }

    // check if delimiter row has the same number of cells and at least one pipe
    size_t delimiter_cell_count = 0;
    if (lexer->lookahead == '|') {
        advance(s, lexer);
    }
    for (;;) {
        while (lexer->lookahead == ' ' || lexer->lookahead == '\t') {
            advance(s, lexer);
        }
        if (lexer->lookahead == '|') {
            delimiter_cell_count++;
            advance(s, lexer);
            continue;
        }
        if (lexer->lookahead == ':') {
            advance(s, lexer);
            if (lexer->lookahead != '-') {
                return false;
            }
        }
        bool had_one_minus = false;
        while (lexer->lookahead == '-') {
            had_one_minus = true;
            advance(s, lexer);
        }
        if (had_one_minus) {
            delimiter_cell_count++;
        }
        if (lexer->lookahead == ':') {
            if (!had_one_minus) {
                return false;
            }
            advance(s, lexer);
        }
        while (lexer->lookahead == ' ' || lexer->lookahead == '\t') {
            advance(s, lexer);
        }
        if (lexer->lookahead == '|') {
            if (!had_one_minus) {
                delimiter_cell_count++;
            }
            advance(s, lexer);
            continue;
        }
        if (lexer->lookahead != '\r' && lexer->lookahead != '\n') {
            return false;
        } else {
            break;
        }
    }
    // if the cell counts are not equal then this is not a table
    if (cell_count != delimiter_cell_count) {
        return false;
    }

    lexer->result_symbol = PIPE_TABLE_START;
    return true;
}

// parse_open_square_brace has already advanced the '['
static bool parse_ref_id_specifier(Scanner *s, TSLexer *lexer, const bool *valid_symbols) {
    // unused
    (void)(valid_symbols);
    
    if (lexer->lookahead != '^') {
        return false;
    }
    lexer->advance(lexer, false);

    // https://pandoc.org/MANUAL.html#extension-footnotes
    // The identifiers in footnote references may not contain spaces, tabs, newlines, 
    // or the characters ^, [, or ].
    while (lexer->lookahead != ' ' && lexer->lookahead != '\t' && lexer->lookahead != '\n' &&
           lexer->lookahead != '^' && lexer->lookahead != '['  && lexer->lookahead != ']') {
        lexer->advance(lexer, false);
    }
    if (lexer->lookahead != ']') {
        return false;
    }
    lexer->advance(lexer, false);
    if (lexer->lookahead != ':') {
        return false;
    }
    lexer->advance(lexer, false);
    lexer->mark_end(lexer);
    lexer->result_symbol = REF_ID_SPECIFIER;
    return true;
}

static bool parse_fenced_div_note_id(Scanner *s, TSLexer *lexer,
                                      const bool *valid_symbols) {
    // unused
    (void)(valid_symbols);

    // precondition: lexer->lookahead == '^'
    advance(s, lexer);

    // https://pandoc.org/MANUAL.html#extension-footnotes
    // The identifiers in footnote references may not contain spaces, tabs, newlines,
    // or the characters ^, [, or ].
    while (lexer->lookahead != ' ' && lexer->lookahead != '\t' && lexer->lookahead != '\n' &&
           lexer->lookahead != '^' && lexer->lookahead != '['  && lexer->lookahead != ']') {
        advance(s, lexer);
    }
    lexer->mark_end(lexer);
    lexer->result_symbol = FENCED_DIV_NOTE_ID;
    return true;
}

// Parse code span delimiters for pipe table cells
// This is similar to the inline scanner's parse_backtick but simplified
// since we only need to handle code spans within a single line
static bool parse_code_span(Scanner *s, TSLexer *lexer, const bool *valid_symbols) {
    // Count backticks
    uint8_t level = 0;
    while (lexer->lookahead == '`') {
        lexer->advance(lexer, false);
        level++;
    }
    mark_end(s, lexer);

    // Try to close an open code span
    if (level == s->code_span_delimiter_length && valid_symbols[CODE_SPAN_CLOSE]) {
        s->code_span_delimiter_length = 0;
        s->inside_code_span = 0;
        lexer->result_symbol = CODE_SPAN_CLOSE;
        return true;
    }

    // Try to open a new code span by looking ahead for a matching closing delimiter
    if (valid_symbols[CODE_SPAN_START]) {
        size_t close_level = 0;
        // Look ahead within the same line to find a closing delimiter
        while (!lexer->eof(lexer) && lexer->lookahead != '\n' && lexer->lookahead != '\r') {
            if (lexer->lookahead == '`') {
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
            // Found matching closing delimiter
            s->code_span_delimiter_length = level;
            s->inside_code_span = 1;
            lexer->result_symbol = CODE_SPAN_START;
            return true;
        }
    }

    return false;
}

// Parse latex span delimiters for pipe table cells
// This is similar to parse_code_span but for dollar signs
static bool parse_latex_span(Scanner *s, TSLexer *lexer, const bool *valid_symbols) {
    // Count dollar signs
    uint8_t level = 0;
    while (lexer->lookahead == '$') {
        lexer->advance(lexer, false);
        level++;
    }
    mark_end(s, lexer);

    // Try to close an open latex span
    if (level == s->latex_span_delimiter_length && valid_symbols[LATEX_SPAN_CLOSE]) {
        s->latex_span_delimiter_length = 0;
        s->inside_latex_span = 0;
        lexer->result_symbol = LATEX_SPAN_CLOSE;
        return true;
    }

    // Try to open a new latex span by looking ahead for a matching closing delimiter
    if (valid_symbols[LATEX_SPAN_START]) {
        size_t close_level = 0;
        // Look ahead within the same line to find a closing delimiter
        while (!lexer->eof(lexer) && lexer->lookahead != '\n' && lexer->lookahead != '\r') {
            if (lexer->lookahead == '$') {
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
            // Found matching closing delimiter
            s->latex_span_delimiter_length = level;
            s->inside_latex_span = 1;
            lexer->result_symbol = LATEX_SPAN_START;
            return true;
        }
    }

    return false;
}

// Parse HTML comment: <!-- ... -->
// This must consume everything from <!-- to --> atomically, including
// newlines and what would otherwise be block markers (lists, headings, etc.)
// This is critical for handling comments that span block boundaries.
// parse_html_comment is called from parse_open_angle_brace, which has already consumed '<'
static bool parse_html_comment(TSLexer *lexer, const bool *valid_symbols) {
    if (!valid_symbols[HTML_COMMENT]) {
        return false;
    }

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
    // This includes newlines, list markers, heading markers, etc.
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

static bool parse_open_angle_brace(TSLexer *lexer, const bool *valid_symbols) {
    if (!valid_symbols[AUTOLINK] && !valid_symbols[RAW_SPECIFIER] && !valid_symbols[HTML_COMMENT]) {
        return false;
    }

    // Current position should be '<'
    if (lexer->lookahead != '<') {
        return false;
    }
    lexer->advance(lexer, false);

    if (lexer->lookahead == '!') {
        return parse_html_comment(lexer, valid_symbols);
    }

    // consume all characters until one of:
    // - '}': that was a raw specifier
    // - '>': that was an autolink
    // - ' ', '\t', EOF: that was a bad lex

    while (!lexer->eof(lexer) && lexer->lookahead != ' ' && lexer->lookahead != '\t') {
        if (valid_symbols[RAW_SPECIFIER] && lexer->lookahead == '}') {
            lexer->mark_end(lexer);
            lexer->result_symbol = RAW_SPECIFIER;
            return true;
        } else if (valid_symbols[AUTOLINK] && lexer->lookahead == '>') {
            lexer->advance(lexer, false); // we want to consume '>' for autolinks
            lexer->result_symbol = AUTOLINK;
            return true;
        }
        lexer->advance(lexer, false);
    }
    return false;
}

static bool parse_raw_specifier(TSLexer *lexer, const bool *valid_symbols) {
    if (!valid_symbols[RAW_SPECIFIER]) {
        return false;
    }
    // Current position should be '='
    if (lexer->lookahead != '=') {
        return false;
    }
    lexer->advance(lexer, false);

    // consume all characters until one of:
    // - '}': that was a raw specifier
    // - ' ', '\t', EOF: that was a bad lex

    while (!lexer->eof(lexer) && lexer->lookahead != ' ' && lexer->lookahead != '\t') {
        if (valid_symbols[RAW_SPECIFIER] && lexer->lookahead == '}') {
            lexer->mark_end(lexer);
            lexer->result_symbol = RAW_SPECIFIER;
            return true;
        }
        lexer->advance(lexer, false);
    }
    return false;

}

static bool parse_language_specifier(TSLexer *lexer, const bool *valid_symbols) {
    if (!valid_symbols[LANGUAGE_SPECIFIER] && 
        !valid_symbols[KEY_SPECIFIER] && 
        !valid_symbols[NAKED_VALUE_SPECIFIER]) {
        return false;
    }
    // Current position should be 'A-Za-z'
    if (!((lexer->lookahead >= 'A' && lexer->lookahead <= 'Z') ||
          (lexer->lookahead >= 'a' && lexer->lookahead <= 'z')) &&
        !(valid_symbols[NAKED_VALUE_SPECIFIER] && 
          (lexer->lookahead >= '0' && lexer->lookahead <= '9'))) {
        return false;
    }
    lexer->advance(lexer, false);

    // consume all alphanumeric characters until one of:
    // - '}', EOF: that was a language specifier
    // - '=': that was a key-value key
    // - ' ', '\t': look ahead of whitespace to peek for an '=' to make the call

    do {
        if (
            (lexer->lookahead >= 'A' && lexer->lookahead <= 'Z') ||
            (lexer->lookahead >= 'a' && lexer->lookahead <= 'z') ||
            (lexer->lookahead >= '0' && lexer->lookahead <= '9') ||
            (lexer->lookahead == '_') ||
            (lexer->lookahead == '-')
        ) {
            lexer->advance(lexer, false);
            continue;
        }
        if (lexer->lookahead == '}') {
            lexer->mark_end(lexer);
            if (valid_symbols[NAKED_VALUE_SPECIFIER]) {
                lexer->result_symbol = NAKED_VALUE_SPECIFIER;
            } else {
                lexer->result_symbol = LANGUAGE_SPECIFIER;
            }
            return true;
        }
        if (lexer->lookahead == '=') {
            lexer->mark_end(lexer);
            lexer->result_symbol = KEY_SPECIFIER;
            return true;
        }
        if ((lexer->lookahead == ' ') || (lexer->lookahead == '\t')) {
            lexer->mark_end(lexer);
            while (!lexer->eof(lexer) && ((lexer->lookahead == ' ') || (lexer->lookahead == '\t'))) {
                lexer->advance(lexer, false);
            }
            if (lexer->eof(lexer)) {
                lexer->result_symbol = LANGUAGE_SPECIFIER;
                return true;
            }
            if (lexer->lookahead == '=') {
                lexer->result_symbol = KEY_SPECIFIER;
                return true;
            } else {
                if (valid_symbols[NAKED_VALUE_SPECIFIER]) {
                    lexer->result_symbol = NAKED_VALUE_SPECIFIER;
                } else {
                    lexer->result_symbol = LANGUAGE_SPECIFIER;
                }
                return true;
            }
        }
        return false;
    } while (!lexer->eof(lexer));
    lexer->result_symbol = LANGUAGE_SPECIFIER;
    return true;
}

static bool parse_open_square_brace(Scanner *s, TSLexer *lexer, const bool *valid_symbols) {
    if (lexer->lookahead != '[') {
        return false;
    }
    lexer->advance(lexer, false);
    
    if (valid_symbols[REF_ID_SPECIFIER] && lexer->lookahead == '^') {
        return parse_ref_id_specifier(s, lexer, valid_symbols);
    }

    if (valid_symbols[HIGHLIGHT_SPAN_START] && lexer->lookahead == '!') {
        lexer->advance(lexer, false);
        if (lexer->lookahead != '!') {
            return false;
        }
        lexer->advance(lexer, false);
        lexer->mark_end(lexer);
        while (!lexer->eof(lexer) && (lexer->lookahead == ' ' || lexer->lookahead == '\t')) {
            lexer->advance(lexer, false);
        }
        lexer->result_symbol = HIGHLIGHT_SPAN_START;
        return true;
    }

    if (valid_symbols[INSERT_SPAN_START] && lexer->lookahead == '+') {
        lexer->advance(lexer, false);
        if (lexer->lookahead != '+') {
            return false;
        }
        lexer->advance(lexer, false);
        lexer->mark_end(lexer);
        while (!lexer->eof(lexer) && (lexer->lookahead == ' ' || lexer->lookahead == '\t')) {
            lexer->advance(lexer, false);
        }
        lexer->result_symbol = INSERT_SPAN_START;
        return true;
    }

    if (valid_symbols[DELETE_SPAN_START] && lexer->lookahead == '-') {
        lexer->advance(lexer, false);
        if (lexer->lookahead != '-') {
            return false;
        }
        lexer->advance(lexer, false);
        lexer->mark_end(lexer);
        while (!lexer->eof(lexer) && (lexer->lookahead == ' ' || lexer->lookahead == '\t')) {
            lexer->advance(lexer, false);
        }
        lexer->result_symbol = DELETE_SPAN_START;
        return true;
    }

    if (valid_symbols[COMMENT_SPAN_START] && lexer->lookahead == '>') {
        lexer->advance(lexer, false);
        if (lexer->lookahead != '>') {
            return false;
        }
        lexer->advance(lexer, false);
        lexer->mark_end(lexer);
        while (!lexer->eof(lexer) && (lexer->lookahead == ' ' || lexer->lookahead == '\t')) {
            lexer->advance(lexer, false);
        }
        lexer->result_symbol = COMMENT_SPAN_START;
        return true;
    }

    
    return false;   
}

static bool parse_single_quote(Scanner *s, TSLexer *lexer, const bool *valid_symbols) {
    if (lexer->lookahead != '\'') {
        return false;
    }
    lexer->advance(lexer, false);
    // prioritize close over open so 'word' works as expected.
    if (valid_symbols[SINGLE_QUOTE_CLOSE]) {
        lexer->mark_end(lexer);
        lexer->result_symbol = SINGLE_QUOTE_CLOSE;
        return true;
    }
    if (valid_symbols[SINGLE_QUOTE_OPEN]) {
        lexer->mark_end(lexer);
        lexer->result_symbol = SINGLE_QUOTE_OPEN;
        return true;
    }
    return false;
}

static bool parse_double_quote(Scanner *s, TSLexer *lexer, const bool *valid_symbols) {
    if (lexer->lookahead != '"') {
        return false;
    }
    lexer->advance(lexer, false);
    // prioritize close over open so 'word' works as expected.
    if (valid_symbols[DOUBLE_QUOTE_CLOSE]) {
        lexer->mark_end(lexer);
        lexer->result_symbol = DOUBLE_QUOTE_CLOSE;
        return true;
    }
    if (valid_symbols[DOUBLE_QUOTE_OPEN]) {
        lexer->mark_end(lexer);
        lexer->result_symbol = DOUBLE_QUOTE_OPEN;
        return true;
    }
    return false;
}

static bool parse_shortcode_close(Scanner *s, TSLexer *lexer, const bool *valid_symbols) {
    if (lexer->lookahead != '>') {
        return false;
    }
    lexer->advance(lexer, false);
    if (!valid_symbols[SHORTCODE_CLOSE] && !valid_symbols[SHORTCODE_CLOSE_ESCAPED]) {
        return false;
    }
    if (lexer->eof(lexer) || lexer->lookahead != '}') {
        return false;
    }
    lexer->advance(lexer, false);
    if (lexer->eof(lexer) || lexer->lookahead != '}') {
        return false;
    }
    lexer->advance(lexer, false);
    if (!lexer->eof(lexer) && lexer->lookahead == '}' && valid_symbols[SHORTCODE_CLOSE_ESCAPED]) {
        lexer->advance(lexer, false);
        lexer->mark_end(lexer);
        lexer->result_symbol = SHORTCODE_CLOSE_ESCAPED;
        return true;
    }
    if (!valid_symbols[SHORTCODE_CLOSE]) {
        return false;
    }
    lexer->mark_end(lexer);
    lexer->result_symbol = SHORTCODE_CLOSE;
    return true;
}

static bool parse_shortcode_open(Scanner *s, TSLexer *lexer, const bool *valid_symbols) {
    if (lexer->lookahead != '{') {
        return false;
    }
    lexer->advance(lexer, false);
    if ((!valid_symbols[SHORTCODE_OPEN] && 
         !valid_symbols[SHORTCODE_OPEN_ESCAPED]) || 
         lexer->eof(lexer) || 
         lexer->lookahead != '{') {
        return false;
    }
    lexer->advance(lexer, false);
    if (!lexer->eof(lexer) && lexer->lookahead == '<' && valid_symbols[SHORTCODE_OPEN]) {
        lexer->advance(lexer, false);
        lexer->mark_end(lexer);
        lexer->result_symbol = SHORTCODE_OPEN;
        return true;
    }

    if (lexer->eof(lexer) || lexer->lookahead != '{' || !valid_symbols[SHORTCODE_OPEN_ESCAPED]) {
        return false;
    }

    lexer->advance(lexer, false);
    if (lexer->eof(lexer) || lexer->lookahead != '<' || !valid_symbols[SHORTCODE_OPEN]) {
        return false;
    }
    lexer->advance(lexer, false);
    lexer->mark_end(lexer);
    lexer->result_symbol = SHORTCODE_OPEN_ESCAPED;
    return true;
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

static bool scan(Scanner *s, TSLexer *lexer, const bool *valid_symbols) {
    // Don't parse HTML comments or track math state when inside a fenced code block -
    // these characters should be literal
    bool inside_fenced_code = s->open_blocks.size > 0 &&
                              s->open_blocks.items[s->open_blocks.size - 1] == FENCED_CODE_BLOCK;

    #ifdef SCAN_DEBUG
    printf("valid symbols:\n");    
    for (int i = 0; i < sizeof(token_names) / sizeof(char *); ++i) {
        if (valid_symbols[i]) {
            printf("  %s: %s\n", token_names[i], valid_symbols[i] ? "true" : "false");
        }
    }
    printf("-- scan() state=%d\n", s->state);
    printf("   matching: %s\n", (s->state & STATE_MATCHING) ? "true": "false");
    printf("   inside_fenced_code: %s\n", inside_fenced_code ? "true": "false");
    printf("   lookahead: %c (%d)\n", lexer->lookahead, (int)lexer->lookahead);
    #endif
    // A normal tree-sitter rule decided that the current branch is invalid and
    // now "requests" an error to stop the branch
    if (valid_symbols[TRIGGER_ERROR]) {
        return error(lexer);
    }

    // Handle latex spans for pipe table cells
    // This must come BEFORE the display math state tracking below, so that
    // latex spans in pipe table cells are parsed correctly
    if (lexer->lookahead == '$' && (valid_symbols[LATEX_SPAN_START] || valid_symbols[LATEX_SPAN_CLOSE])) {
        return parse_latex_span(s, lexer, valid_symbols);
    }


    // Handle HTML comments, raw_specifiers, autolinks - must consume atomically to prevent block structure
    // recognition inside comments (e.g., list markers, headings)
    // But NOT inside fenced code blocks where they should be literal
    if (!s->simulate && !(s->state & STATE_MATCHING) && 
        lexer->lookahead == '<' && !inside_fenced_code && 
        (valid_symbols[HTML_COMMENT] || valid_symbols[AUTOLINK] || valid_symbols[RAW_SPECIFIER])) {
        return parse_open_angle_brace(lexer, valid_symbols);
    }
    if (!s->simulate && !(s->state & STATE_MATCHING) && 
        lexer->lookahead == '=' && // this needs to be allowed inside_fenced_code because we're actually inside fenced code..
        (valid_symbols[RAW_SPECIFIER])) {
        #ifdef SCAN_DEBUG
        printf("Attempting to lex RAW_SPECIFIER\n");
        #endif
        return parse_raw_specifier(lexer, valid_symbols);
    }

    // the logic here is tricky. We're trying to see a $$, mark STATE_IN_DISPLAY_MATH
    // and go on. But we can only serialize state if we successfully return an external
    // token.
    //

    // IMPORTANT: Don't process DISPLAY_MATH_STATE_TRACK_MARKER when we're in STATE_MATCHING mode.
    // When matching block continuations (e.g., inside a fenced div), we need to let the block
    // continuation logic run first. Otherwise, we'll consume the $$ before checking if we need
    // to match the block structure, causing a parse error.
    if (!s->simulate && !(s->state & STATE_MATCHING) && lexer->lookahead == '$' &&
        !inside_fenced_code &&
        !s->inside_code_span &&
        valid_symbols[DISPLAY_MATH_STATE_TRACK_MARKER]) {
        advance(s, lexer);
        if (lexer->lookahead == '$') {
            advance(s, lexer);
            s->state ^= STATE_IN_DISPLAY_MATH;
            // printf("-- TOGGLED! it's now %d\n", s->state);
            lexer->mark_end(lexer);
            lexer->result_symbol = DISPLAY_MATH_STATE_TRACK_MARKER;
            return true;
        }
        // this token isn't really used anywhere, but it's here because
        // we can no longer backtrack after advancing the lexer to see if we had a $$.
        lexer->mark_end(lexer);
        lexer->result_symbol = INLINE_MATH_STATE_TRACK_MARKER;
        return true;
    }

    // Handle code spans for pipe table cells
    if (lexer->lookahead == '`' && !valid_symbols[FENCED_CODE_BLOCK_START_BACKTICK] && (
        valid_symbols[CODE_SPAN_START] || valid_symbols[CODE_SPAN_CLOSE])) {
        #ifdef SCAN_DEBUG
        printf("Trying to scan a code span\n");
        #endif
        return parse_code_span(s, lexer, valid_symbols);
    }

    // Close the inner most block after the next line break as requested. See
    // `$._close_block` in grammar.js
    if (valid_symbols[CLOSE_BLOCK]) {
        s->state |= STATE_CLOSE_BLOCK;
        lexer->result_symbol = CLOSE_BLOCK;
        return true;
    }

    // if we are at the end of the file and there are still open blocks close
    // them all
    if (lexer->eof(lexer)) {
        if (valid_symbols[TOKEN_EOF]) {
            lexer->result_symbol = TOKEN_EOF;
            return true;
        }
        if (s->open_blocks.size > 0) {
            // printf("EOF block close\n");
            lexer->result_symbol = BLOCK_CLOSE;
            if (!s->simulate)
                pop_block(s);
            return true;
        }
        return false;
    }

    if (!(s->state & STATE_MATCHING)) {
        // Parse any preceeding whitespace and remember its length. This makes a
        // lot of parsing quite a bit easier.
        for (;;) {
            if (lexer->lookahead == ' ' || lexer->lookahead == '\t') {
                s->indentation += advance(s, lexer);
            } else {
                break;
            }
        }
        // We are not matching. This is where the parsing logic for most
        // "normal" token is. Most importantly parsing logic for the start of
        // new blocks.
        if (valid_symbols[INDENTED_CHUNK_START] &&
            !valid_symbols[NO_INDENTED_CHUNK]) {
            if (s->indentation >= 4 && lexer->lookahead != '\n' &&
                lexer->lookahead != '\r') {
                lexer->result_symbol = INDENTED_CHUNK_START;
                if (!s->simulate) {
                    if (!can_push_block(s)) {
                        return error(lexer);
                    }
                    push_block(s, INDENTED_CODE_BLOCK);
                }
                s->indentation -= 4;
                return true;
            }
        }
        // Decide which tokens to consider based on the first non-whitespace
        // character
        #ifdef SCAN_DEBUG
        printf("before main lookahead switch\n");
        #endif
        switch (lexer->lookahead) {
            case '\r':
            case '\n':
                if (valid_symbols[BLANK_LINE_START]) {
                    // A blank line token is actually just 0 width, so do not
                    // consume the characters
                    lexer->result_symbol = BLANK_LINE_START;
                    return true;
                }
                break;
            case ':':
                return parse_fenced_div_marker(s, lexer, valid_symbols);
            case '`':
                // A backtick could mark the beginning or ending of a fenced
                // code block.
                #ifdef SCAN_DEBUG
                printf("Trying to parse fenced code block\n");
                #endif
                return parse_fenced_code_block(s, '`', lexer, valid_symbols);
            case '~':
                // A tilde could mark the beginning or ending of a fenced code
                // block.
                return parse_fenced_code_block(s, '~', lexer, valid_symbols);
            case '*':
                // A star could either mark  a list item or a thematic break.
                // This code is similar to the code for '_' and '+'.
                return parse_star(s, lexer, valid_symbols);
            case '_':
                return parse_thematic_break_underscore(s, lexer, valid_symbols);
            case '>':
                // A '>' could mark the closing of shortcodes or the beginning of a block quote 
                if (valid_symbols[SHORTCODE_CLOSE] || valid_symbols[SHORTCODE_CLOSE_ESCAPED]) {
                    return parse_shortcode_close(s, lexer, valid_symbols);
                } else {
                    return parse_block_quote(s, lexer, valid_symbols);
                }
            case '#':
                // A '#' could mark a atx heading
                return parse_atx_heading(s, lexer, valid_symbols);
            case '=':
                // A '=' could mark a setext underline
                return parse_setext_underline(s, lexer, valid_symbols);
            case '+':
                // A '+' could be a list marker
                return parse_plus(s, lexer, valid_symbols);
            case '0':
            case '1':
            case '2':
            case '3':
            case '4':
            case '5':
            case '6':
            case '7':
            case '8':
            case '9':
                DEBUG_HERE;
                // A number could be a list marker (if followed by a dot or a
                // parenthesis)

                if (!valid_symbols[NAKED_VALUE_SPECIFIER]) {
                    return parse_ordered_list_marker(s, lexer, valid_symbols);
                }
                break;
            case '-':
                // A minus could mark a list marker, a thematic break, a
                // setext underline, or a cite_suppress_author
                return parse_minus(s, lexer, valid_symbols);
            case '[':
                if (valid_symbols[HIGHLIGHT_SPAN_START] || 
                    valid_symbols[INSERT_SPAN_START] || 
                    valid_symbols[DELETE_SPAN_START] || 
                    valid_symbols[COMMENT_SPAN_START] || 
                    valid_symbols[REF_ID_SPECIFIER]) {
                    return parse_open_square_brace(s, lexer, valid_symbols);
                }
                break;
            case '^':
                if (valid_symbols[FENCED_DIV_NOTE_ID]) {
                    return parse_fenced_div_note_id(s, lexer, valid_symbols);
                }
                break;
            case '(':
                // A '(' could be an example list marker (@)
                return parse_example_list_marker(s, lexer, valid_symbols);
            case '\'':
                return parse_single_quote(s, lexer, valid_symbols);
            case '"':
                return parse_double_quote(s, lexer, valid_symbols);
            case '{':
                if (valid_symbols[SHORTCODE_OPEN] || valid_symbols[SHORTCODE_OPEN_ESCAPED]) {
                    return parse_shortcode_open(s, lexer, valid_symbols);
                }
                break;
            case '@':
                return parse_cite_author_in_text(s, lexer, valid_symbols);
        }
        DEBUG_HERE;
        if (lexer->lookahead != '\r' && lexer->lookahead != '\n' &&
            valid_symbols[PIPE_TABLE_START]) {
            return parse_pipe_table(s, lexer, valid_symbols);
        }
        if ((valid_symbols[LANGUAGE_SPECIFIER] || 
             valid_symbols[KEY_SPECIFIER] || 
             valid_symbols[NAKED_VALUE_SPECIFIER]) &&  
            ((lexer->lookahead >= 'A' && lexer->lookahead <= 'Z') ||
             (lexer->lookahead >= 'a' && lexer->lookahead <= 'z'))) {
            DEBUG_HERE;
            return parse_language_specifier(lexer, valid_symbols);
        }
        DEBUG_HERE;
        if (valid_symbols[NAKED_VALUE_SPECIFIER] && (lexer->lookahead >= '0' && lexer->lookahead <= '9')) {
            DEBUG_HERE;
            return parse_language_specifier(lexer, valid_symbols);
        }
    } else { // we are in the state of trying to match all currently open blocks
        bool partial_success = false;
        while (s->matched < (uint8_t)s->open_blocks.size) {
            if (s->matched == (uint8_t)s->open_blocks.size - 1 &&
                (s->state & STATE_CLOSE_BLOCK)) {
                if (!partial_success)
                    s->state &= ~STATE_CLOSE_BLOCK;
                break;
            }
            if (match(s, lexer, s->open_blocks.items[s->matched])) {
                partial_success = true;
                s->matched++;
            } else {
                if (s->state & STATE_WAS_SOFT_LINE_BREAK) {
                    s->state &= (~STATE_MATCHING);
                }
                break;
            }
        }
        if (partial_success) {
            if (s->matched == s->open_blocks.size) {
                s->state &= (~STATE_MATCHING);
            }
            lexer->result_symbol = BLOCK_CONTINUATION;
            return true;
        }

        if (!(s->state & STATE_WAS_SOFT_LINE_BREAK)) {
            // printf("BLOCK_CLOSE in matching\n");
            lexer->result_symbol = BLOCK_CLOSE;
            pop_block(s);
            if (s->matched == s->open_blocks.size) {
                s->state &= (~STATE_MATCHING);
            }
            return true;
        }
    }

    // The parser just encountered a line break. Setup the state correspondingly
    if ((valid_symbols[LINE_ENDING] || valid_symbols[SOFT_LINE_ENDING] ||
         valid_symbols[PIPE_TABLE_LINE_ENDING]) &&
        (lexer->lookahead == '\n' || lexer->lookahead == '\r')) {
        if (lexer->lookahead == '\r') {
            advance(s, lexer);
            if (lexer->lookahead == '\n') {
                advance(s, lexer);
            }
        } else {
            advance(s, lexer);
        }
        s->indentation = 0;
        s->column = 0;
        if (!(s->state & STATE_CLOSE_BLOCK) &&
            (valid_symbols[SOFT_LINE_ENDING] ||
             valid_symbols[PIPE_TABLE_LINE_ENDING])) {
            lexer->mark_end(lexer);
            for (;;) {
                if (lexer->lookahead == ' ' || lexer->lookahead == '\t') {
                    s->indentation += advance(s, lexer);
                } else {
                    break;
                }
            }
            s->simulate = true;
            uint8_t matched_temp = s->matched;
            s->matched = 0;
            bool one_will_be_matched = false;
            while (s->matched < (uint8_t)s->open_blocks.size) {
                if (match(s, lexer, s->open_blocks.items[s->matched])) {
                    s->matched++;
                    one_will_be_matched = true;
                } else {
                    break;
                }
            }
            bool all_will_be_matched = s->matched == s->open_blocks.size;
            const bool *symbols = s->state & STATE_IN_DISPLAY_MATH ?
                display_math_paragraph_interrupt_symbols :
                paragraph_interrupt_symbols;
            // printf("-- recursive call to scan for closing line. State: %d\n", s->state);
            if (!lexer->eof(lexer) &&
                !scan(s, lexer, symbols)) {
                s->matched = matched_temp;
                // If the last line break ended a paragraph and no new block
                // opened, the last line break should have been a soft line
                // break Reset the counter for matched blocks
                s->matched = 0;
                s->indentation = 0;
                s->column = 0;
                // If there is at least one open block, we should be in the
                // matching state. Also set the matching flag if a
                // `$._soft_line_break_marker` can be emitted so it does get
                // emitted.
                if (one_will_be_matched) {
                    s->state |= STATE_MATCHING;
                } else {
                    s->state &= (~STATE_MATCHING);
                }
                if (valid_symbols[PIPE_TABLE_LINE_ENDING]) {
                    if (all_will_be_matched) {
                        lexer->result_symbol = PIPE_TABLE_LINE_ENDING;
                        return true;
                    }
                } else {
                    lexer->result_symbol = SOFT_LINE_ENDING;
                    // reset some state variables
                    s->state |= STATE_WAS_SOFT_LINE_BREAK;
                    return true;
                }
            } else {
                s->matched = matched_temp;
            }
            s->indentation = 0;
            s->column = 0;
        }
        if (valid_symbols[LINE_ENDING]) {
            // If the last line break ended a paragraph and no new block opened,
            // the last line break should have been a soft line break Reset the
            // counter for matched blocks
            s->matched = 0;
            // If there is at least one open block, we should be in the matching
            // state. Also set the matching flag if a
            // `$._soft_line_break_marker` can be emitted so it does get
            // emitted.
            if (s->open_blocks.size > 0) {
                s->state |= STATE_MATCHING;
            } else {
                s->state &= (~STATE_MATCHING);
            }
            // reset some state variables
            s->state &= (~STATE_WAS_SOFT_LINE_BREAK);
            lexer->result_symbol = LINE_ENDING;
            return true;
        }
    }
    return false;
}

void *tree_sitter_markdown_external_scanner_create(void) {
    Scanner *s = (Scanner *)malloc(sizeof(Scanner));
    s->open_blocks.items = (Block *)calloc(1, sizeof(Block));
#if defined(__STDC_VERSION__) && (__STDC_VERSION__ >= 201112L)
    _Static_assert(ATX_H6_MARKER == ATX_H1_MARKER + 5, "");
#else
    assert(ATX_H6_MARKER == ATX_H1_MARKER + 5);
#endif
    deserialize(s, NULL, 0);

    return s;
}

bool tree_sitter_markdown_external_scanner_scan(void *payload, TSLexer *lexer,
                                                const bool *valid_symbols) {
    Scanner *scanner = (Scanner *)payload;
    scanner->simulate = false;
    return scan(scanner, lexer, valid_symbols);
}

unsigned tree_sitter_markdown_external_scanner_serialize(void *payload,
                                                         char *buffer) {
    Scanner *scanner = (Scanner *)payload;
    return serialize(scanner, buffer);
}

void tree_sitter_markdown_external_scanner_deserialize(void *payload,
                                                       const char *buffer,
                                                       unsigned length) {
    Scanner *scanner = (Scanner *)payload;
    deserialize(scanner, buffer, length);
}

void tree_sitter_markdown_external_scanner_destroy(void *payload) {
    Scanner *scanner = (Scanner *)payload;
    free(scanner->open_blocks.items);
    free(scanner);
}
