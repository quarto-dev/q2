# rich-editor (prototype)

A prototype editor for Quarto 2 documents. It uses the same backend as
hub-client: `@quarto/quarto-sync-client` for automerge sync and the
`wasm-quarto-hub-client` module for parsing qmd and writing it back. All of
the UI lives in one component, `src/Editor.tsx`.

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
