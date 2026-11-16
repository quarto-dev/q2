# Shift+Enter in the comment input: span-friendly hard line breaks

**Strand:** bd-ov4jydyf
**Status:** plan — decisions reviewed 2026-11-16; awaiting go-ahead (not started)
**Related:** bd-ajeb4y6f (writer hardening, parallel)

## Goal

In the q2-preview comment bubble, Enter submits a comment. Add
**Shift+Enter** to start a new line inside the comment. A new line
must be stored as a Pandoc `LineBreak` inline. The qmd writer prints a
`LineBreak` as a backslash at the end of the line (`\` + newline). That
form keeps the line break inside the `[>> …]` span:

```markdown
Some paragraph.[>> first line\
second line]
```

The text must never reach the AST as a raw `\n` inside a `Str`, or as a
blank line. Either one changes the document's structure when the qmd is
read back (details under "Problems today").

This matches what the docx import already produces. A Word comment with
several paragraphs is imported as one set of inlines with `LineBreak`
between the paragraphs (`blocks_to_inlines`,
`crates/quarto-core/src/editorial_marks.rs:211`). The OOXML and Typst
exports already accept that shape: `inlines_to_plain_text` turns a
`LineBreak` into `\n`.

## Where the feature lives

The comment UI lives in the preview renderer package, not in
`hub-client/` itself. `hub-client` and `q2 preview` both use it.

- `ts-packages/preview-renderer/src/q2-preview/custom/CommentBlock.tsx`
  - `CommentWrapper`: the bubble chrome, used by both block comments and
    span comments. Its `<textarea className="q2-comment-input">` has the
    key handler at about line 1608: `Enter` → `preventDefault` +
    `addComment(commentText)`, and `Escape` closes the input. The hint
    under the input reads `Enter to add · Esc to close` (about line
    1654). The textarea already grows with its content.
  - `CommentBlockInner.addComment` (about line 430) builds the comment as
    `{ t: 'Span', c: [['', ['quarto-edit-comment'], []], [{ t: 'Str', c: text }]] }`.
    **All of the raw text goes into one `Str`.** The function then
    appends the span to the block's inline slot (`Para`, `Plain` or
    `Header`). A `CodeBlock` gets wrapped in a comment-container `Div`
    instead.
  - `CommentSpanContent` renders a comment's inlines in the bubble
    through the normal inline registry (bd-y66gbfs4). A `LineBreak` is
    already rendered as `<br />` (`inlines/LineBreak.tsx`).
- `ts-packages/preview-renderer/src/q2-preview/custom/CommentSpan.tsx`:
  `addComment` (about line 173) repeats the same single-`Str`
  construction for comments attached to a `[span]`.
- Commits go through `PreviewContext.commitSubtreeEdit` and then to the
  WASM `apply_node_edit`. That call runs the incremental qmd writer
  (`crates/pampa/src/writers/qmd.rs`: `write_linebreak` at about line
  2293, and the editorial-mark span branch of `write_span` at about
  line 2460).

## What the writer does with each shape (measured)

I measured this with `target/debug/pampa`: Pandoc JSON → qmd → native,
compared with the input. The scratch scripts are in the session
scratchpad. Each case was tried in a paragraph, a tight list item and a
blockquote, with the same result in all three.

| comment inlines | qmd written | read back |
|---|---|---|
| `Str a, LineBreak, Str b` | `[>> a\`⏎`b]` (with `  ` / `> ` prefixes in a list item / blockquote) | **identical** |
| `a, LB, LB, b` (empty line inside the comment) | `a\`⏎`\`⏎`b` | **identical** |
| `LB, a` (break at the start) | `[>> \`⏎`a]` | identical |
| `a, LB, Str "# x"` (also `- x`, `1. x`, `> x`, `::: x`, `\| x`, `+ x`, `]`, `` ``` ``, `***`) | the writer escapes the start of the line (`\#`, `\-`, `1\.`, …) | same meaning; only the Str/Space split differs |
| `a, LB, Str "   x"` (leading spaces) | `a\`⏎`   x` | the leading spaces are **dropped** |
| `a, LB` (**break at the end**) | `[>> a\`⏎`]` | **parse error** |
| `LineBreak` inside a **Header** comment | `# H [>> a\`⏎`b]` | **parse error**. An ATX heading is one line. |
| `Str "hello world"` (the current client shape) | `[>> hello world]` | `Str hello, Space, Str world`: same meaning, different shape |
| `Str "a\nb"` (**a paste today**) | `[>> a`⏎`b]` | `SoftBreak`: the line break disappears when rendered |
| `Str "a\n\nb"` (**a paste today**) | `[>> a`⏎⏎`b]` | **Q-2-21 Unclosed Editorial Comment**: the document is corrupted |

Conclusions:

