import { Parser } from "./parsers";
import { grammar } from "./language";
import { compile, ruleNames } from "./compile";
import { Caret, layout } from "./caret";
import grammarSource from "./gramma.gramma?raw";

const grammarEl = document.getElementById("grammar") as HTMLTextAreaElement;
const inputEl = document.getElementById("input") as HTMLTextAreaElement;
const treeEl = document.getElementById("tree") as HTMLDivElement;
const caretIndexEl = document.getElementById("caret-index") as HTMLDivElement;

const caret = new Caret(treeEl, (index) => {
  caretIndexEl.textContent = `caret ${index}`;
});

grammarEl.value = grammarSource;
inputEl.value = `A = "a" | B
B = "b"*
`;

// The generated module imports the combinators from the served parsers.ts.
const parsersUrl = new URL("./parsers.ts", import.meta.url).href;

let start: Parser | null = null;

const updateGrammar = async (): Promise<void> => {
  const tree = grammar(grammarEl.value);
  if (tree === false) {
    start = null;
    updateInput();
    return;
  }

  const js = compile(tree, parsersUrl);
  const url = URL.createObjectURL(new Blob([js], { type: "text/javascript" }));
  const mod = await import(/* @vite-ignore */ url);
  URL.revokeObjectURL(url);
  start = mod[ruleNames(tree)[0]];
  updateInput();
};

// On a failed parse the previous tree stays in place, outlined in red.
const updateInput = (): void => {
  const tree = start === null ? false : start(inputEl.value);
  treeEl.classList.toggle("error", tree === false);
  if (tree === false) return;
  treeEl.replaceChildren();
  const lay = layout(tree);
  treeEl.append(lay.root);
  caret.set(lay);
};

grammarEl.addEventListener("input", updateGrammar);
inputEl.addEventListener("input", updateInput);
updateGrammar();
