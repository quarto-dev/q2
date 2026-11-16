# rich-editor (prototype)

A prototype editor for Quarto 2 documents. It uses the same backend as
hub-client: `@quarto/quarto-sync-client` for automerge sync and the
`wasm-quarto-hub-client` module for parsing qmd and writing it back. All of
the UI lives in one component, `src/Editor.tsx`.

The window has two panes: on the left, the file's source in a plain textarea
(for debugging); on the right, the visual editor, which renders the title
block and, under it, the body. The title block is hub-client's
`PreviewTitleBlock` with the default theme CSS, fed by
`deriveTitleBlockMeta`, a WASM call that runs the render pipeline's author
normalization on the document's metadata in well under a millisecond instead
of waiting for a full render. The body is rendered block by block from the
parsed source.

A document has only the title-block fields its author adds. A row of "+"
items above the title block ("+ Title  + Subtitle  + Abstract", one for each
field the document doesn't have, in `src/TitleBlockWidget.tsx`) adds them:
it shows while the caret is in the title block, the pointer is over it or
the document is empty, and it is laid over the space above the title block,
so showing it moves nothing. The keyboard reaches it too: Left at the start
of the first field or Up on its first line goes to it. An added field stays,
empty, with its placeholder, until Backspace in it removes it, and the
source holds it all along as an empty value (`subtitle: ""`). Enter never
adds a field.

The title, subtitle and abstract are editable in place, and so are the body's
paragraphs and headings of text: Enter splits a paragraph, and
Backspace at its start joins it to the one before. Typing one to six `#` and
a space at the start of a paragraph makes it a heading (Backspace or undo
right after gives back the `#`s as text), and Backspace at the start of a
heading's text makes it a paragraph again. Block quotes of one paragraph
of text are edited the same way, made by typing `>` and a space.
So are bullet lists whose items are each one line of text (tight
lists), with a line per item: typing `-`, `*` or `+` and a space makes an
item, Enter at the end of an item starts another, Enter on an empty item
ends the list, and the items of lines next to each other are one list.
Numbered lists of such items, written `1.` or `1)`, are edited the same
way, made by typing `1.` and a space; an edit keeps a list's start
number and delimiter, and a new list is numbered `1.` from 1. A bullet
item and a numbered one next to each other are in two lists.
Typing `---` (or `***` or `___`) at the start of a paragraph puts a
horizontal rule before it; Backspace at the start of the line after a
rule, Delete at the end of the line before it, or deleting a selection
across it, removes it.

The body's text can be formatted: bold, italic, underline,
strikethrough, highlight (`[text]{.mark}`), code and links, in any
combination, shown as `q2 render` shows them. Its design is
`mockups/inlines.html`, with "Arrow out of code", "Converts as you type"
and "Popover" chosen. Typing at the end of bold or italic carries it on,
and at its start doesn't. Code has an inside and an outside at each
edge: Right at the end of code steps out of it without moving on, and
Left at its start too, so text can be typed just outside it. A caret put
somewhere by a click or the browser takes the formats of the character
before it. Cmd+B, I, U, Shift+X, E and Shift+H (Ctrl elsewhere) turn
bold, italic, underline, strikethrough, code and highlight on or off over
the selection, or, with nothing selected, for what's typed next; Enter
carries the formats (but code) on to the line it starts. Markdown typed
out and closed (`**bold**`, `*italic*`, `__bold__`, `_italic_`,
`~~strike~~`, `` `code` ``, `[text](url)`) becomes formatting, and
Backspace right after gives the characters back; an underscore inside a
word stays one. The caret in a link, or the mouse resting on one, shows a
popover under it with the link's address, Edit and Remove (`src/LinkPopover.tsx`);
Tab moves into it and Escape puts it away. Cmd+K opens a small form to
make or edit a link. Copy takes the plain text.

A toolbar above the document (`src/Toolbar.tsx`) follows the caret, as
in a word processor: its style box shows the style of the caret's line
(Paragraph, Heading 1 to 4) and its menu changes it, its bulleted and
numbered list buttons are pressed while the caret is on an item of their
kind and make the line one (from an item of the other kind too) or a
paragraph again, and its quote button is pressed while the
caret is in a quote and quotes or unquotes the line. Its bold, italic,
underline and highlight buttons, after the style box, are pressed for
the formats of what's selected (or of what typing at the caret gives)
and turn them on or off; its link button, after them, is pressed in a
link and opens the link form.
With a selection, they change every line it is on, as one undo step, and
the caret stays on its text. A heading can't be in a quote yet, so neither
can be made into the other from the toolbar, and nor can a list item be
made a heading or a quote, or a heading or a quote an item; outside the body, it's disabled. Its undo and
redo buttons, first in it, work wherever the caret is, as the keyboard's
undo and redo do, and are disabled while there's nothing to undo or redo. Its design is the toolbar of `mockups/block-annotations.html`
with nothing in the margin and no breadcrumb.
Resting the mouse on one of its buttons, enabled or not, shows a tooltip
with the button's name and keyboard shortcut (`src/Tooltip.tsx`), and
its style menu gives each style's shortcut; their design is the "Chosen"
section of `mockups/shortcut-tooltips.html`. The line styles have
shortcuts as in Google Docs (`src/shortcuts.ts`), which do what the
toolbar does, and nothing where it's disabled: Cmd+Option+0 Paragraph,
Cmd+Option+1 to 4 the headings, Cmd+Shift+8 the bulleted list,
Cmd+Shift+7 the numbered list and Cmd+Shift+9 the quote (Ctrl and Alt
elsewhere).
The rest of the body (paragraphs, headings, quotes and lists with math,
a citation, a note, an image or a span of another kind in their text,
quotes of more than one paragraph, lists with blank lines
between items or lists in items, example lists (`(@)`), code) is shown by
`src/ReadOnlyBlock.tsx` and can't be edited yet; no edit removes it or
changes its source. The document is one
`contenteditable` surface run by a small editing engine in `src/engine/`:
the browser moves the caret and selection (so selections can span fields),
and the engine intercepts every edit, applies it to a model of text
fields, and renders it back. Its rules (Enter moves to the next field,
fields are never joined or split, an empty field disappears when left
unless it's one that stays when empty) and
its single undo history are pure TypeScript with unit tests. Like
`src/quarto-synced-file/`, it imports nothing from the rest of the demo;
the document's fields are configured in `src/VisualEditor.tsx`.

The body is one field, like the abstract: its text has a line per block, so
splitting and joining paragraphs, undo, and following a collaborator's edit
are the same text operations as everywhere else. A block that can't be
edited is a line holding one placeholder character (`ATOM`), which the
engine never removes, joins to another line or lets text onto, and which
is rendered as the block it stands for (`src/editing/body.ts`). A
horizontal rule is a line holding another (`VOID`), which the engine keeps
on a line of its own too, but removes as an edit says. Each line that can
be edited is an element of its own (`Lines`).

A line can start with a prefix, a run of characters the engine keeps out
of the text (the Unicode noncharacters U+FDD0 to U+FDEF), which gives the
line a kind: in the body, a heading's level, a quote, or a bullet or
numbered list's item. A change of kind is then a
text edit like any other, so undo, collaborators' edits and the caret all
handle it without knowing about headings. The engine's rules for prefixes
are generic (one can't be typed; the caret never goes before one; a joined
line takes the kind of the line it joins; Enter in the middle of a line
keeps the kind on both halves, and at the end starts a line without one,
unless it's a kind Enter carries on (`Schema.continued`, an item's),
which it takes off an empty line instead);
what a prefix means, how it's rendered, what typing makes one
(`Schema.inputRule`) and how it's copied (`Schema.prefixText`) are up to
`src/VisualEditor.tsx` and `src/editing/body.ts`; a line can be rendered
inside an element of its own, as a quote's paragraph is in a
`blockquote`, or share one with the lines next to it, as a list's items
are in one `ul` (or `ol`, with the list's start number). An edited heading keeps
its written attributes (`{#id .class}`), and only its text is rewritten in
the source.

Formatting is text too (`src/engine/marks.ts`): a format is on from an
open mark to a close mark, characters from the Unicode private use area
that the engine keeps out of sight, and a link's address is part of its
opening mark. So making text bold, undoing it and following a
collaborator who bolds a word are text edits like any other. A line's
marks are kept canonical, and where two characters' formats differ, the
caret can be at each offset between the marks there, each giving typed
text formats of its own: that's how code has a caret position inside and
one outside at each edge, and how a format turned on at the caret is
held (in an empty run, between marks that open and close at once) until
something is typed or the caret leaves. Deleting text keeps its marks, so
what follows keeps its formats. The page shows the formats as the
elements `q2 render` writes, and where the caret is at a place no text
stands for (outside code at the end of a line), the line holds a
zero-width space for the caret to be drawn in (`renderText` in
`src/engine/dom.ts`). `src/editing/inlines.ts` reads a block's inlines
into a line with marks, and writes one back: spaces at the ends of
bold (and the like) go outside it, as markdown needs.

The qmd writer writes italic inside bold with underscores (`**a_b_**`),
which doesn't read back when the italic is inside a word. An edit whose
document wouldn't parse once written isn't written (`fieldSource.ts`), so
such formatting is shown but never reaches the source.

Every edit is written to the file's source text as it's made, so
collaborators see it at once, whether they use this editor or a source
editor. The engine reaches the document only through `FieldSource`
(`src/engine/source.ts`): read the fields, write a change and read back
what the document made of it, and hear when the document changes. A change
from someone else is made to the editor's own state too (`rebase.ts`), so
the caret and selection stay on the text they were on, typed text stays,
and undo undoes only this editor's edits. `src/editing/fieldSource.ts`
implements `FieldSource` for a qmd file's metadata: it writes the edited
document with the incremental qmd writer, which rewrites only the changed
front-matter keys and body paragraphs, and sends the difference as one
splice. While the source doesn't parse, the editor shows the last text it
could read and takes no edits. While the connection to the sync server
is down, editing goes on: edits go into the local automerge document,
and are synced, and merged with what others did meanwhile, once the
connection is back. They're kept only in memory, so reloading the page
before then loses them.

An empty line in the body is an empty paragraph, so Enter on an empty
line adds another and they stay when the caret leaves. qmd has no syntax
for one (blank lines between paragraphs don't add up), so it's written as
a paragraph of one non-breaking space, which `q2 render` shows as an empty
line too; `&nbsp;` on a line of its own in the source reads the same. An
empty quote is a quote of that paragraph (`> ` and a non-breaking
space), since `>` alone is a quote with no paragraph in it. An empty
list item is an item with nothing in it, a bare `*` (or a bare number,
`2.`, in a numbered list).

The qmd writer writes a list again as a whole when any item of it
changes, with `*` before every item, so editing an item of a list written
with `-` changes every item's line in the source (a fixme in
`lists.spec.ts`). A numbered list is written renumbered from its start
number, with two spaces after a number below 10 (`1.  One.`), so editing
an item of a list written `1. One.`, or `1.` before every item, changes
every line of it too (a fixme in `numbered-lists.spec.ts`).

A paragraph that the source wraps over several lines is one line here. An
edit keeps its line breaks wherever the text around them stays, so typing
in a wrapped paragraph changes only what was typed.

Not done yet in the body: a paragraph can't be started between two blocks
that can't be edited, or after one at the end of the document.

The non-UI part lives in `src/quarto-synced-file/`: one qmd file kept in sync
with a hub, its parsed AST, and splice-based edits. It has no UI dependencies
besides an optional React hook, so it could become its own package.

## Running

1. Build the WASM module (once, and after Rust changes):

   ```bash
   cd hub-client && npm run build:wasm
   ```

2. Start a hub (for example, `npm run local-prod` in `hub-client/`) and
   open a project there to get its index document id.

3. Start the dev server:

   ```bash
   cd q2-demos/rich-editor && npm run dev
   ```

4. Open the printed URL with the document to edit:

   ```
   http://localhost:5173/?doc=<index-doc-id>&file=index.qmd
   ```

   `server` defaults to `ws://127.0.0.1:8080/ws`; override it with
   `&server=<ws-url>`.

## Trying it together

The build (`dist/`) is a static site, and a document doesn't need a hub,
only an automerge sync server, so two people can edit one document in the
editor with nothing running on their machines:

1. Put a document on the public automerge sync server:

   ```bash
   npm run share-doc -- [file.qmd]   # default: e2e/fixtures/basic.qmd
   ```

   It prints the query string that opens it (`?server=…&doc=…&file=…`).
   Anyone with the document id can read and edit the document, so share
   only demo content. `SYNC_SERVER=<ws-url>` puts it on another server.

2. Publish the build to Posit Connect, once per change to the editor:

   ```bash
   npm run deploy:connect
   ```

   This builds the WASM module and the app, and updates the demo's content
   on connect.posit.it
   (`https://connect.posit.it/content/a52850b5-b6b5-4655-86d9-98f55ef4590f/`).
   Connect's address and API key come from rsconnect-python's config,
   saved once per machine, outside the repo:

   ```bash
   uvx --from rsconnect-python rsconnect add --name connect \
     --server https://connect.posit.it --api-key <key>
   ```

   Never put the key in the repo or in a file in it. Who can open the
   content is set in Connect (its Access settings).

3. Open the content's URL with the query string from step 1.

## Tests

```bash
npm run test:unit   # editing logic, in Node
npm run test:wasm   # needs the WASM build from step 1
npm run test:e2e    # the app in Chromium, Firefox and WebKit, on a test hub; needs the WASM build
npm run test:e2e:chromium   # the same in Chromium only, for iterating
```

While iterating, run Chromium only, on the specs you're changing
(`npm run test:e2e:chromium -- e2e/body.spec.ts`). Run every browser on
those specs before committing a finished piece of work, and the whole suite
in every browser before merging.

The e2e tests are the most valuable part of this prototype. They are the
specification of the editing experience, written against what a person
sees and what the source text holds, not against this implementation, and
they are meant to be complete: someone with only the specs should be able
to build this editor again. Behaviour that no spec describes is a gap in
the specs; add the spec with (or before) the behaviour.

`npm run test:e2e` runs one suite at a time across all of the repo's
worktrees (`scripts/e2e-queue.mjs`): a run started while another is going
waits for it to finish. Playwright sizes its workers for a machine to
itself, so runs in parallel each run much slower and slow tests time out.
`npx playwright test` skips the queue.

The tests start a dev server on port 5192, and use one that's already
there. To run them outside the queue in two checkouts at once, give one a
port of its own (`E2E_PORT=5200 npx playwright test`), or each will be
testing the other's app.

Every spec runs in Chromium, Firefox and WebKit (`npx playwright install`
fetches them; `--project=firefox` runs one). The engines differ most
where an editor lives: the caret, selections and `contenteditable`. A
spec that can't hold in one engine says so where it's skipped, with the
reason.

The specs are: `title-block.spec.ts`, `cross-field.spec.ts`,
`body.spec.ts`, `headings.spec.ts`, `quotes.spec.ts`, `lists.spec.ts`,
`numbered-lists.spec.ts`, `rules.spec.ts`, `inlines.spec.ts` (formatting)
and `links.spec.ts` (links, their popover and form)
(editing),
`title-block-widget.spec.ts` (adding and removing title-block fields),
`toolbar.spec.ts` (the toolbar's undo and redo buttons, style box and quote button, its tooltips, and the line styles' shortcuts), `keys.spec.ts` (moving and
deleting by word and by line), `clipboard.spec.ts` (copy, cut, paste, drag
and drop),
`composition.spec.ts` (input methods), `blocks.spec.ts` (how each kind of
block is shown), `read-only.spec.ts` (when nothing can be edited),
`source.spec.ts` (what an edit does to the source text),
`collaboration.spec.ts` (more than one person in a document),
`render-parity.spec.ts` (the editor shows a document as `q2 render` would)
and `app.spec.ts` (the page around the editor: which document it opens,
the source pane, the connection to the hub, the status line, the layout
in a narrow window).
Tests marked `fixme` describe behaviour we want and don't have yet.

`render-parity.spec.ts` compares the editor's page with the same fixture
rendered by the WASM module's full render pipeline, without the editor's
own markup. It compares HTML, not styles, so it runs in Chromium only, and
it opens fixtures in `/fixture.html` instead of on a hub.

Every other test puts a document from `e2e/fixtures/` on an in-process sync hub
and opens it in the app, once per collaborator, each in a browser of its
own; the test itself is one more collaborator, reading and editing the
source text. Playwright starts the dev server itself.

`/fixture.html?fixture=<name>` (default `basic`) opens the visual editor
on a fixture held in memory, with no hub, for trying it without a server.

### What the specs take as given

The specs describe the editor, not the parts of Quarto 2 it is built on.
Someone building the editor again from the specs has these as they are:

- **The `wasm-quarto-hub-client` module** (`src/quarto-synced-file/wasm.ts`
  wraps it): it parses qmd into a Pandoc JSON AST, writes an AST back as
  qmd, either whole or incrementally (rewriting only the changed
  front-matter keys and blocks, so the rest of the source stays as it
  was), renders a document with the full pipeline (for
  `render-parity.spec.ts`), works out the title block's metadata
  (`deriveTitleBlockMeta`, which normalizes the authors), and compiles the
  default theme's CSS.
- **`@quarto/quarto-sync-client`**: the automerge sync client, and the
  in-process test hub the specs put documents on.
- **`@quarto/preview-renderer`'s `PreviewTitleBlock`**: the title block's
  markup, as `q2 render` emits it.

The specs find their way around the page by this markup, which a new
editor has to keep:

- The page: `?doc=<index document id>&file=<path>`, with
  `&server=<ws-url>` (see `app.spec.ts`), and
  `/fixture.html?fixture=<name>`. The source pane is a `textarea`, shown
  by the toolbar's "Developer tools" button. The
  page's own status is `.status` (`.status-error` for an error), and the
  line under the editor is `.visual-editor .status`.
- The document is in `iframe.visual-frame`, in the page structure `q2
  render` emits. `#quarto-document-content` is the one `contenteditable`
  element: it holds the title block (`#title-block-header`, with
  `h1.title`, `p.subtitle` and `.abstract`) and then the body's blocks as
  its children.
- Text that can be edited is in `.qe-field` elements: one in each
  title-block field, and one for each editable line of the body (a direct
  child of `#quarto-document-content`, or, for a quote, the `p` in a
  `blockquote` that is one, or, for a list item, an `li` in a `ul` or `ol`
  that is one). Formatted text in a line is in the elements `q2 render`
  writes for it (`strong`, `em`, `u`, `del`, `span.mark`, `code`,
  `a[href]`), and the line may hold a zero-width space (U+200B) where the
  caret is drawn, which the specs leave out of its text and offsets. An
  empty field's placeholder is
  its `::before` content. A horizontal rule is an `hr` among the body's
  blocks. A block the editor doesn't know is a
  `.qe-unknown` box, and the row that adds title-block fields is
  `.qe-widget`.
- The toolbar is found by its roles: a `toolbar` named "Formatting",
  holding a "Text style" button that opens a "Text style" `menu` of
  `menuitemradio`s, "Undo" and "Redo" buttons, and "Bold", "Italic",
  "Underline", "Highlight", "Link", "Bulleted list", "Numbered list" and
  "Quote" buttons with `aria-pressed`.
- A link's popover is `.qe-linkbar`, holding its address as a link and
  "Edit" and "Remove" buttons, and the link form is the `form` in
  `.qe-linkform`, with `text` and `url` inputs; both are in the document's
  frame.

### What no spec covers

No spec describes these, and the prototype doesn't do them: right-to-left
and mixed-direction text, touch input, and showing where collaborators'
carets are. Accessibility is barely covered: the title-block widget's and the
toolbar's buttons are found by their role and name, and nothing else is. Pressing Enter
to confirm an input-method composition is also unspecified: the specs
play an input method's part with synthetic events (`Editor.compose`),
which can't reproduce that key.

### Why the e2e suite isn't faster

We profiled the suite (2026-11) and decided to leave it as it is. A full
Chromium run takes about 3–4 minutes, set by `body.spec.ts`: Playwright
runs a file's tests one after another, and spreads only whole files across
workers.

- **Running a file's tests in parallel** (`fullyParallel`) halves a run
  at 16 workers, for the same CPU time. We don't, because the suite
  then takes over the machine: several agents running it at once slow
  each other (and whoever is using the machine) down, and slow tests
  time out.
- **Opening a document** takes about a second. Half of that is the app
  compiling its default theme, which only changing the app would avoid
  (shipping it precompiled); that saves about 10% of a run.
- **Serving a production build** (`vite preview`) instead of the dev
  server saves about 0.4s an open, less than the 14s build it needs
  first.
- **Moving specs out of the browser** would save little, and would pin
  the implementation the specs are meant to leave open.
