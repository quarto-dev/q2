# rich-editor (prototype)

A prototype editor for Quarto 2 documents. It uses the same backend as
hub-client: `@quarto/quarto-sync-client` for automerge sync and the
`wasm-quarto-hub-client` module for parsing qmd and writing it back. All of
the UI lives in one component, `src/Editor.tsx`.

The window has two panes: on the left, the file's source in a plain textarea
(for debugging); on the right, the visual editor, which so far renders only
the title block. It uses hub-client's `PreviewTitleBlock` with the default
theme CSS, fed by `deriveTitleBlockMeta`, a WASM call that runs the render
pipeline's author normalization on the document's metadata in well under a
millisecond instead of waiting for a full render.

The title, subtitle and abstract are editable in place. The title block is one
`contenteditable` surface run by a small editing engine in `src/engine/`:
the browser moves the caret and selection (so selections can span fields),
and the engine intercepts every edit, applies it to a model of plain-text
fields, and renders it back. Its rules (Enter moves to the next field,
fields are never joined or split, an empty field disappears when left) and
its single undo history are pure TypeScript with unit tests. Like
`src/quarto-synced-file/`, it imports nothing from the rest of the demo;
the title block's fields are configured in `src/VisualEditor.tsx`.

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
front-matter keys, and sends the difference as one splice. While the
source doesn't parse, the editor shows the last text it could read and
takes no edits.

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

## Tests

```bash
npm run test:unit   # editing logic, in Node
npm run test:wasm   # needs the WASM build from step 1
npm run test:e2e    # the app in Chromium, on a test hub; needs the WASM build
```

The e2e tests are the specification of the editing experience, written
against what a person sees and what the source text holds, not against
this implementation: `title-block.spec.ts` and `cross-field.spec.ts`
(editing), `source.spec.ts` (what an edit does to the source text) and
`collaboration.spec.ts` (more than one person in a document). Tests marked
`fixme` describe behaviour we want and don't have yet.

Each test puts a document from `e2e/fixtures/` on an in-process sync hub
and opens it in the app, once per collaborator, each in a browser of its
own; the test itself is one more collaborator, reading and editing the
source text. Playwright starts the dev server itself.

`/fixture.html?fixture=<name>` (default `basic`) opens the visual editor
on a fixture held in memory, with no hub, for trying it without a server.
