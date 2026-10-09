// Entry point on the incremental parsers. Each textarea owns a `Doc`;
// an input event becomes a splice (common prefix / suffix against the
// doc's current text) and a reparse, during which untouched nodes are
// reused, so the caret's descriptors keep pointing at live nodes.

import { Doc, Match, ParseTree, Parser, createDoc, edit } from "./inc_parsers";
import { grammar } from "./inc_language";
import { compile, ruleNames } from "./inc_compile";
import { Caret, indexOf } from "./inc_caret";
import { CaretView, layout } from "./inc_caret_view";
import grammarSource from "./gramma.gramma?raw";

const grammarEl = document.getElementById("grammar") as HTMLTextAreaElement;
const inputEl = document.getElementById("input") as HTMLTextAreaElement;
const treeEl = document.getElementById("tree") as HTMLDivElement;
const caretIndexEl = document.getElementById("caret-index") as HTMLDivElement;

// Created once there is a tree to point into.
let caret: Caret | null = null;
let view: CaretView | null = null;

grammarEl.value = grammarSource;
inputEl.value = `A = "a" | B
B = "b"*
`;

const grammarDoc = createDoc(grammarEl.value);
const inputDoc = createDoc(inputEl.value);

// Bring `doc` up to date with `text` by a single splice.
const sync = (doc: Doc, text: string): void => {
  const old = doc.text;
  if (old === text) return;
  let prefix = 0;
  const limit = Math.min(old.length, text.length);
  while (prefix < limit && old[prefix] === text[prefix]) prefix++;
  let suffix = 0;
  while (suffix < limit - prefix && old[old.length - 1 - suffix] === text[text.length - 1 - suffix]) suffix++;
  edit(doc, prefix, old.length - prefix - suffix, text.slice(prefix, text.length - suffix));
};

// The single tree of a top-level match. A start rule that contributes
// several nodes (e.g. via @flatten) gets a synthetic root; one that
// contributes none (everything hidden) has nothing to show.
const rootOf = (m: Match | false): ParseTree | null => {
  if (m === false || m.trees.length === 0) return null;
  if (m.trees.length === 1) return m.trees[0];
  return { type: "root", text: m.trees.map((t) => t.text).join(""), children: m.trees };
};

// The generated module imports the combinators from the served inc_parsers.ts.
const parsersUrl = new URL("./inc_parsers.ts", import.meta.url).href;

let start: Parser | null = null;

const updateGrammar = async (): Promise<void> => {
  sync(grammarDoc, grammarEl.value);
  const tree = rootOf(grammar(grammarDoc, 0));
  if (tree === null) {
    start = null;
    updateInput();
    return;
  }

  let js: string;
  try {
    js = compile(tree, parsersUrl);
  } catch (e) {
    // e.g. an unknown @transform: treat like a grammar that does not parse.
    console.warn(e);
    start = null;
    updateInput();
    return;
  }
  const url = URL.createObjectURL(new Blob([js], { type: "text/javascript" }));
  const mod = await import(/* @vite-ignore */ url);
  URL.revokeObjectURL(url);
  start = mod[ruleNames(tree)[0]];
  // New parser functions: nothing in the old cache can be reused.
  inputDoc.cache.clear();
  updateInput();
};

// On a failed parse the previous tree stays in place, outlined in red.
const updateInput = (): void => {
  sync(inputDoc, inputEl.value);
  const tree = start === null ? null : rootOf(start(inputDoc, 0));
  treeEl.classList.toggle("error", tree === null);
  if (tree === null) return;
  treeEl.replaceChildren();
  const lay = layout(tree);
  treeEl.append(lay.root);
  if (caret === null) {
    caret = new Caret(tree, (c) => {
      const sel = c.selection();
      caretIndexEl.textContent =
        sel === null ? `caret ${indexOf(c.position)}` : `selection ${indexOf(sel[0])}–${indexOf(sel[1])}`;
      view?.draw();
    });
    view = new CaretView(treeEl, caret);
    view.set(lay);
    caret.moveTo(caret.position);
  } else {
    view!.set(lay);
    caret.reparsed(tree);
  }
};

grammarEl.addEventListener("input", updateGrammar);
inputEl.addEventListener("input", updateInput);
updateGrammar();
