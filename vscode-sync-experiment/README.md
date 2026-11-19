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
- **Quarto Hub: Share this folder** — publish the open folder as a new project
  that browsers, mirrors, and other clones can join. You choose the sync server
  (default `wss://sync.automerge.org`; any `ws://` or `wss://` URL works, and the
  choice is remembered per folder), and a confirm says what is published before
  anything leaves your machine. A git clone publishes its tracked files and
  becomes a git-enabled project; a plain folder publishes its files and becomes a
  plain project. Files over 10 MB are skipped and listed in the Quarto Hub output.
  Uncommitted edits are published as they are on disk, and the confirm names them.
  A file whose name looks like a secret (`.env`, `.Renviron`, `*.pem`, or a name
  containing `secret` or `token`) is listed in the confirm too, so you can stop and
  remove it first.
  - **The public server cannot unpublish.** Anyone who gets the link can read and
    edit the project, and nothing deletes it afterward. Use a server you control for
    anything private.
  - **The share panel** appears only after the server confirms it holds everything.
    It offers three forms, and selecting one copies it: a share link, the
    `q2 preview --hub-url <server> --project <id> --ui editor --allow-edit` command,
    and the server and project id (the Join command also accepts the link). If the
    upload is interrupted, reopen the folder to finish it, then run
    **Quarto Hub: Show share information**.
  - **The owner.** The sharing folder owns the project: `.quarto-hub/project.json`
    records an owner id that matches the project's `owner` key. In a git clone, when
    you check out another version of a file, the owner re-points the project at the
    right version for everyone following it, removes files git removed, and
    publishes files git adds, each as one change. Another clone of the same
    repository never does this, even with the same user. Ownership cannot be
    transferred yet.
- **Quarto Hub: Show share information** — show the share panel again for a shared folder.
- Setting `quartoHub.userName` — the label shown next to your cursor.

Projects you create from a folder (Create Folder from Project) use
`wss://sync.automerge.org`; Share this folder accepts any `ws://` or `wss://`
server. There is no authentication.
The buffer's dirty indicator is cosmetic — saving is a no-op because edits
have already synced.

## Develop

```bash
npm install                      # from the repo root (workspace member)
npm run build -w quarto-hub-vscode   # or: npm run watch
```

Then open `vscode-sync-experiment/` as a folder in VS Code, go to the
Run and Debug view in the sidebar, and start **Run Quarto Hub Extension**.
That builds the bundle and opens a second VS Code window with the extension
loaded; run "Quarto Hub: Connect to Document" from its command palette.
Alternatively `npm run package` and install the `.vsix`.
