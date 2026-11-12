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

The title and subtitle are editable in place. The title block is one
`contenteditable` surface run by a small editing engine in `src/engine/`:
the browser moves the caret and selection (so selections can span fields),
and the engine intercepts every edit, applies it to a model of plain-text
fields, and renders it back. Its rules (Enter moves to the next field,
fields are never joined or split, an empty field disappears when left) and
its single undo history are pure TypeScript with unit tests. Like
`src/quarto-synced-file/`, it imports nothing from the rest of the demo;
the title block's fields are configured in `src/VisualEditor.tsx`. Edits
are local for now and don't reach the source text.

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
npm run test:e2e    # the visual editor in Chromium; needs the WASM build
```

The e2e tests run against `fixture.html`, which opens the visual editor on
a document from `e2e/fixtures/` with no hub
(`/fixture.html?fixture=<name>`, default `basic`). Playwright starts the
dev server itself.
