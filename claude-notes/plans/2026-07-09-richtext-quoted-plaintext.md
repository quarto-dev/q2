# Rich-text editor: render `Quoted` as editable plaintext quotes, not a chip

**Strand:** bd-iwv3708i
**Date:** 2026-07-09
**Status:** planning (awaiting go-ahead)

## Problem (reproduced in the binary)

In the `q2 preview --allow-edit` / `format: q2-preview` block-level rich-text
(tiptap) editor, a Pandoc `Quoted` inline node is "protected" as an opaque
**chip** — a bordered, monospace, `contenteditable=false` pill showing the
verbatim source. The user can't edit inside the quoted span, and the quotes
look like an alien token rather than prose.

Reproduced with `./target/debug/q2 preview /tmp/q2-quote-repro/index.qmd
--allow-edit --no-browser` on:

```markdown
He said "smart quotes" and it works.
```

Clicking the paragraph to enter the rich-text editor shows `"smart quotes"` as a
chip. Confirmed in the DOM:

```json
{ "cls": "q2-chip q2-chip-span", "text": "\"smart quotes\"",
  "editable": "false", "dataSrc": "\"smart quotes\"" }
```

(The **read-only** preview renders quotes fine as curly `"…"`/`'…'` via
`inlines/Quoted.tsx`; the problem is only the *edit* seed path.)

### Why this is the wrong treatment

Pandoc represents `Quoted` structurally, the same way it represents `Emph`,
`Strong`, `Subscript`, `Superscript` (delimiter + inline content + delimiter).
The editor already renders those four as **WYSIWYG marks** by recursing into
their content. Quotes are different from emphasis in one respect: their visible
form *is* the delimiter characters (`"`/`'`), so they should appear and edit as
**literal quote characters** wrapping still-WYSIWYG content — not as a mark, and
not as an opaque chip.

## Root cause (single site)

`ts-packages/preview-renderer/src/q2-preview/richtext/astToProseMirror.ts`,
`inlines()` switch, lines **69–74**:

```ts
case 'Underline':
case 'SmallCaps':
case 'Quoted':
  // Outside the v1 mark set -> chip the whole construct verbatim.
  out.push(chip(node, 'span', ctx, ''));
  break;
```

`Quoted` is lumped with `Underline`/`SmallCaps` and chipped. The quote type is
never even inspected.

The conversion is **entirely TypeScript**. Rust/pampa only does qmd ⇄ AST; no
Rust change is needed.

## Fix

Split `Quoted` out of the chip case into a recursive text-emitting case that
mirrors `Emph`/`Strong` but wraps the content in literal quote characters:

```ts
case 'Quoted': {
  // Pandoc Quoted is [QuoteType, Inline[]]. Emit literal straight quote
  // characters around still-WYSIWYG content (marks recurse). Straight quotes
  // round-trip: pampa's reader re-parses "…"/'…' back into Quoted, and its qmd
  // writer emits straight quotes.
  const [qt, content] = node.c as [{ t: string }, AstNode[]];
  const ch = qt?.t === 'SingleQuote' ? "'" : '"';
  out.push(S.text(ch, marks));
  out.push(...inlines(content, marks, ctx));
  out.push(S.text(ch, marks));
  break;
}
```

Leave `Underline` and `SmallCaps` chipped (out of scope).

### Why straight quotes (not curly)

The user asked for the "plaintext" form (`"content"` / `'content'`) — straight
quotes. This is also what the whole edit round-trip is built on:

- pampa's qmd **writer** emits straight quotes (`write_quoted`,
  `crates/pampa/src/writers/qmd.rs:1790`).
- pampa's **reader** parses straight `"`/`'` spans into `Inline::Quoted`
  (tree-sitter `pandoc_single_quote` / `pandoc_double_quote`).

Emitting curly quotes would break the serialize→reparse round-trip (a curly `"`
is a `Str`, not a quote delimiter).

### Serializer is already correct

The editor serializes ProseMirror → markdown text → pampa reparse. Straight
quote characters emitted as plain `text` survive verbatim: prosemirror-markdown's
`esc()` (node_modules/prosemirror-markdown/dist/index.js:820) escapes only
`` ` * \ ~ [ ] _ `` — **not** `"` or `'`. No serializer change needed.

