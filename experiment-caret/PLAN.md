# Plan: caret, selection and structural editing in the parse-tree view

Implements IDEA.md inside the experiment-caret Vite app. The "rendered
document" is the nested-div parse tree on the right; the "source" is the
input textarea; the AST is the `ParseTree` produced by the compiled grammar.

The UI technique comes from `experiment/advanced-careting`
(`hub-client/src/components/render/q2-debug/entry.tsx`, `sourceOffset.ts`,
`q2-debug.html`). That branch maps Monaco byte offsets onto a React render of
a Pandoc AST. Here the setup is much simpler, so most of its machinery falls
away:

| Branch                                           | Here                                          |
| ------------------------------------------------ | --------------------------------------------- |
| UTF-8 byte offsets from Rust                     | UTF-16 string indices recorded by the parsers |
| escape/entity alignment (`alignSourceAndText`)   | none: leaf `text` *is* the source slice       |
| `data-off` stamps from node `l` fields           | stamps from `start`/`end` on every node       |
| postMessage round trips to Monaco                | direct writes to the textarea string          |
| full reparse via WASM, remap ranges by splices   | incremental reparse; untouched nodes survive  |

What carries over unchanged in spirit: the DOM is never `contenteditable`
and the browser selection is never used; the caret is our own absolutely
positioned div; the selection is an SVG hull layer; a DOM `Range` on a leaf's
text node gives inter-character pixel positions; `caretPositionFromPoint`
resolves clicks; a hidden textarea is the input sink.

## 0. Prerequisites in the tree

`parsers.ts` trees are lossless: concatenating leaf texts in order yields
the matched source exactly (see the earlier discussion). That gives the
*writer* for free and makes every tree whose leaves carry text a "valid AST"
in IDEA.md's sense. The grammar may throw away structure (e.g. a
single-branch `alt` collapses), but never characters, so the sup-isoparser
and the parser coincide. Nothing to build here.

## 1. Parsers record source locations and parse a `Doc` (`parsers.ts`)

Nodes carry their own span instead of being located afterwards. A parser
only ever sees the suffix it is handed, so it cannot know an absolute index
unless it is told where it is. Change the signature to take a document and
a position:

```ts
type ParseTree = { type: string; text: string; children: ParseTree[]; start: number; end: number };
type Parser = (doc: Doc, pos: number) => ParseTree | false;
type Doc = { text: string; cache: Map<Parser, Map<number, Entry>>; far: number };
```

- `start` is `pos`; `end` is `pos + text.length`. `rest` goes away, since
  the parent needs only `end` to continue. The invariant
  `doc.text.slice(start, end) === text` replaces `text + rest === input`.
- Every combinator is the same as today with `pos` threaded through: `seq`
  advances `at = child.end`, `many` recurses at `t.end`, empty nodes
  (`opt`, `not`, `eof`) have `start === end === pos`.
- Each combinator's body is wrapped in a `memo` that caches the result per
  (parser, position) together with `far`, the furthest index that parse
  examined. Primitives report what they looked at (a literal its full
  length, a regex one past its match, `eof` one past the position);
  composites inherit the maximum of their children through `doc.far`. This
  is `inc_parsers.ts` as sketched earlier, now the only `parsers.ts`.
- `language.ts`'s `tok` becomes `{ ...t.children[0], end: t.end }`.
  `compile.ts` is unchanged. `main.ts` keeps one `Doc` per textarea and
  calls `start(doc, 0)`.

## 2. Incremental reparse: source splice → minimal AST change (`src/doc.ts`)

Every source change arrives as a splice `{ at, removed, inserted }`, from
one of two places: the textarea's `input` event (derive the splice from the
old and new value by common prefix/suffix, or from `beforeinput` plus the
textarea selection), or an AST edit (step 8), which produces splices
directly. Both go through the same path:

1. `edit(doc, at, at + removed, inserted)`: replace the text, then walk the
   memo cache once. Entries whose examined span `[pos, far)` overlaps the
   splice are dropped; entries at or after the splice end are kept with
   their position shifted by the length delta.
2. `start(doc, 0)` again. Every parser whose entry survived returns the
   **same node object** as before; only parsers whose entries were dropped
   run again and build fresh nodes. The set of fresh nodes is the minimal
   AST change: the leaves touched by the splice and the chain of containers
   above them, nothing else. (`many` is memoised at every suffix, so an
   edit in the middle of a repetition rebuilds the prefix entries, whose
   `far` reaches the edit, and reuses the tail.)
