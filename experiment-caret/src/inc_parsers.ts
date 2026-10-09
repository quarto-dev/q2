// Incremental parser combinators.
//
// Same combinators as parsers.ts, with two changes:
//
// 1. A parser takes a `Doc` and a position instead of a string suffix.
//    Nodes record only `text` and `children`; a node's extent is its
//    text length, so nodes never carry absolute positions and can be
//    reused unchanged after the document shifts around them.
//
// 2. Every combinator memoizes its result per (parser, position), along
//    with `far`, the furthest position that attempt examined. When the
//    document is spliced (`edit`), entries that examined the spliced
//    region are dropped and entries after it are shifted. Reparsing then
//    returns the *same node objects* for everything the splice did not
//    touch, which is what lets a caret hold on to a node across edits.

export type ParseTree = {
  type: string;
  text: string;
  children: ParseTree[];
};

export type Parser = (doc: Doc, pos: number) => ParseTree | false;

type Entry = { result: ParseTree | false; far: number };

export type Doc = {
  text: string;
  cache: Map<Parser, Map<number, Entry>>;
  // The furthest position examined by the parse currently in progress.
  far: number;
};

export const createDoc = (text: string): Doc => ({ text, cache: new Map(), far: 0 });

// Replace `removed` characters at `at` with `inserted`, and fix up the
// cache so the next parse reuses everything the splice did not touch.
export const edit = (doc: Doc, at: number, removed: number, inserted: string): void => {
  doc.text = doc.text.slice(0, at) + inserted + doc.text.slice(at + removed);
  const delta = inserted.length - removed;
  for (const [parser, table] of doc.cache) {
    const next = new Map<number, Entry>();
    for (const [pos, entry] of table) {
      if (entry.far <= at) {
        // Examined only text before the splice.
        next.set(pos, entry);
      } else if (pos >= at + removed && pos > at) {
        // Started after the splice: what it saw is unchanged, just moved.
        next.set(pos + delta, entry);
      }
      // Otherwise it looked at text that changed: drop it.
    }
    doc.cache.set(parser, next);
  }
};

const node = (type: string, text: string, children: ParseTree[]): ParseTree =>
  ({ type, text, children });

// Record that the current parse looked at text up to (not including) `upto`.
const seen = (doc: Doc, upto: number): void => {
  if (upto > doc.far) doc.far = upto;
};

const memo = (p: Parser): Parser => {
  const m: Parser = (doc, pos) => {
    let table = doc.cache.get(m);
    if (table === undefined) {
      table = new Map();
      doc.cache.set(m, table);
    }
    const hit = table.get(pos);
    if (hit !== undefined) {
      seen(doc, hit.far);
      return hit.result;
    }
    const outer = doc.far;
    doc.far = pos;
    const result = p(doc, pos);
    const far = doc.far;
    table.set(pos, { result, far });
    doc.far = Math.max(outer, far);
    return result;
  };
  return m;
};

// Match a literal string.
export const str = (s: string): Parser =>
  memo((doc, pos) => {
    seen(doc, pos + s.length);
    return doc.text.startsWith(s, pos) ? node("str", s, []) : false;
  });

// Match one character satisfying `test`. The node type is `type`.
const char = (type: string, test: (c: string) => boolean): Parser =>
  memo((doc, pos) => {
    const cp = doc.text.codePointAt(pos);
    if (cp === undefined) {
      seen(doc, pos + 1);
      return false;
    }
    const c = String.fromCodePoint(cp);
    seen(doc, pos + c.length);
    return test(c) ? node(type, c, []) : false;
  });

// Match any single character.
export const any: Parser = char("any", () => true);

// Match one ASCII letter.
export const letter: Parser = char("letter", (c) => (c >= "a" && c <= "z") || (c >= "A" && c <= "Z"));

// Match one ASCII digit.
export const digit: Parser = char("digit", (c) => c >= "0" && c <= "9");

// Match each parser in order.
export const seq = (...ps: Parser[]): Parser =>
  memo((doc, pos) => {
    const children: ParseTree[] = [];
    let at = pos;
    for (const p of ps) {
      const t = p(doc, at);
      if (t === false) return false;
      children.push(t);
      at += t.text.length;
    }
    return node("seq", doc.text.slice(pos, at), children);
  });

// Match the first parser that succeeds.
export const alt = (...ps: Parser[]): Parser =>
  memo((doc, pos) => {
    for (const p of ps) {
      const t = p(doc, pos);
      if (t !== false) return t;
    }
    return false;
  });

// Match zero or more repetitions.
export const many = (p: Parser): Parser =>
  memo((doc, pos) => {
    const children: ParseTree[] = [];
    let at = pos;
    for (;;) {
      const t = p(doc, at);
      if (t === false || t.text.length === 0) break;
      children.push(t);
      at += t.text.length;
    }
    return node("many", doc.text.slice(pos, at), children);
  });

// Match one or more repetitions.
export const many1 = (p: Parser): Parser => {
  const m = many(p);
  return memo((doc, pos) => {
    const t = m(doc, pos);
    return t !== false && t.children.length > 0 ? t : false;
  });
};

// Match zero or one occurrence.
export const opt = (p: Parser): Parser =>
  memo((doc, pos) => {
    const t = p(doc, pos);
    return t === false ? node("opt", "", []) : t;
  });

// Succeed only if `p` would fail; consumes nothing.
export const not = (p: Parser): Parser =>
  memo((doc, pos) => (p(doc, pos) === false ? node("not", "", []) : false));

// Succeed only at end of input.
export const eof: Parser = memo((doc, pos) => {
  seen(doc, pos + 1);
  return pos === doc.text.length ? node("eof", "", []) : false;
});

// Apply `f` to the resulting tree on success.
export const transform = (p: Parser, f: (t: ParseTree) => ParseTree): Parser =>
  memo((doc, pos) => {
    const t = p(doc, pos);
    return t === false ? false : f(t);
  });

// Allow recursive grammars: the parser is built on first use, after every
// rule constant exists. It is built only once, so the combinators inside
// keep their identity and their cache entries across parses.
export const lazy = (f: () => Parser): Parser => {
  let p: Parser | null = null;
  return memo((doc, pos) => {
    if (p === null) p = f();
    return p(doc, pos);
  });
};
