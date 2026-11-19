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
- **Quarto Hub: Join Session** — paste whatever you have: a share ticket
  (`q2preview…`), a hub URL (`http://127.0.0.1:3000`, `ws://…`), or a
  quarto-hub.com share link. The command works out the project and where it
  lives, then offers only the modes that apply, default first:
  - **Attach this folder** (git clone of the shared repository),
  - **Create a mirror folder**,
  - **Editor only** (the hub owns the files; offered when the hub's project
    folder is the folder you have open, and confirmed first),
  - **Rebind this folder to the new session** (the open folder is already bound).

  The mode is written to `.quarto-hub/project.json`, and the status bar shows it.
- **Quarto Hub: Leave Session** — stops syncing the joined folder and ends the
  join proxy. The folder stays bound.
- Setting `quartoHub.q2Path` — the `q2` executable that Join Session runs for a
  ticket (default `q2` on `PATH`). It has machine scope, so a workspace
  `settings.json` cannot set it. On Windows use `q2.exe`; a `q2.cmd` also works.
- Setting `quartoHub.userName` — the label shown next to your cursor.

Create Folder from Project and Attach Folder use `wss://sync.automerge.org` unless a share
link names another server; Share this folder accepts any `ws://` or `wss://` server; Join
Session uses the server of what you paste. There is no authentication.

### Tickets

A ticket makes the extension start `q2 preview --join <ticket> --host 127.0.0.1 --port N
--no-browser --exit-on-stdin-close` as a local proxy to the host's session (the extension has
no iroh of its own), and sync through `ws://127.0.0.1:N/ws`. The ticket is held in memory
only: it is never written to `project.json` or to the Quarto Hub output channel. It is
visible, though, in the process argument list to other users of the same machine; treat a
ticket as a credential for that session. The proxy exits when the extension host does. The
proxy's port changes every run, so a folder bound through a ticket is stale after a restart:
the next open reports the project as unavailable and offers to rebind.
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
