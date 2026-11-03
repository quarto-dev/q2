# AST differ fuzz findings (2026-10-14)

Results of fuzzing the TypeScript AST differ
(`ts-packages/preview-renderer/src/astdiff/`) with simple insertions into
parseable qmd documents, and what they say about where the differ falls
back to delete + insert instead of describing the edit structurally.

## The fuzzer

`ts-packages/preview-renderer/scripts/astdiff-fuzz.ts`

```bash
cd ts-packages/preview-renderer
npx tsx scripts/astdiff-fuzz.ts                      # defaults: 20 000 trials, top 30
npx tsx scripts/astdiff-fuzz.ts --seed lists --positions all --top 10
npx tsx scripts/astdiff-fuzz.ts --dump /tmp/large.json   # every large case, as JSON
```

Options: `--pampa <path>` (default `../../target/debug/pampa`), `--max <n>`,
`--top <n>`, `--seed <name>`, `--positions boundaries|all`, `--workers <n>`,
`--dump <file>`.

Method:

- Five seed documents: prose (headers, emphasis, links, code), nested
  bullet and ordered lists, fenced div + block quote + code block + rule,
  the nested-div document from the q2-debug sessions, and footnote + math
  + pipe table.
- 64 tokens: single characters (` `, `a`, `#`, `*`, `-`, `>`, `:`, `` ` ``,
  `[`, `|`, `$`, `<`, `{`, …) and common sequences (`- `, `1. `, `> `,
  `# `, `::: {}\n`, `:::\n`, ` ``` `, `**`, `](`, `![`, `<!-- `, `[^1]`,
  `{{< `, `\n> `, `\n# `, `\n- `, `\n\n`, …).
- Each token is inserted at every line boundary and word boundary of every
  seed (`--positions all` tries every offset). The edited document is
  parsed with the pampa CLI — the same parser the hub-client WASM runs. If
  it parses, the fresh AST is reconciled against a copy of the seed's AST.
- Score: **churn** = nodes (objects with a `t`) inside inserted subtrees +
  inside deleted subtrees + created nodes + changed nodes. A trial is
  "large" when churn exceeds the token's own worth (a few nodes) by more
  than four, or removes three or more nodes.

## Run summary

| | |
|---|---|
| trials | 20 000 |
| parsed | 12 857 (7 143 did not parse) |
| reconcile fallbacks | 11 before the fix below, 0 after |
| large diffs | 1 995 (15.5 % of parsed) |
| wall time | ~2.5 min, 8 workers |

## Bug found and fixed

Eleven trials hit the reconcile fallback (the applied script did not
reproduce the fresh parse). Cause: `split` shallow-cloned the node, so the
two halves of a split `OrderedList` shared the list-attributes array
(`c[0]`); a later `change start 1→2` on the second half wrote through to
the first. `cloneShell` in `astdiff/apply.ts` now also copies the arrays
directly inside `c` (list attributes, link targets, captions). Child nodes
are still shared, which is intended — the main child list is replaced.

## Large diffs that are legitimate (~⅓ of flagged cases)

The document really did change shape; delete + insert is the honest
description.

| insertion | effect |
|---|---|
| `<!-- ` anywhere | rest of the document becomes a raw HTML comment (305 cases) |
| ` ``` ` / ` ```\n ` | opens a code block to end of document (74) |
| `<` at line start | rest of the paragraph becomes raw HTML (73) |
| `\n:::\n` inside a div | closes the div early; everything after moves out (45) |
| any character in a table row or its `|---|` line | table becomes a paragraph (~90) |
| any character before `1. one` | ordered list becomes a paragraph (41) |
| any character before a closing code fence | code block swallows the rest (57) |

## Large diffs that are differ weaknesses

Ordered by number of cases.

### 1. Line break + block marker typed mid-paragraph (~500 cases)

Tokens `\n> `, `\n# `, `\n\n# `, `\n- ` inside a paragraph. The tail of
the paragraph is deleted and re-inserted inside a BlockQuote / Header /
BulletList item.

```
[prose] insert "\n> " at L3:1  "A"|" first paragraph with *emphasis*, …"
  churn 48 — 10× delete inline, then insert BlockQuote (whole subtree)
```

Cause: `splitPoint` accepts a following node as evidence only when it is
of the **same kind** as the split node (or a same-kind node reached
through single-child wrappers). Here the tail reappears as a Para inside
a BlockQuote, as a Header, or as a Plain inside a list item. Plain Enter
mid-paragraph is fine today because both halves are paragraphs.

Mirror case: Backspace joining a quoted / listed / header line back into
the previous paragraph (`mergeTarget` has the same same-kind restriction).

### 2. Typing at the start of a list item line (~300 cases)

Any character before `- nested two` makes that line a continuation of the
previous item: its words move into the previous item's Plain, joined by a
SoftBreak.

```
[lists] insert "a" at L6:0  ""|"  - nested two"
  churn 12 — insert SoftBreak, Str("a-"), Space, Str("nested"), … ; delete item
```

Cause: this is a merge whose boundary falls **inside the last child** of
the two items (`Plain` + `Plain`), not between siblings. Both the split
and the merge prefix tests are strictly sibling-level: `x.children` must
be a prefix of `y.children` as a whole.

### 3. A space before a list item flattens its nested list (~50 cases)

```
[lists] insert " " at L4:0  ""|"- second item with *emph*"
  churn 17 — delete nested BulletList; insert item, insert item
```

The nested items should be released with a split of the item after its
first block plus an unwrap of the nested list through `content[0]/items`.
Cause: evidence flattening (`flattenWrappers`) only descends through
**single-child** wrapper chains, and the nested list has two items, so the
nested items are never seen as "released".

### 4. Unrelated same-kind nodes get paired (low churn, misleading)

When a structural change lines up two paragraphs that share nothing, the
gap walker pairs them by kind and emits word-by-word text edits:

```
[nested] insert "<" at L3:0
  change text "stuff!!!"→"we", "this"→"are", "is"→"quoting", …
```

Cause: same-kind pairing in `diffGap` has no similarity requirement.
Delete + insert would be the honest description when nothing is shared.

## Proposed fixes

The first three weaknesses share one shape; fixing them together is
natural, with the fuzzer as the regression gate.

1. **Evidence through all descendants.** `flattenWrappers` (used for
   released / absorbed evidence) should walk every main-slot descendant,
   not only single-child chains. The `unwrap`/`wrap` chain itself keeps
   the single-child requirement; only the evidence loosens. (Fixes 3.)
2. **Cross-kind neighbours in split/merge evidence.** Accept a following
   (split) or preceding (merge) node of a different kind whose main slot
   has the same child element kind and starts with the remainder; the
   existing type-change (unwrap + wrap) and wrap-chain rules then finish
   the job. (Fixes 1 and its mirror.)
3. **Deep prefix.** Let the split/merge boundary recurse into the last
   child: `x` is a deep prefix of `y` when all but the last child match
   and the last child is itself a (deep) prefix of `y`'s corresponding
   child. Emit the outer merge/split first, then the inner one on the
   restart. (Fixes 2, and the mid-line `:::` case where header words
   cross out of a div.)
4. **Similarity before pairing.** In the same-kind branch of `diffGap`,
   require `looksLike` (shared children) or a text prefix relation before
   recursing into a pair; otherwise fall through to delete / insert.
   (Fixes 4.)

Expected effect: the ~850 cases in 1–3 become a split or merge plus the
structural ops already in the vocabulary, with no node of the typed text
deleted and re-inserted.
