// Compile a ParseTree produced by `grammar` in language.ts into JavaScript
// source that builds the same parsers from parsers.ts.
//
// Every rule becomes `export const Name = lazy(() => ...)`, so rules may
// reference each other in any order and recurse freely. `parsersModule` is
// the specifier the generated code imports the combinators from. The names
// `any`, `letter` and `digit` are built in: a grammar may reference them
// without defining them.

import { ParseTree } from "./parsers";

export const compile = (tree: ParseTree, parsersModule = "./parsers.js"): string => {
  const rules = ruleTrees(tree).map(compileRule);
  return [
    `import { str, any, letter, digit, seq, alt, many, many1, opt, not, lazy } from ${JSON.stringify(parsersModule)};`,
    "",
    ...rules,
    "",
  ].join("\n");
};

// The names of the rules, in source order. The first is the start rule.
export const ruleNames = (tree: ParseTree): string[] =>
  ruleTrees(tree).map((rule) => rule.children[0].text);

const ruleTrees = (tree: ParseTree): ParseTree[] =>
  tree.children[1].children.map((entry) => entry.children[0]);

const compileRule = (rule: ParseTree): string => {
  const name = rule.children[0].text;
  const body = compileExpr(rule.children[2]);
  return `export const ${name} = lazy(() => ${body});`;
};

const compileExpr = (t: ParseTree): string => {
  switch (t.type) {
    case "alt": {
      const branches = [t.children[0], ...t.children[1].children.map((c) => c.children[1])];
      return call("alt", branches);
    }
    case "seq":
      return call("seq", t.children);
    case "star":
      return `many(${compileExpr(t.children[0])})`;
    case "plus":
      return `many1(${compileExpr(t.children[0])})`;
    case "opt":
      return `opt(${compileExpr(t.children[0])})`;
    case "not":
      return `not(${compileExpr(t.children[1])})`;
    case "group":
      return compileExpr(t.children[1]);
    case "string":
      return `str(${t.text})`;
    case "ref":
      return t.text;
    default:
      throw new Error(`unknown node type: ${t.type}`);
  }
};

// Emit `fn(a, b, ...)`, or just `a` when there is a single operand.
const call = (fn: string, operands: ParseTree[]): string =>
  operands.length === 1
    ? compileExpr(operands[0])
    : `${fn}(${operands.map(compileExpr).join(", ")})`;
