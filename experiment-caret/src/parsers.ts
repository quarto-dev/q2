// Minimal parser combinators.
//
// A Parser takes a string and returns either a ParseTree or false.
// The tree records what was matched (`text`), the sub-matches
// (`children`), and the unconsumed input (`rest`) so parsers compose.

export type ParseTree = {
  type: string;
  text: string;
  children: ParseTree[];
  rest: string;
};

export type Parser = (input: string) => ParseTree | false;

const node = (type: string, text: string, children: ParseTree[], rest: string): ParseTree =>
  ({ type, text, children, rest });

// Match a literal string.
export const str = (s: string): Parser => (input) =>
  input.startsWith(s) ? node("str", s, [], input.slice(s.length)) : false;

// Match one character satisfying `test`. The node type is `type`.
const char = (type: string, test: (c: string) => boolean): Parser => (input) => {
  const cp = input.codePointAt(0);
  if (cp === undefined) return false;
  const c = String.fromCodePoint(cp);
  return test(c) ? node(type, c, [], input.slice(c.length)) : false;
};

// Match any single character.
export const any: Parser = char("any", () => true);

// Match one ASCII letter.
export const letter: Parser = char("letter", (c) => (c >= "a" && c <= "z") || (c >= "A" && c <= "Z"));

// Match one ASCII digit.
export const digit: Parser = char("digit", (c) => c >= "0" && c <= "9");

// Match each parser in order.
export const seq = (...ps: Parser[]): Parser => (input) => {
  const children: ParseTree[] = [];
  let rest = input;
  for (const p of ps) {
    const t = p(rest);
    if (t === false) return false;
    children.push(t);
    rest = t.rest;
  }
  return node("seq", input.slice(0, input.length - rest.length), children, rest);
};

// Match the first parser that succeeds.
export const alt = (...ps: Parser[]): Parser => (input) => {
  for (const p of ps) {
    const t = p(input);
    if (t !== false) return t;
  }
  return false;
};

// Match zero or more repetitions.
export const many = (p: Parser): Parser => (input) => {
  const t = p(input);
  if (t === false || t.rest === input) return node("many", "", [], input);
  const m = many(p)(t.rest) as ParseTree;
  return node("many", t.text + m.text, [t, ...m.children], m.rest);
};

// Match one or more repetitions.
export const many1 = (p: Parser): Parser => (input) => {
  const t = many(p)(input);
  return t !== false && t.children.length > 0 ? t : false;
};

// Match zero or one occurrence.
export const opt = (p: Parser): Parser => (input) => {
  const t = p(input);
  return t === false ? node("opt", "", [], input) : t;
};

// Succeed only if `p` would fail; consumes nothing.
export const not = (p: Parser): Parser => (input) =>
  p(input) === false ? node("not", "", [], input) : false;

// Succeed only at end of input.
export const eof: Parser = (input) =>
  input === "" ? node("eof", "", [], "") : false;

// Apply `f` to the resulting tree on success.
export const transform = (p: Parser, f: (t: ParseTree) => ParseTree): Parser => (input) => {
  const t = p(input);
  return t === false ? false : f(t);
};

// Allow recursive grammars: the parser is looked up on each call.
export const lazy = (f: () => Parser): Parser => (input) => f()(input);