## Round-trip verification (already run against native pampa)

`qmd → pampa AST → (proposed astToDoc) → serialize → pampa reparse` is
semantically stable. Confirmed the pampa leg for every shape:

| Input qmd | pampa untransformed AST |
|---|---|
| `"smart"` | `Quoted DoubleQuote [Str "smart"]` |
| `'single quoted'` | `Quoted SingleQuote [Str "single", Space, Str "quoted"]` |
| `"outer 'inner' done"` | `Quoted DoubleQuote [… Quoted SingleQuote [Str "inner"] …]` (nesting) |
| `"**bold** inside"` | `Quoted DoubleQuote [Strong [Str "bold"], …]` (inner mark preserved) |

So: nested quotes recurse to nested literal quotes; marks inside quotes stay
WYSIWYG; all re-parse to the same `Quoted` structure. Apostrophes-in-words
(`it's`) are handled by pampa as curly `Str`, not `Quoted`, and are untouched by
this change.

## Known risk to design around: unbalanced quotes

Agent audit flagged (and the qmd writer documents at
`crates/pampa/src/writers/qmd.rs:1438–1447`): pampa's smart-quote classifier
treats a lone straight `'`/`"` in non-apostrophe position as a quotation mark,
and an **unbalanced** one raises a parse error (`Q-2-7` / `Q-2-10`). So if a user
deletes one quote of a pair mid-edit, the surviving straight quote can make the
committed block fail to parse.

**Assessment:** this is the *intended* consequence of "plaintext quotes" — the
quotes are now ordinary editable characters with ordinary parse consequences,
identical to editing raw qmd source. It is the same class of transient
invalidity as typing a lone `*` or `` ` `` in prose (already possible in the
current editor). We are **not** re-introducing protection.

**Action:** confirm the existing edit-commit path degrades gracefully on an
unparseable block (rejects/keeps the prior good state rather than corrupting the
document) — an investigation item, not new machinery. If it already handles a
lone `*`/`` ` `` sanely, quotes need nothing extra. Only escalate if this
investigation shows the commit path corrupts on parse failure.

### Investigation result (2026-07-09): commit path degrades gracefully — no new machinery needed

The text-channel commit runs `parseQmdContentSync(newText)` **before** writing.
An unbalanced quote makes that return `{ success: false, error }`, and both
front-ends short-circuit before any document mutation:

- **`q2 preview`** — `q2-preview-spa/src/PreviewApp.tsx:585-589`: on
  `!parseResult.success` it `console.error`s and `return`s. No Automerge write,
  no `vfsAddFile`, no `contentTick` bump — the document is untouched and the edit
  is dropped.
- **hub-client** — `hub-client/src/components/render/ReactPreview.tsx:783-789`:
  same guard, and additionally surfaces a **visible** commit error
  (`settleCommitStatus('error')` + `setCommitError("Edit could not be parsed:
  …")`) before returning.

