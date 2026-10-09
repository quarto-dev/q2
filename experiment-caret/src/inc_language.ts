// The grammar for the parser language, on the incremental combinators.
// Same as language.ts except that tokens keep their trailing whitespace
// as a child, so trees are lossless: a node's extent is its text length.
//
//   Expr   = Term (("+" | "-") Term)*
//   Term   = Factor (("*" | "/") Factor)*
//   Factor = Number | "(" Expr ")"
//   Number = digit+
//
// One rule per line. Expressions:
//   "..."      literal string        Name       reference to a rule
//   ( e )      grouping              e1 e2      sequence
//   e1 | e2    alternation           e*  e+  e? repetition
//   !e         negative lookahead
// The names `any`, `letter` and `digit` are built in and match one character.

import { Parser, str, any, letter, digit, seq, alt, many, many1, not, eof, transform, lazy } from "./inc_parsers";

const named = (type: string, p: Parser): Parser =>
  transform(p, (t) => ({ ...t, type }));

const oneOf = (cs: string): Parser => alt(...Array.from(cs, str));

const ws = many(oneOf(" \t"));

// A token: `p` followed by optional horizontal whitespace. The node's
// children are [p, ws]; see `tokText` in inc_compile.ts.
const tok = (p: Parser): Parser => seq(p, ws);

const name = tok(seq(alt(letter, str("_")), many(alt(letter, digit, str("_")))));
const string = named(
  "string",
  tok(seq(str('"'), many(alt(seq(str("\\"), any), seq(not(str('"')), not(str("\\")), any))), str('"'))),
);
const newline = tok(alt(str("\r\n"), str("\n")));

const atom: Parser = alt(
  string,
  named("ref", name),
  named("group", seq(tok(str("(")), lazy(() => alternation), tok(str(")")))),
);

const prefix: Parser = alt(
  named("not", seq(tok(str("!")), atom)),
  atom,
);

const postfix: Parser = alt(
  named("star", seq(prefix, tok(str("*")))),
  named("plus", seq(prefix, tok(str("+")))),
  named("opt", seq(prefix, tok(str("?")))),
  prefix,
);

const sequence: Parser = named("seq", many1(postfix));

const alternation: Parser = named(
  "alt",
  seq(sequence, many(seq(tok(str("|")), sequence))),
);

const rule: Parser = named(
  "rule",
  seq(name, tok(str("=")), alternation, alt(newline, eof)),
);

export const grammar: Parser = named(
  "grammar",
  seq(many(oneOf(" \t\r\n")), many(seq(rule, many(newline))), eof),
);