1. A `LineBreak` between two pieces of text is safe in every context
   where a comment can be added (paragraph, `Plain` in a list item,
   blockquote, comment-container `Div`, `[span]` comment). The writer
   already handles indentation, blockquote prefixes and escaping at the
   start of a line.
2. The client must **never** produce a `LineBreak` at the end of a
   comment, and must **never** produce one inside a `Header`.
3. **Problems today:** the textarea already accepts pasted text with
   several lines. Pasting a blank line creates a document that no longer
   parses. The text → inlines conversion below fixes this along with the
   new feature.

## Design

### 1. One pure text → inlines function (new: `q2-preview/custom/commentText.ts`)

```ts
export function commentTextToInlines(
  text: string,
  opts: { allowLineBreaks: boolean },
): InlineNode[]
```

- Convert `\r\n` and `\r` to `\n`.
- Trim the whole text. This removes empty lines at the start and end,
  so no `LineBreak` can come first or last.
- Split the text into lines. Remove leading whitespace from each line
  after the first (the reader drops it anyway) and trailing whitespace
  from every line.
- Split each line into words: whitespace runs become `Space`, and the
  words between them become `Str`. This is the shape the reader
  produces. The client-built block only travels inside the edit payload
  (`commitSubtreeEdit` → `apply_node_edit` → qmd → re-parse), and
  nothing displays or stores it, so the change has no visible effect.
  What it does is let the WASM test (§Tests C) require that the payload
  and the AST read back are exactly equal (see D6).
- Put lines together with `LineBreak` when `allowLineBreaks` is true,
  and with `Space` when it is false. An empty line in the middle of the
  text becomes two `LineBreak`s, which round-trips exactly (see the
  table).
- Return `[]` for text that is only whitespace. The caller already
  ignores empty input.

Both `addComment` implementations call this function instead of
building `[{ t: 'Str', c: text }]`. It is exported so that the WASM
test (§Tests C) builds its commit with exactly the code the UI uses.

### 2. Where line breaks are allowed: `allowLineBreaks`

- New `CommentWrapper` prop: `allowLineBreaks: boolean`, default `true`.
- `CommentBlock` passes `block.t !== 'Header'`.
- `CommentSpan` passes `enclosingBlock?.t !== 'Header'`, read from
  `EnclosingBlockContext`, which is already in scope.
- The wrapper's `addComment(text)` stays a string API. Each caller
  converts the text, so the wrapper does not need to know about AST
  shapes.

### 3. Keyboard handling (the textarea `onKeyDown`)

```text
Enter (no Shift), not composing  → preventDefault; submit (as today)
Shift+Enter, allowLineBreaks     → do nothing; the browser inserts "\n"
Shift+Enter, !allowLineBreaks    → preventDefault (a heading holds one line)
Enter while IME composing        → do nothing (it confirms the composition)
Escape                           → as today
```

The IME guard (`e.nativeEvent.isComposing || e.keyCode === 229`)
fixes an existing bug. Today, a user typing CJK text who presses Enter
to confirm a candidate submits the half-written comment. It costs one
line, and I recommend including it (decision D2).

The textarea already resizes itself through the `commentText` effect,
so a new line makes it taller without further changes.

Hint text (D3): `↵ add · ⇧↵ new line · Esc close` where line breaks
are allowed, and `↵ add · Esc close` inside a heading.

### 4. How a comment with line breaks is displayed

- **Expanded rows** render a `<br>` through `CommentSpanContent`. No
  change is needed. A test locks this in.
- The **compact bubble** is one line (`nowrap`, ellipsis, 140px). A
  `<br>` there would make the bubble taller and make the "first line
  preview" look broken. Add a `compact` prop to `CommentSpanContent`.
  When it is set, the bubble's registry override renders `LineBreak`
  as a single space. That is the same override that already replaces
  `Span` and `Note`.

### 5. Hardening the writer (Rust): separate strand bd-ajeb4y6f

The writer produces qmd it cannot read back in two cases: a span that
ends in a `LineBreak`, and a `LineBreak` inside a heading. Neither the
reader nor the new client code produces these shapes. Imports and
filters can, though. Pandoc's own Markdown writer prints a `LineBreak`
in a heading as a space. That fix is filed as bd-ajeb4y6f (D1). This
plan only adds Rust tests that **lock in** the safe cases this feature
depends on.

## Tests (written first; each verified failing before the fix where applicable)

### A. Unit tests: `custom/commentText.test.ts` (vitest)

A table of cases for `commentTextToInlines`:

- `"hello world"` → `Str, Space, Str`; runs of spaces collapse to one
  `Space`
- `"a\nb"` → `Str a, LineBreak, Str b`; `\r\n` and `\r` give the same
  result
