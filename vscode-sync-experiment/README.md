# Quarto Hub for VS Code (prototype)

Opens a Quarto Hub file as a live-synced VS Code buffer and shows the cursors
of people editing it in the web app.

- **Quarto Hub: Connect to Document** — paste the file's Automerge document
  id (with or without the `automerge:` prefix). A `quartohub://` buffer opens;
  every keystroke is spliced into the Automerge doc, and remote changes are
  written back into the buffer.
- **Quarto Hub: Disconnect** — closes the connection for the active buffer.
- **Quarto Hub: Create Folder from Project** — paste a project's index
  document id and pick a new folder. The project's files are written to disk
  and the folder opens as a workspace that stays in sync while open. Files
  open in an editor use the live buffer binding (VS Code autosaves them);
  everything else is written to disk directly. Local edits, creates, and
  deletes, including ones made while VS Code was closed, are pushed back on
  the next start as proper merges, using the Automerge history kept in the
  folder's `.quarto-hub/` directory (never synced; add it to your own gitignore).
- Setting `quartoHub.userName` — the label shown next to your cursor.

Only `wss://sync.automerge.org` is supported; there is no authentication.
The buffer's dirty indicator is cosmetic — saving is a no-op because edits
have already synced.

## Develop

```bash
npm install                      # from the repo root (workspace member)
npm run build -w quarto-hub-vscode   # or: npm run watch
```

Then open `hub-client/vscode-sync-experiment/` as a folder in VS Code, go to the
Run and Debug view in the sidebar, and start **Run Quarto Hub Extension**.
That builds the bundle and opens a second VS Code window with the extension
loaded; run "Quarto Hub: Connect to Document" from its command palette.
Alternatively `npm run package` and install the `.vsix`.
