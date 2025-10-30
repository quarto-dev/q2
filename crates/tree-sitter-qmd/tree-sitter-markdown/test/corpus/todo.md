TODO:

- (span openings need to be in the external scanner because of the negative lookahead assertion against `[^`, `[!!`, etc)

  - maybe not? this is now working

- lexer->advance() vs advance(s, lexer)?

blocks:

- metadata in blockquotes (:yikes:)
- tables

inlines:

- underline, emphasis, superscript etc
- citations
- footnotes
- quotes
- shortcodes
