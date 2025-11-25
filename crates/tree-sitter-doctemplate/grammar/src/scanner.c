
#include "tree_sitter/parser.h"
#include <assert.h>
#include <ctype.h>
#include <string.h>
#include <wctype.h>

// set this define to turn on debugging printouts
// #define SCAN_DEBUG 1

#ifdef SCAN_DEBUG
#include <stdio.h>
#define DEBUG_HERE printf("(%d) <trace>\n", __LINE__);
#define DEBUG_PRINT(...) do {  \
    printf("(%d) ", __LINE__); \
    printf(__VA_ARGS__);       \
} while (0)
#else
#define DEBUG_HERE
#define DEBUG_PRINT(...)
#endif

#define DEBUG_EXP(FMT, EXPRESSION) DEBUG_PRINT(#EXPRESSION ": " FMT "\n", EXPRESSION)
#define DEBUG_LOOKAHEAD DEBUG_PRINT("   lookahead: (%d) (%c)\n", (int)lexer->lookahead, lexer->lookahead >= 32 ? lexer->lookahead : ' ')

#define EMIT_TOKEN(TOKEN) \
do {                                                        \
    DEBUG_PRINT("external lexer production: " #TOKEN "\n"); \
    lexer->result_symbol = TOKEN; \
    return true; \
} while (0)

#define LEX_CHARACTER(chr) \
do { \
    if (lexer->lookahead != chr) { \
        return false; \
    } \
    lexer->advance(lexer, false); \
} while (0)

#define LEX_STRING(str) \
do { \
    char *ptr = str; \
    while (*ptr) { \
        LEX_CHARACTER(*ptr); \
        ptr++; \
    } \
} while (0)

////////////////////////////////////////////////////////////////////////////////
// External tokens: the order needs to match externals in grammar.js
//
// For explanation of the tokens see grammar.js

typedef enum {
    KEYWORD_FOR_1,
    KEYWORD_FOR_2,
    KEYWORD_ENDFOR_1,
    KEYWORD_ENDFOR_2,
    KEYWORD_IF_1,
    KEYWORD_IF_2,
    KEYWORD_ELSE_1,
    KEYWORD_ELSE_2,
    KEYWORD_ELSEIF_1,
    KEYWORD_ELSEIF_2,
    KEYWORD_ENDIF_1,
    KEYWORD_ENDIF_2
} TokenType;

////////////////////////////////////////////////////////////////////////////////
// Scanner effectively empty struct for now but might be needed in future

typedef struct {
    unsigned own_size;
} Scanner;

static void lex_whitespace(TSLexer *lexer) {
    while (!lexer->eof(lexer) && (lexer->lookahead == ' ' || lexer->lookahead == '\t')) {
        lexer->advance(lexer, false);
    }
}

static bool scan(Scanner *s, TSLexer *lexer, const bool *valid_symbols) {
    (void)(s);
    // KEYWORD_FOR_1: "$", w($), "for"
    // KEYWORD_FOR_2: "${", w($), "for"

    int kw_type = 1;
    LEX_CHARACTER('$');
    if (lexer->lookahead == '{') {
        kw_type = 2;
        LEX_CHARACTER('{');
    }
    lex_whitespace(lexer);
    
    if (lexer->lookahead == 'f') {
        LEX_STRING("for");
        if (kw_type == 1) {
            EMIT_TOKEN(KEYWORD_FOR_1);
        } else {
            EMIT_TOKEN(KEYWORD_FOR_2);
        }
    }
    if (lexer->lookahead == 'e') {
        LEX_CHARACTER('e');
        if (lexer->lookahead == 'l') {
            LEX_STRING("lse");
            if (lexer->lookahead == 'i') {
                LEX_STRING("if");
                if (kw_type == 1) {
                    EMIT_TOKEN(KEYWORD_ELSEIF_1);
                } else {
                    EMIT_TOKEN(KEYWORD_ELSEIF_2);
                }
            } else {
                if (kw_type == 1) {
                    EMIT_TOKEN(KEYWORD_ELSE_1);
                } else {
                    EMIT_TOKEN(KEYWORD_ELSE_2);
                }
            }
        }
        if (lexer->lookahead == 'n') {
            LEX_STRING("nd");
            if (lexer->lookahead == 'i') {
                LEX_STRING("if");
                if (kw_type == 1) {
                    EMIT_TOKEN(KEYWORD_ENDIF_1);
                } else {
                    EMIT_TOKEN(KEYWORD_ENDIF_2);
                }
            }
            if (lexer->lookahead == 'f') {
                LEX_STRING("for");
                if (kw_type == 1) {
                    EMIT_TOKEN(KEYWORD_ENDFOR_1);
                } else {
                    EMIT_TOKEN(KEYWORD_ENDFOR_2);
                }
            }
        }
    }
    if (lexer->lookahead == 'i') {
        LEX_STRING("if");
        if (kw_type == 1) {
            EMIT_TOKEN(KEYWORD_IF_1);
        } else {
            EMIT_TOKEN(KEYWORD_IF_2);
        }
    }
    return false;
}

////////////////////////////////////////////////////////////////////////////////
// tree-sitter API

// Write the whole state of a Scanner to a byte buffer
static unsigned serialize(Scanner *s, char *buffer) {
    unsigned size = 0;
    for (size_t i = 0; i < sizeof(unsigned); i++) {
        buffer[size++] = '\0';
    }
    s->own_size = size;
    return size;
}

// Read the whole state of a Scanner from a byte buffer
// `serizalize` and `deserialize` should be fully symmetric.
static void deserialize(Scanner *s, const char *buffer, unsigned length) {
    (void)(buffer);
    s->own_size = length;
}

void *tree_sitter_doctemplate_external_scanner_create(void) {
    Scanner *s = (Scanner *)malloc(sizeof(Scanner));
    deserialize(s, NULL, 0);

    return s;
}


bool tree_sitter_doctemplate_external_scanner_scan(void *payload, TSLexer *lexer,
                                                const bool *valid_symbols) {
    Scanner *scanner = (Scanner *)payload;
    return scan(scanner, lexer, valid_symbols);
}

unsigned tree_sitter_doctemplate_external_scanner_serialize(void *payload,
                                                         char *buffer) {
    Scanner *scanner = (Scanner *)payload;
    return serialize(scanner, buffer);
}

void tree_sitter_doctemplate_external_scanner_deserialize(void *payload,
                                                       const char *buffer,
                                                       unsigned length) {
    Scanner *scanner = (Scanner *)payload;
    deserialize(scanner, buffer, length);
}

void tree_sitter_doctemplate_external_scanner_destroy(void *payload) {
    Scanner *scanner = (Scanner *)payload;
    free(scanner);
}
