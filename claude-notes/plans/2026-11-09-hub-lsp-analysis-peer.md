---
title: '`q2 hub-lsp`: a hub peer that answers document-structure questions over LSP'
date: 2026-11-09
description: 'Adds a q2 subcommand that joins a hub as a samod client peer, follows the projects named by its LSP workspace folders, and answers `quarto/projectComments` over stdio, so a host such as the Connect quartohub satellite can ask q2 about document structure.'
status: in-progress  # phase 1 (prototype) done; phase 2 not started
braid:
  strand: bd-xaxuifhm
  priority: P2
  labels: [hub, lsp]
---

## Overview

Posit Connect's quartohub satellite runs `q2 hub` for collaborative editing.
It needs to know when editorial comments are created or resolved in a project, so it can raise Connect notifications.
Comments are only the first such question: hosts of a hub will keep asking about document structure.
Rather than reimplement qmd understanding in each host, q2 serves the answers.

`q2 hub-lsp` is a new subcommand that:

1. dials a hub's sync websocket as a samod client peer, sending any extra handshake headers the host requires (quartohub's gateway requires `X-Quartohub-Relay-Key`);
2. follows the projects its LSP client names as workspace folders: each project's index document and every file document the index lists;
3. serves LSP over stdio, so only the parent process can reach it;
4. answers a custom request, `quarto/projectComments`, from `DocumentProfile.comments`;
5. sends a custom notification, `quarto/documentsChanged`, carrying only document ids, so the client knows when to ask again.

The client decides what to do with the answers.
In quartohub, that is debouncing, diffing comment sets, and calling Connect.
The full cross-repository design lives in Connect's `agent-plans/2026/11/05/quartohub-comment-notifications.md` (branch `cscheid/quartohub-comment-notifications`).

## Prior art in this repository

- `crates/quarto-hub-provider/` (`q2 provide-hub`) joins a hub as a samod client peer with a custom `Dialer`, finds the index document, lists its files, and reads file text (`materialize.rs`).
  Its integration tests run a bare samod acceptor behind a tungstenite server (`tests/integration/join.rs`).