3. One walk over the new tree fixes up reused nodes after the splice, whose
   `start`/`end` are still in pre-edit coordinates, by adding the delta, and
   collects a `Set<ParseTree>` of live nodes. (Alternative: shift lazily on
   cache hit. Same cost, harder to explain; do the walk.)

Nodes before the splice are untouched, nodes after it are the same objects
with shifted spans, and that is what makes cursors stable for free: a
`Position` points at a node object, so it stays valid across any edit that
did not rebuild that node. Step 5 covers the case where it did.

Correctness caveat carried over from the sketch: a regex that looks ahead
further than one character past its match can be reused when it should not
be. Acceptable for this experiment; `far` could be widened for such
patterns later.

## 3. Positions and indices (`src/positions.ts`)

Positions *inside* a leaf's text are not nodes. The caret needs them, so:

```ts
type Position = { node: ParseTree; side: "before" | "after" }
              | { leaf: ParseTree; char: number };   // 0 <= char <= text.length
```

with `strIndexOf(position)` reading `start`/`end` directly off the node.
A `char` of 0 or `text.length` is the same string index as the leaf's
before/after side, which is the "identify multiple indices" case from
IDEA.md. Keep them distinct in the model; collapse only in the cursoring
layer (step 5).

- **AST indices** as in IDEA.md: every node has a *before* and an *after*
  index, and a node's after index is identified with the next sibling's
  before index. Enumerate them in document order so each gets an integer.
  Store `astIndex -> { node, side }` and the reverse. Rebuilt after each
  reparse; cheap, and only the mapping functions use it.