- `"a\n\nb"` → `a, LB, LB, b`
- `"\n\na\n\n"` → `[Str a]` (no `LineBreak` at either end)
- `"a\n   b"` → `a, LB, b` (leading whitespace removed)
- `"a  \nb"` → `a, LB, b` (trailing spaces do not become `Space`)
- `"  \n \n"` → `[]`
- `allowLineBreaks: false`: `"a\n\nb"` → `a, Space, b`
- Property check: the result never starts or ends with `LineBreak` or
  `Space`, and no `Str` contains whitespace

### B. Component integration: `custom/CommentBlock.lineBreak.integration.test.tsx` (jsdom)

This uses the `mount()` + `vi.fn()` `commitSubtreeEdit` setup from
`CommentBlock.resolveLast.integration.test.tsx`.

1. Shift+Enter in the input does **not** commit, and the event's
   default is **not** prevented.
2. Type `line one`, then Shift+Enter (simulated by setting the value
   to `line one\nline two`), then Enter. The input commits once, and the
   committed block's comment span holds exactly
   `[Str line, Space, Str one, LineBreak, Str line, Space, Str two]`.
3. A plain Enter still commits a comment of one line, with no
   `LineBreak`. This checks that nothing else changed.
4. **Header block:** Shift+Enter has its default prevented. A pasted
   `a\nb` commits as `a, Space, b`. The hint has no "Shift+Enter" text.
5. **Code block:** the comment-container `Div` that is created holds a
   `Para` whose comment span contains `LineBreak`.
6. **Appending to an existing comment container:** the new comment goes
   into the last comment paragraph, with its `LineBreak`s kept.
7. An Enter keydown with `isComposing: true` does not commit (D2).
8. Display: an expanded bubble for a comment `a, LB, b` contains one
   `<br>`. In the compact bubble the same comment has no `<br>`, and its
   `textContent` is `a b`.
9. `CommentSpan` path (`CommentSpan.lineBreak.integration.test.tsx`, or
   cases added to the existing CommentSpan test): a span comment inside
   a `Para` commits a `LineBreak`. A span comment inside a `Header`
   commits a `Space`.

### C. WASM end-to-end round trip: `hub-client/src/services/commentLineBreak.wasm.test.ts`

This is modeled on `applyNodeEdit.wasm.test.ts`. It runs the real
`render_page_in_project_with_attribution` → `apply_node_edit` path,
which uses the same incremental writer and inline splicing as the live
app. For each fixture:

1. Render the document and take `untransformed_ast_json`.
2. Find the target block and add the comment the way the UI does, by
   calling `commentTextToInlines(...)` from `@quarto/preview-renderer`.
   Ideally the test also calls a shared "append comment" helper (see
   the implementation note below).
3. Call `apply_node_edit` and assert:
   - The new qmd contains the expected text, prefixes included.
   - Every byte outside the edited block is unchanged.
   - Reading the new qmd back succeeds with no errors.
   - **Exact equality:** the re-read comment span's inlines deep-equal
     the inlines the helper produced, ignoring source info. This means
     the payload is already in the reader's normal form. If anyone
     changes the helper later, a round-trip difference fails the test
     instead of slipping through.

Fixtures:

| context | input | expected text |
|---|---|---|
| paragraph | `Para text.` | `Para text.[>> one\`⏎`two]` |
| paragraph that already has a comment | `Text.[>> old]` | the old comment is kept, the new one is appended |
| tight list item (`Plain`) | `- item` | continuation indented by `  ` |
| loose list item | `- a`⏎⏎`- b` | indented continuation; the list stays loose |
| blockquote | `> quote` | continuation prefixed with `> ` |
| blockquote inside a list | `- > q` | both prefixes |
| code block | a fenced block | a container `Div`; the comment paragraph holds the break |
| `[span]` comment | `Some [text] here.` | `[text[>> one\`⏎`two]]` |
| heading | `# Title` | text of one line, `[>> one two]` |
| blank line in the middle | text `one\n\ntwo` | `one\`⏎`\`⏎`two` |
| awkward line starts | second line is `# x`, `- x`, `1. x`, `> x`, `::: x`, `]`, `[>> y`, `` ``` `` | the line start is escaped; re-reading gives the same meaning (a `Str` comparison that allows splits) |

These tests run under `npm run test:wasm` in hub-client and need a
current WASM build (`npm run build:wasm`, see memory note on stale hub
WASM).

### D. Rust: lock in the writer behavior we depend on (`crates/pampa/tests/integration/qmd_writer_pandoc_shapes.rs`)

- `line_break_inside_editorial_comment`: `assert_roundtrip` on a comment
  span holding `a, LB, b` inside a paragraph, a `Plain` in a bullet
  list, a blockquote, a blockquote inside a list, and a span-in-span
  comment, plus a double `LB`.
- `after_line_break_in_editorial_comment`: `check_all` over
  `LINE_START_TEXTS`, with each text placed after a `LineBreak` inside
  a `[>> …]` span. This mirrors the existing `after_line_break` test,
  but inside the editorial span.