- `crates/quarto-lsp/` serves LSP over stdio with `tower-lsp`; `crates/quarto-lsp-core/` holds transport-agnostic analysis.
- `crates/quarto-core/src/document_profile.rs`: `DocumentProfile::extract` collects comments (v13, GH #445).
  `ProfileComment` already reads in-band `author=` and `date=` attributes, and `ProfileComment::to_json` resolves 1-based positions.
- samod `DocHandle::changes()` streams change events for one document.

## Protocol

Documents and projects are named by Automerge URLs, `automerge:` followed by the document id.

| Direction | Message | Shape |
|---|---|---|
| client to server | `initialize` (standard) | `workspaceFolders`: one folder per project, its `uri` the index document's Automerge URL |
| client to server | `workspace/didChangeWorkspaceFolders` (standard) | added and removed projects |
| client to server | `quarto/projectComments` (custom request) | params `project`; result `project` plus `files`, each with `path`, `document` and `comments` (`JsonComment` objects) |
| server to client | `quarto/documentsChanged` (custom notification) | `project` plus `documents`, the Automerge URLs that changed since the last notification for that project |

The server advertises `workspace.workspaceFolders` support with change notifications.
Text document synchronization is off: the server owns the text, which it reads from the hub.

Only files whose path ends in `.qmd` are analyzed for comments in phase 1.
Each file is parsed on its own, so a comment in an included file is reported for the included file, not the including one.

## Command line

```bash
q2 hub-lsp --server ws://127.0.0.1:40404/ws \
  --header-file X-Quartohub-Relay-Key=/path/to/relay-secret
```

`--header-file NAME=PATH` reads the header value from a file, trimming a trailing newline.
Secrets do not go on the command line or in the environment, where other processes running as the same user can read them.
`--header-file` may repeat.

## Work items

### Phase 1: prototype

Tests come first in each item.

- [x] `quarto-hub-provider`: generalize the dialer.
  Tests: a `HeaderDialer` handshake request carries every configured header; a non-ws(s) URL is a permanent failure; an integration test where the acceptor rejects a handshake without the expected header, and accepts it with the header.
  Then: factor the connect path out of `BearerDialer` and add `HeaderDialer` with static headers.
- [x] New crate `quarto-hub-lsp`: comment extraction from qmd text.
  Tests: one comment with its text and 1-based position; an attribute-stamped comment carries `author`; a document with no comments yields an empty list; an empty mark is reported with empty text (the client filters it).
  A file that does not parse is a `ParseFailure`, not an empty list: a mark still being typed (`[>>` with no closing bracket) is Q-2-21, and a client must not read it as every comment having been resolved.
- [x] `quarto-hub-lsp`: the hub workspace.
  Tests, against an in-process samod acceptor: following a project reports the comments of each `.qmd` file; an edit made on the hub side is reflected in the next query; a file added to the index is followed; a change produces a change hint naming the document; unfollowing stops hints.
- [x] `quarto-hub-lsp`: the LSP server.
  Tests, driving the `tower-lsp` service in process: `initialize` with a workspace folder, then `quarto/projectComments` returns that project\'s comments; `workspace/didChangeWorkspaceFolders` adds a project; a hub-side edit produces a `quarto/documentsChanged` notification; an unknown project is an error response.
- [x] `quarto`: the `hub-lsp` subcommand with `--server` and `--header-file`.
  Tests: a `--header-file` argument splits at its first `=`, needs both a name and a path, reads its value without the trailing newline, and names the header and file when the file cannot be read.
- [x] End-to-end: `q2 hub-lsp` against a running `q2 hub`, driven over stdio, with the invocation and observed output recorded below.
- [x] Exit on `exit` while the client keeps stdin open (found by the end-to-end run).
  `tower_lsp::Server::serve` returns only when its input ends, and dropping the tokio runtime then waits on the blocking stdin read.
  `server::serve` now stops once an `exit` notification has been handled, and the command shuts its runtime down without waiting.
  Tests: `serve_returns_after_exit_while_the_input_stays_open` (in process) and `hub_lsp_cli` (the real binary against a real `q2 hub`).
  `q2 lsp` is built the same way and likely has the same bug; it is left alone here.
- [x] Full workspace suite: green except 35 environment failures on the development machine. 30 come from pandoc 3.13 against the pinned 3.12.1 (filed as bd-dti84hkx); 5 from the local R setup (a missing `bsicons` package, a different R error class, and no graphics device for knitr in 14 typst smoke-all fixtures). None of the failing crates changed here.

### End-to-end record

A scratch project with `index.qmd` containing `[>> test! ]` on line 7, served by `q2 hub --project <dir> --port 47811`.
The driver, a Python LSP client speaking `Content-Length` framing over the pipes, runs:

```bash
q2 hub-lsp --server ws://127.0.0.1:47811/ws --header-file X-Relay-Key=<file>
```

It sends `initialize` with the project as a workspace folder, then `quarto/projectComments`.
It then appends a second, attribute-stamped comment to `index.qmd` on disk, which the hub syncs into the file document, waits for `quarto/documentsChanged`, and queries again.
Finally it sends `shutdown` and `exit` and waits for the process.
Observed (inspected by hand; ids shortened):

```text
initialize -> {"capabilities": {"workspace": {"workspaceFolders": {"changeNotifications": true, "supported": true}}}, "serverInfo": {"name": "quarto-hub-lsp", "version": "0.42.0"}}
projectComments (before edit) -> files: [{path: "index.qmd", comments: [{text: "test!", start_line: 7, start_column: 1, end_line: 7, end_column: 12, file: "index.qmd"}]}]
notification -> {"method": "quarto/documentsChanged", "params": {"documents": ["automerge:v3Yp…"], "project": "automerge:3yEA…"}}
projectComments (after edit) -> comments: [{text: "test!", start_line: 7, …}, {text: "second comment", author: "u-1", start_line: 9, start_column: 12, …}]
hub-lsp exited with 0
```

### Phase 2: later

- [ ] Carry a comment\'s identifier (`#c-7` in its attribute block) in `ProfileComment` and `JsonComment`, with a `profile_version` bump.
  Today the identifier is dropped: only key-value attributes survive, so a client cannot use it as a stable comment key.
- [ ] Project-aware analysis, so comments in included files count toward the including document.
- [ ] Attribute each comment to the author of its span, using Automerge history.
- [ ] Standard LSP requests (`textDocument/documentSymbol`, diagnostics) on hub documents.

## Design notes

- **Why a new crate.** The peer needs samod and `quarto-hub`; the server needs `tower-lsp`.
  `quarto-lsp-core` must stay WASM-compatible, and `quarto-lsp` is the editor-facing server whose client owns the text.
  A separate crate keeps both of those unchanged.
- **Change hints are coalesced.** Each followed document has a task reading `DocHandle::changes()`.
  Hints for one project are batched for a short window before one notification goes out, so typing does not flood the client.
- **Determinism.** Results list files sorted by path and comments in source order, so a client can diff two answers without sorting.