- `iStrFromiAst(i): number` — `side === "before" ? node.start : node.end`.
- `iAstFromiStr(s): number[]` — all AST indices whose string index is `s`
  (the surjection's fibre). For `hello` this returns `[0, 1, 2]` at string
  index 0, exactly the example in IDEA.md.
- `write(node): string` — leaf text, or children written in order.

## 4. Render with stamps (`main.ts` → split out `src/render.ts`)

Keep the current nested-div renderer and add:

- `data-str="start-end"` on every node div (the branch's `data-off`), read
  straight from the node, and `data-ast="before-after"`.
- Leaf text goes in a child `span[data-text]` whose *only* child is a text
  node, mirroring `DATA_STR_TEXT`. The `Range` and hit-test code depend on
  this shape.
- A `WeakMap<ParseTree, HTMLElement>` and the reverse, so positions resolve
  to elements without re-querying by index. Because reused nodes are the
  same objects, the renderer can skip subtrees whose node is already mapped
  to a connected element and only re-render fresh nodes; a full re-render
  is fine to start with.
- Container CSS from `q2-debug.html`: `#tree { position: relative;
  user-select: none }`, `[data-text] { cursor: text }`.

## 5. Caret model, stops, movement (`src/cursor.ts`)

- `Caret = { head: Position; anchor: Position }`. Selection is
  `[min, max)` of their string indices; empty when they coincide.
- **Stable identity** comes from step 2: a `Position` holds a node object,
  and any edit elsewhere leaves that object in the tree with an updated
  span. After each reparse, check each cursor's node against the live set.
  If it is gone (its subtree was rebuilt or deleted), relocate: map the
  cursor's old string index through the splice (a position inside the
  removed range collapses to the splice start; one after it shifts by the
  delta, as in the branch's `mapOffset`), then `iAstFromiStr` on the new
  tree and pick a candidate, preferring a leaf `char` position and a node
  of the same `type` as the old one. So an index is only ever the
  *bridge* when the stable reference breaks; it is never the primary
  representation.
- **Stops**: every caret-visitable position in document order: each char
  boundary of every leaf, plus before/after of textless leaves (port
  `caretStops`). Default identification rule: positions with the same string
  index collapse to one stop, preferring `{ leaf, char }` over
  `{ node, side }`. Keep the raw (uncollapsed) list available so a modifier
  key can later step through the identified positions one by one; IDEA.md
  wants granular control retained.
- `step(position, dir)`, `moveCaret(dir, extend)`: port `stepCaret` and
  `moveCaret` (collapse to the selection edge on an unshifted arrow).

## 6. Caret and selection painting (`src/caret.ts`)

Port from `entry.tsx` with the byte/escape layers removed:

- **Caret element**: one `div.caret` appended to `#tree`, `position:
  absolute`, 2px wide, blink animation, `.unfocused` and `.hidden` states
  (CSS verbatim from `q2-debug.html`).
- **Placing the caret** (`placeCaret`): for a `{ leaf, char }` position,
  `document.createRange()`, `setStart(textNode, char)`, `setEnd` the same,
  take `getClientRects()[0]`, with the branch's fallback to the host's
  left/right edge when the collapsed rect is empty. For a `{ node, side }`
  position, use the node element's bounding box left or right edge. Convert
  to `#tree` scroll space: `rect.left - rootRect.left + root.scrollLeft`.
- **Selection layer** (`paintSelection` + `hullPaths`): an `<svg>` sized to
  `scrollWidth`/`scrollHeight` under `#tree`. For a selection `[S, E)` in
  string indices: each leaf overlapping it contributes the client rects of a
  `Range` over the overlapped sub-range of its text node; textless leaves
  contribute their box; containers contribute nothing. `hullPaths` is ported
  as-is: group boxes into rows by source order, join neighbours with
  diagonals so a row of boxes of different heights reads as one highlight.
  Fill/stroke CSS from `.q2-debug-selection-hull` (translucency is the point
  here, as the branch notes).
- Coalesce painting onto `requestAnimationFrame`; repaint after every render
  and on scroll/resize.

## 7. Clicks and drags (`src/pointer.ts`)

Port `caretHitFromPoint`, `leaves`, `nearestLeaf`, `offsetInLeaf`,
`sourceOffsetAtPoint` and the pointer handlers:

- Resolve a pointer to the nearest *leaf* (vertical distance first, then
  horizontal), never a container; inside a leaf clamp x into the nearest
  line rect and ask `caretPositionFromPoint` (`caretRangeFromPoint` on
  WebKit) for the character index; otherwise snap to the nearer edge.
- `pointerdown` places head and anchor (shift extends from the anchor);
  `pointermove` with pointer capture moves the head, with the row-tie
  preference for the anchor's row; `pointerup` ends the drag.
- Cache leaf rects per render, invalidate on scroll/resize.

## 8. Input sink, keys, and AST edits (`src/input.ts`, `src/edit.ts`)

A hidden `textarea` parked on the caret, focused on click. `beforeinput`
handles `insertText`, `insertFromPaste`, `deleteContentBackward`,
`deleteContentForward`; `keydown` handles arrows with shift. Everything else
is `preventDefault`'d for now, as on the branch.

Each edit is expressed on the AST and reduced to splices in pre-edit string
indices, computed from the nodes' `start`/`end`:

- **type(text)** at a position: host preference as on the branch — the leaf
  containing the position, then the leaf ending there, then the one starting
  there. Splice is `{ at, 0, text }`.
- **backspace / delete** at `{ leaf, char }`: one character of the leaf. At
  a `{ node, side: "after" }` position backspace removes `node` whole
  (IDEA.md: "delete the before node"): splice `{ node.start,
  node.end - node.start, "" }`. Keep this as a lookup `rules[nodeType]` so
  per-type behaviour (join instead of delete, `&` in a matrix) can be added
  without touching the core.
- **selection delete**: port `deleteFromNode` — nodes fully inside `[S, E)`
  go; straddling leaves are trimmed; straddling containers are kept and the
  rule recurses. One splice per removed node or trimmed slice, so a
  container's own syntax survives.
- **commit**: feed the splices (right-to-left) through step 2, re-resolve
  the cursors (step 5), render, repaint. The AST is not mutated by hand: the
  AST edit only *determines* the splice, and the incremental reparse
  rebuilds exactly the touched nodes. That closes IDEA.md's loop in both
  directions with one mechanism: AST edit → minimal source edit (the
  splices) → minimal AST edit (the reparse), and a source edit in the
  textarea takes the second half of the same path.

## 9. Later

- **Structural copy/paste**: closest common ancestor of the selected nodes,
  slice the subtree between the selection ends, write it with `write`.
- **Per-type editing rules and selection shapes** (rectangular selection for
  graphic-like nodes), as flagged in IDEA.md's open questions.
- **Tighter `far` for regexes** with lookahead, if a grammar ever needs it.
- The grammar textarea could drive the same machinery, since the language
  parses itself; not needed for the first pass.

## Order of work

1 → 2 (verify by typing in the textarea and logging which nodes are fresh)
→ 3 → 4 → 6 (caret visible at a hard-coded position) → 7 (click to place)
→ 5 (arrows, shift, selection hull, stable cursors across textarea edits)
→ 8. Each step is independently checkable in the browser.
