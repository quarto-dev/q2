// Incremental parser combinators.
//
// Same combinators as parsers.ts, with three changes:
//
// 1. A parser takes a `Doc` and a position instead of a string suffix,
//    and returns a `Match`: the number of characters consumed and the
//    tree built for them. Nodes record only `text` and `children`, never
//    absolute positions, so they can be reused unchanged after the
//    document shifts around them.
//
// 2. Every combinator memoizes its result per (parser, position), along
//    with `far`, the furthest position that attempt examined. When the
//    document is spliced (`edit`), entries that examined the spliced
//    region are dropped and entries after it are shifted. Reparsing then
//    returns the *same node objects* for everything the splice did not
//    touch, which is what lets a caret hold on to a node across edits.

// 3. A match contributes a *list* of nodes to its parent, usually one.
//    Transforms change that: `hide` contributes none, `flatten` contributes
//    a node's children in its place, `unwrap` replaces a single-child node
//    by its child. The match keeps its length regardless, so parsing
//    continues correctly. A node's text is the concatenation of its
//    children's texts (or its own characters, for a leaf), so the tree is
//    an AST of the information-bearing parts only; hidden parts are
//    constants of the grammar and can be regenerated from it.
//
export type ParseTree = {
  type: string;
  text: string;
  children: ParseTree[];
};

// What a parser consumed, and the nodes it contributes to its parent.
export type Match = { length: number; trees: ParseTree[] };

export type Parser = (doc: Doc, pos: number) => Match | false;

type Entry = { result: Match | false; far: number };

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
        // The examined extent moves with it.
        next.set(pos + delta, { result: entry.result, far: entry.far + delta });
      }
      // Otherwise it looked at text that changed: drop it.
    }
    doc.cache.set(parser, next);
  }
};

const node = (type: string, text: string, children: ParseTree[]): ParseTree =>
  ({ type, text, children });

const leaf = (type: string, text: string): Match => ({ length: text.length, trees: [node(type, text, [])] });

// A container node over everything the matches contribute.
const container = (type: string, length: number, matches: Match[]): Match => {
  const children = matches.flatMap((m) => m.trees);
  return { length, trees: [node(type, children.map((c) => c.text).join(""), children)] };
};

// Rewrite the nodes a match contributes, keeping its length.
const mapTrees = (p: Parser, f: (trees: ParseTree[]) => ParseTree[]): Parser =>
  memo((doc, pos) => {
    const m = p(doc, pos);
    return m === false ? false : { length: m.length, trees: f(m.trees) };
  });

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
    return doc.text.startsWith(s, pos) ? leaf("str", s) : false;
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
    return test(c) ? leaf(type, c) : false;
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
    const matches: Match[] = [];
    let at = pos;
    for (const p of ps) {
      const m = p(doc, at);
      if (m === false) return false;
      matches.push(m);
      at += m.length;
    }
    return container("seq", at - pos, matches);
  });

// Match the first parser that succeeds.
export const alt = (...ps: Parser[]): Parser =>
  memo((doc, pos) => {
    for (const p of ps) {
      const m = p(doc, pos);
      if (m !== false) return m;
    }
    return false;
  });

// Match zero or more repetitions.
export const many = (p: Parser): Parser =>
  memo((doc, pos) => {
    const matches: Match[] = [];
    let at = pos;
    for (;;) {
      const m = p(doc, at);
      if (m === false || m.length === 0) break;
      matches.push(m);
      at += m.length;
    }
    return container("many", at - pos, matches);
  });

// Match one or more repetitions.
export const many1 = (p: Parser): Parser => {
  const m = many(p);
  return memo((doc, pos) => {
    const r = m(doc, pos);
    return r !== false && r.length > 0 ? r : false;
  });
};

// Match zero or one occurrence.
export const opt = (p: Parser): Parser =>
  memo((doc, pos) => {
    const m = p(doc, pos);
    return m === false ? leaf("opt", "") : m;
  });

// Succeed only if `p` would fail; consumes nothing.
export const not = (p: Parser): Parser =>
  memo((doc, pos) => (p(doc, pos) === false ? leaf("not", "") : false));

// Succeed only at end of input.
export const eof: Parser = memo((doc, pos) => {
  seen(doc, pos + 1);
  return pos === doc.text.length ? leaf("eof", "") : false;
});

// Consume what `p` consumes but contribute no node.
export const hide = (p: Parser): Parser => mapTrees(p, () => []);

// Drop nodes that are empty: a repetition with no items, an optional
// that matched nothing, or anything else with no children and no text.
// Their presence carries no information.
export const dropEmpty = (p: Parser): Parser =>
  mapTrees(p, (trees) => trees.filter((t) => t.children.length > 0 || t.text.length > 0));

// Contribute a node's children in place of the node, so a sequence or
// repetition nested inside another does not add a level.
export const flatten = (p: Parser): Parser => mapTrees(p, (trees) => trees.flatMap((t) => t.children));

// Replace a node that has exactly one child by that child, so an
// optional part that is absent does not leave a wrapper behind.
export const unwrap = (p: Parser): Parser =>
  mapTrees(p, (trees) => trees.flatMap((t) => (t.children.length === 1 ? t.children : [t])));

// Apply `f` to each contributed node on success.
export const transform = (p: Parser, f: (t: ParseTree) => ParseTree): Parser =>
  mapTrees(p, (trees) => trees.map(f));

// Collapse everything contributed into one leaf of type "text" holding the
// concatenated text, e.g. the characters of a name into a single string.
export const text = (p: Parser): Parser =>
  mapTrees(p, (trees) => (trees.length === 0 ? [] : [node("text", trees.map((t) => t.text).join(""), [])]));

// Give each contributed node the type `type`.
export const named = (p: Parser, type: string): Parser => transform(p, (t) => ({ ...t, type }));

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
