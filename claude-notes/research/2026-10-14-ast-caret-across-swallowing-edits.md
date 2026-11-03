# Keeping the AST caret across "swallowing" intermediate states (2026-10-14)

Design options for the q2-debug AST caret
(`hub-client/src/components/render/q2-debug/dom/caret.ts`) when a single
keystroke restructures a large part of the document, typically as an
intermediate state the user will undo a moment later by closing a
delimiter. Nothing here is implemented; this is the menu.

## The problem

The caret is `(node, offset)`: a text-bearing AST node plus a character
offset. It survives edits because the reconciler patches the AST in place
and the applier reports text changes, splits, merges and deletions, which
the caret maps through (`migrate` in `caret.ts`).

Typing an opening ` ``` ` above the caret's paragraph makes the parser
swallow everything after it into one `CodeBlock`. The differ, correctly,
deletes every later block and inserts the code block. The caret's `Str` is
inside a removed subtree, so the caret is handed to the nearest surviving
neighbour — usually the end of the paragraph before the fence. Closing the
fence a moment later inserts *fresh* paragraph objects: the text is back
but the caret cannot recognise its old node, so it stays where it was
parked. The same shape arises from an unclosed `<!--`, a stray `<` turning
a paragraph into raw HTML, or an unclosed `$$` swallowing text into a
`Math` leaf. Fenced divs do **not** have this problem: the differ wraps and
unwraps them with identity intact.

Two goals:

1. After the delimiter is closed, the caret is back on the node and offset
   it had before, as if nothing happened.
2. (Bonus) While the swallowing block exists, the caret is shown at the
   corresponding spot *inside* the block's text, and follows edits there.

## Design 1 — identity resurrection in the reconciler

Keep deleted subtrees in a tombstone cache for a few updates, keyed by
structural hash. When a later edit script inserts a subtree whose hash
matches a tombstone, the applier splices in the **old object** instead of
the fresh one, then syncs annotations from the fresh parse exactly as it
does today. The caret's node literally returns to the tree; no mapping is
needed.

- Match whole subtrees first, then nodes within a matched parent, so a
  common word (`Str("the")`) never resurrects across unrelated paragraphs.
- Also fixes cut-and-paste, undo/redo, and every other "block briefly
  vanishes" case. Fits the stable-AST philosophy: a node that reappears
  unchanged *is* the same node.
- Gaps: gives the caret nowhere to live during the intermediate state, and
  fails if the user edited the swallowed text while it was a code block
  (hashes no longer match on release).
- Where: `astdiff/apply.ts` (consult the cache on `insert`) and
  `astdiff/reconcile.ts` (populate it from `removedNodes`, expire after N
  updates).

## Design 2 — text projection between a leaf and the subtree it replaced

Define a *flattening* of a block sequence to plain text: inline text
concatenated (`textOf`), blocks joined by blank lines, list items prefixed.
A swallowed region's flattened text is nearly identical to the code block's
text — code text is verbatim source, inline text is source minus markup.

- **Swallow.** When the caret's node is deleted and the same script inserts
  or grows a text leaf, align the flattened deleted subtree against the
  leaf's text and map the caret's flattened offset across. The alignment is
  a character diff and the mapping is exactly `mapOffsetThroughChange`
  (`dom/textDiff.ts`), which exists. The caret is then **parked** on the
  leaf as an ordinary text position — this is the bonus: it draws inside
  the code text, and edits inside the block move it like any text cursor
  because that path already works.
- **Release.** The leaf's text shrinks by a `change_text` and subtrees are
  inserted. Align the removed portion of the code text against the
  flattened inserted subtrees, map back, and locate the flattened offset
  in the node sequence to recover `(node, offset)`.
- Handles text edited while swallowed, since alignment tolerates
  differences. Weakness: approximate where markup was heavy (links, code
  spans, math) — the caret may land a character or two off.
- Where: `caret.ts` (`park` / `unpark`), plus a `flatten(nodes)` helper
  next to `textDiff.ts`. Detection heuristic: caret node ∈ a removed
  subtree **and** the script inserted/changed a text leaf.

## Design 3 — source-offset parking

Every node carries fresh `l` source offsets after each update (annotations
are synced). When the caret's node is deleted, convert it to a source
offset (node start + text→source alignment, as in the
`experiment/advanced-careting` branch's `sourceOffset.ts`), keep that
offset while parked, and on each update look for a text-bearing node whose
span covers it to re-attach. Inside the code block: offset − block start −
fence line gives the position in the code text.

- This is the experiment branch's model, used **only as a fallback** while
  parked; the primary caret stays AST-attached.
- Catch: the parked offset must be adjusted for edits made elsewhere while
  parked, and the iframe sees only ASTs, not editor deltas. Needs either
  the editor's text deltas in the `UPDATE_AST` message or an approximation
  from the code block's own text changes.

## Recommendation

Designs 1 and 2 together:

- **Resurrection is the ground truth.** When the original node comes back
  unchanged, the caret is restored exactly.
- **Projection provides the parked position** (the bonus) and the fallback
  when the swallowed text was edited.

The caret would carry an `attached` position and an optional `parked`
record `{ leaf, offset, original }`, preferring `original` on release when
resurrection brings it back, else reverse-projecting `offset`, else the
existing neighbour hand-off. Both mechanisms are AST-only and generalise
to every swallowing delimiter listed above.

## Related

- `claude-notes/research/2026-10-14-astdiff-fuzz-findings.md` — which
  insertions produce large diffs (the swallowing cases are the
  "legitimate" ones there).