So an unbalanced-quote edit is handled exactly like any other unparseable block
(a lone `*`/`` ` ``): rejected, prior good state preserved, zero corruption. The
plaintext-quote change needs **no** new protection. (Pre-existing minor
asymmetry — `q2 preview` only console-logs where hub-client shows an error chip —
is out of scope here; noted for a possible follow-up.)

## Tests (TDD — write first, watch fail, then implement)

1. **Pure unit** (new, alongside `plainSeed.test.ts`; no pampa oracle, runs in
   default `vitest run`): assert `astToDoc` maps `Quoted` to editable text, not a
   chip.
   - `Quoted DoubleQuote [Str "smart quotes"]` → `unknown` empty, **no** `chip`
     node, `doc.textContent` contains `"smart quotes"` (with straight quotes).
   - `Quoted SingleQuote [...]` → `'...'`.
   - `Quoted DoubleQuote [Strong [Str "bold"], Space, Str "inside"]` → serializes
     to `"**bold** inside"` (inner mark WYSIWYG, straight quotes literal).
   - nested `Quoted DoubleQuote [… Quoted SingleQuote …]` → `"… '…' …"`.
2. **Serializer unit:** a doc containing literal `"x"` / `'y'` text serializes to
   `"x"` / `'y'` unescaped (guards the `esc()` assumption).
3. **Round-trip oracle** (`richtext/roundtrip.test.ts`, gated by
   `QUARTO_RUN_PAMPA_ROUNDTRIP=1`): add fixtures `quoted-double`,
   `quoted-single`, `quoted-nested`, `quoted-with-marks`; assert semantic
   equivalence and empty `unknown`.
4. **Real-binary e2e** (`q2-preview-spa/e2e/`, follow `edit-chrome-placement`
   pattern): open the paragraph with a quote; assert the quoted span is **not** a
   `.q2-chip` and the straight quote characters are present as editable text.
   Optionally: place caret inside the quote, type, and verify write-back
   re-parses to a `Quoted` node.

## Work items

- [x] TDD test 1 — pure unit (`quotedSeed.test.ts`), RED confirmed (7 conversion cases failed on chip)
- [x] TDD test 2 — serializer straight-quote passthrough (guarded in `quotedSeed.test.ts`; passed independently of the conversion change, isolating the `esc()` assumption)
- [x] Implement the `case 'Quoted'` split in `astToProseMirror.ts`; tests 1–2 GREEN (8/8), full richtext suite green (21 pass / 15 skipped)
- [x] TDD test 3 — round-trip oracle fixtures verified under `QUARTO_RUN_PAMPA_ROUNDTRIP=1` (19/19 pass; 4 new quote fixtures semantically round-trip)
- [x] Investigate commit-path behavior on unbalanced/unparseable block — finding recorded above (graceful, no corruption, no new machinery)
- [x] TDD test 4 — real-binary e2e `q2-preview-spa/e2e/richtext-quoted.spec.ts` (2 tests, both green) after rebuild chain (`cargo xtask build-q2-preview-spa` + `cargo build -p quarto --bin q2`; no WASM rebuild — TS-only change)
- [ ] `npm run build:all` (hub-client) green; `cargo xtask verify` as needed
- [x] End-to-end manual verification in Chrome (rebuild chain) — evidence below
- [ ] hub-client/changelog.md entry (if any hub-client artifact changes) — two-commit workflow

## End-to-end verification (2026-07-09, rebuilt `target/debug/q2`)

Invocation: `./target/debug/q2 preview index.qmd --allow-edit` on a fixture with
double, single, nested, and marked quotes. Clicked each paragraph to activate the
rich-text editor and inspected the live DOM (across the preview iframe):

- **`He said "smart quotes" and it works.`** → active `.q2-richtext-editor`,
  **0 `.q2-chip`**, editor text = `He said "smart quotes" and it works.` (literal
  straight quotes, caret placeable inside the quoted words).
- **`A quote with **bold** inside: "very *important* text".`** → **0 chips**;
  editor text `A quote with bold inside: "very important text".`; the inner
  `*important*` renders as a real WYSIWYG `<em>` **inside** the straight quotes,
  and `**bold**` as `<strong>` outside — marks recurse correctly.

Before the change the same click showed `"smart quotes"` as a
`q2-chip q2-chip-span` pill (`contenteditable=false`). Output was inspected
directly (DOM query + zoomed screenshots), not inferred from absence of errors.

## Files

- **Change:** `ts-packages/preview-renderer/src/q2-preview/richtext/astToProseMirror.ts` (lines 69–74)
- **Tests:** `.../richtext/plainSeed.test.ts` (model), `.../richtext/roundtrip.test.ts`, `q2-preview-spa/e2e/`
- **Reference (no change):** `.../richtext/serializer.ts`, `.../richtext/schema.ts`,
  `crates/pampa/src/writers/qmd.rs` (quote writer + smart-quote notes),
  `crates/pampa/src/pandoc/treesitter_utils/quote_helpers.rs` (quote reader)

## Notes

- Superseded spike `.../tiptap-roundtrip-spike/astToPm.ts` also chips `Quoted`
  (line 61); it is not the production path and is out of scope.
- Rebuilding for the live check needs the full chain (WASM → SPA bundle → q2
  binary) per `CLAUDE.md` "Verifying Rust changes in `q2 preview`" — though this
  change is TS-only, the SPA bundle must be rebuilt (`npm run build:all` +
  `cargo xtask build-q2-preview-spa` + `cargo build --bin q2`) for `q2 preview`
  to pick it up.