These should pass today (the measurements say so). They guard against a
future writer change breaking this feature without anyone noticing.
The two shapes that fail (a trailing `LB`, and an `LB` in a heading)
belong to bd-ajeb4y6f.

### E. Manual / browser check before the PR

Run `q2 preview` on a scratch document. Add a comment with
Shift+Enter in a paragraph, a list item, a blockquote, a code block and
a heading. Check the source in the editor pane, the expanded and
compact bubbles, and that resolve (✓) still works on a comment with
several lines.

## Implementation steps

1. Write tests A, B and D (D should already pass, so just confirm it
   does). Confirm that A and B fail.
2. Add `commentText.ts` and export it from the package entry point the
   hub-client test imports.
3. Optionally move the "append a comment span to a source node" logic
   out of `CommentBlockInner.addComment` into a pure function
   (`appendCommentToBlock(sourceNode, inlines) → BlockNode`). Then
   test C uses the real logic for code-block wrapping and containers,
   instead of copying it. This is a small, mechanical refactor
   (decision D4).
4. Change both `addComment`s to use the function, add
   `allowLineBreaks`, and change the key handler, the hint, and the
   compact-mode `LineBreak` override.
5. Write test C, run `npm run build:wasm`, then `npm run test:wasm`.
6. Add a changelog entry (hub-client / preview).
   `cargo xtask verify --skip-hub-build`, then the hub test suites.
7. Do the manual check (E). Then open the PR from
   `braid/bd-ov4jydyf-<slug>`.

## Decisions (reviewed 2026-11-16)

- **D1. Writer hardening: separate strand, filed as bd-ajeb4y6f**
  (`discovered-from` this one) so a parallel agent can take it. The
  client-side guard here is enough on its own. The two strands don't
  block each other.
- **D2. IME guard: accepted.** With an IME (Chinese, Japanese, Korean
  input), keys first build up a *composition*: the user types, for
  example, phonetic text, chooses a candidate, and presses **Enter to
  confirm** it. That Enter keydown arrives with `isComposing === true`.
  (Safari reports it as `keyCode 229` and may not set `isComposing`, so
  the guard checks both.) The guard only ignores that confirming Enter.
  Once the composition is finished, the **next** Enter is an ordinary
  keydown and submits the comment as it does now. Without the guard,
  the confirming Enter submits half-written text and the user's last
  word never makes it into the comment. Test B7 covers it.
- **D3. Hint text: try the short form** `↵ add · ⇧↵ new line · Esc close`.
  Inside a heading it stays `↵ add · Esc close`.
- **D4. `appendCommentToBlock` refactor: accepted.**
- **D5. Shift+Enter in a heading is ignored: accepted.** Why headings
  differ: a comment is stored *inside its block's inline content*. A
  paragraph's inline content can run over several source lines, so
  `\`+newline stays inside the paragraph. An ATX heading
  (`# Title [>> …]`) is one source line by definition, so any newline
  ends the heading, and the rest of the comment becomes a broken
  paragraph (the parse error measured above). Setext headings can't
  span lines either, and the reader doesn't accept them anyway. There
  is one way to give headings multi-line comments: store their comments
  the way code blocks do, wrapping the heading in a
  `::: {.quarto-edit-comment-container}` div. That changes the
  document's structure around a heading. Section divs, slide splitting
  (a wrapped heading no longer starts a reveal.js slide), and possibly
  TOC/crossref section numbering all depend on headings being top
  level. Commenting should not risk any of that. So headings keep
  comments of one line, and that limit is a property of the syntax,
  not a UI choice. A follow-up could show a short note when Shift+Enter
  is pressed in a heading; this plan doesn't include one.
- **D6. Splitting words into `Str` / `Space`: accepted, and a smaller
  change than first described.** Your reading is right. Today the
  client sends `Str "hello world"`, and that never survives: the block
  goes into the edit payload, `apply_node_edit` writes it as
  `[>> hello world]`, and the preview re-renders from re-parsing that
  qmd, which gives `Str hello, Space, Str world`. Nothing displays the
  client-built AST before the commit (`PreviewRoot.commitSubtreeEdit`
  only builds the payload; `CommentWrapper` waits for the re-rendered
  comment count before closing the input). So the current comments that
  get stored are already in the reader's normal form, and the change
  only affects the short-lived payload. It is worth doing for two
  reasons. It lets test C require exact equality. And it keeps
  `apply_node_edit` from comparing a nonstandard subtree with the source
  (if inline splicing ever compares the new comment's inlines to
  anything, normal-form inlines are the safe choice). Risk: low. The
  existing comment tests check text and rendering, not the payload's
  `Str` shape. I will grep for assertions on `{ t: 'Str', c: <text with
  spaces> }` in the comment tests and update any that rely on the old
  shape.
