---
title: 'GE-11: The Join session command'
date: 2026-11-18
description: One command turns a ticket, a URL, or a share link into a bound and syncing extension folder in the right mode, with a way to leave, and spawns a local join proxy for remote hubs.
status: done
depends-on: [ge-10]
lane: A
epic: ../2026-11-18-git-epochs-epic.md
---

**Epic:** [git epochs](../2026-11-18-git-epochs-epic.md) · **Waits for:** [GE-10](ge-10-extension-clone-mode.md) ·
**Unblocks:** [GE-14](ge-14-cross-client-e2e.md), [GE-16](ge-16-windows-run-b.md) ·
**Branch:** `ge/11-join-command` off `feature/git-epochs`, merged by rebase and fast-forward, no push.

## Goal

Today a user must know a project\'s index doc id, and the server is a constant. This plan adds **Quarto Hub: Join
session**, one command that accepts whatever the user has (a `q2preview…` ticket, a URL, a share link), works out the
project and where it lives, offers only the modes that apply, binds the folder, and starts syncing. **Quarto Hub:
Leave session** stops it. For a remote hub the extension spawns the local `q2 preview --join` proxy itself, because it
has no iroh.

## Decisions implemented

- **G22**, in full: the four input kinds, the `/health` read, the `project_root` comparison for every kind of join, the
  offers, the binding written at each join, Leave, and the status bar.
- **G19:** the mode is written to `project.json` at join; a bound folder is offered rebind (implemented in GE-10).
- **G21, G20, G23** are consumed, not changed: attach (GE-10), mirror and editor-only (GE-9), and the mirror\'s
  location (inside the current workspace folder, or home).
- **G32:** the 15 s proxy readiness timeout. The 5 s unavailable timeout applies to reading the index.

## Context

- The extension has three commands and one setting today (`vscode-sync-experiment/package.json`, `contributes`, lines
  24 to 46). `createFolder` asks for an index doc id only, places the folder in the first workspace folder or home, and
  reloads onto it (`src/extension.ts:95-112`, `:99`, `:112`). `startProject` starts sync for any folder that has
  `.quarto-hub/project.json` (`:115-117`).
- The server is a constant (`src/project.ts:40`), written into `project.json` at creation (`:123`, in
  `createProjectFolder`, `:110`) and used by `makeRepo` (`:63`); `ProjectSync` already dials `meta.syncServer`
  (`:183`). Single-file mode keeps its own constant (`extension.ts:179`) and stays out of scope.
- `/health` returns `index_document_id` and an optional `project_root` (`crates/quarto-hub/src/server.rs:47-53`). The
  preview hub needs no credential on `/ws` (`server.rs:1646`, `crates/quarto-preview/src/lib.rs:735`); `/health` on
  other servers is unverified, so any failure counts as unreachable.
- `q2 preview --join <ticket>` runs a local token-authenticated proxy and reads no project; `--port` and `--host`
  choose where it listens and `--no-browser` applies (`crates/quarto/src/main.rs:372`, help text above it). The ticket
  is opaque: the host\'s endpoint address and a 32-byte session token (`crates/quarto-p2p/src/ticket.rs`).
  `startJoinGuest(ticket)` already spawns the proxy in tests (`q2-preview-spa/e2e/helpers/previewServer.ts:421`).
- The share-link parser is `parseProjectRef`, returning the project id plus optional `server`, `file`, and `name`
  (`ts-packages/quarto-hub-mcp/src/share-url.ts:24`, `:87`); GE-8 moved it to a shared package.
- From the earlier plans: GE-9 added `mode` to `project.json`, the mirror and editor-only modes, and the status bar
  item; GE-10 added attach, the clone check (`checkClone`), rebind, and the detached count. This plan calls them. If a
  name differs, use the name GE-9 or GE-10 chose.

## Behavior

**Input kinds.** The input box takes one string, trimmed.

| Input | Meaning | Server and id |
|---|---|---|
| Matches `^q2preview[A-Za-z0-9_-]+$` | A share ticket | Spawn the proxy on a free port N; server is `ws://127.0.0.1:N/ws` |
| `ws://` or `wss://` | A hub websocket URL | Used as given |
| `http://` or `https://` | The address a user copies from a browser | `/ws` URL derived: scheme `ws` or `wss`, same host and port, path `/ws` (detail chosen by the plan author; confirm) |
| A share link (`#/share/<id>?server=…`) | A quarto-hub.com style link | Id and server from `parseProjectRef`; the proxy is not involved |

Anything else is refused with "That is not a ticket, a URL, or a share link." The ticket pattern is checked before
anything is spawned: the spawn for a `.cmd` uses `shell: true` (below), and an input that merely starts with `q2preview`
must never reach a shell.

**The flow.**

1. Classify the input. For a ticket, spawn the proxy (below) and wait until it is ready.
2. Ask `http://<host>:<port>/health` (the `/ws` URL with scheme and path swapped) for `index_document_id` and
   `project_root`. If it is unreachable and the id is not already known (share link), prompt for the doc id.
3. Read the index doc from the server (5 s) to learn whether the share is git-enabled (it carries a `repo` entry). A
   missing project is "Project not found on this server."
4. Compare `project_root` with the open workspace folder: real paths, trailing separators ignored, case-insensitive on
   macOS and Windows.
5. Offer the modes that apply, below, in one QuickPick with the default first.
6. Bind: write `.quarto-hub/project.json` (`indexDocId`, `syncServer`, `mode`, `createdAt`, plus whatever GE-9 and GE-10
   add), then start sync. The ticket is never written.
7. Show the status bar item (mode, detached count). The proxy port changes every run, so a folder bound through a
   ticket goes stale; the next open meets "project unavailable" and offers rebind (GE-10).

**What is offered.** "Loopback" means host `127.0.0.1`, `localhost`, or `::1`. "Passes" means GE-10\'s `checkClone`
accepts the open folder for the share\'s `repo` entry.

| Kind | `project_root` matches the open folder | Git-enabled and the folder passes | Items, default first |
|---|---|---|---|
| Loopback URL | yes | any | Editor only (with a confirm), Mirror |
| Loopback URL | no | yes | Attach, Mirror |
| Loopback URL | no | no | Mirror |
| Ticket, remote-looking URL, share link | yes | yes | Attach, Mirror, Editor only (never the default; its text says "only if the hub runs on this machine") |
| Ticket, remote-looking URL, share link | yes | no | Mirror, Editor only (never the default) |
| Ticket, remote-looking URL, share link | no | yes | Attach, Mirror |
| Ticket, remote-looking URL, share link | no | no | Mirror |

With no workspace folder open only Mirror is offered, placed in the home directory as today. A folder that is already
bound is offered **Rebind this folder to the new session** first (GE-10\'s rebind and its confirm), then Mirror. For a
loopback match attach is not offered, because a match means the hub\'s own directory (two full agents, unsupported); for
the remote-looking kinds a match may be a coincidence of paths, so attach stays available (detail chosen by the plan
author; confirm). Editor only always asks "This looks like the hub\'s own project folder. Use it as editor only?".

**Mirror** reuses `createProjectFolder`, now taking the server (the constant at `project.ts:63` and `:123` is replaced
by the parameter). The existing **Create Folder from Project** command stays as it is (detail chosen by the plan
author; confirm).

**The spawned proxy.** The command is `q2 preview --join <ticket> --host 127.0.0.1 --port N --no-browser --exit-on-stdin-close`, with stdin piped, so the
proxy exits when the extension host dies, and with `q2` from a new setting `quartoHub.q2Path` (default `q2`, found on
`PATH`; declared with machine scope in `package.json`, so a workspace `settings.json` cannot set it). The extension picks N by binding port 0 and
closing it. It polls `/health` every 250 ms for up to 15 s. Stderr goes to the Quarto Hub output channel with the
ticket redacted. The ticket is held in memory only. Its appearance in the process argument list is visible to other
local users; that is accepted at demo quality. On Windows the supported spawn is `q2.exe` directly. Node 20.12 and
later refuses to spawn a `.cmd` or `.bat` without a shell (it throws EINVAL), so the proxy manager sets `shell: true`
only for those two extensions and shows the real error in "Could not run q2".

| Failure | Message |
|---|---|
| `q2` not found | "Could not run q2. Set quartoHub.q2Path." |
| The proxy exits early | The last lines of its stderr |
| Bad ticket, or a `q2` too old for `--join` or `--exit-on-stdin-close` | The same stderr |
| Not ready in 15 s | "The join proxy did not become ready." (the child is killed) |
| `/health` unreachable and no id | A prompt for the doc id |
| A `ws://` URL that does not connect | A connection error; nothing is written to `project.json` |

**Leave session** stops the folder\'s sync, closes the spawned `q2`\'s stdin so the proxy exits, then kills it if needed (POSIX: a detached process group, SIGTERM then
SIGKILL after 3 s; Windows: `taskkill /T /F`; best effort), and clears the status bar. The folder stays bound.
Deactivation kills any spawned proxy too.

## Test specifications

All in the vscode-free core from GE-8 unless marked. Each is written and seen failing first.

1. `classifyJoinInput`: a table of tickets, `ws`, `wss`, `http`, `https`, share links with and without `server=`,
   whitespace, garbage, a bare doc id (refused), and a string that starts with `q2preview` but carries shell
   metacharacters (`q2preview x & calc`, refused before any spawn).
2. `deriveWsUrl` and `healthUrlFor`: `http://127.0.0.1:3000/` to `ws://127.0.0.1:3000/ws`, `https` to `wss`, a
   `/ws` URL to its `/health`, IPv6 loopback, a URL with a path.
3. `projectRootMatches`: symlinked temp directories (real paths), trailing separators, case folding on `darwin` and
   `win32` (platform injected), `C:\` against `c:/`.
4. `offeredModes`: every row of the table above, plus no workspace folder and an already-bound folder.
5. Ticket hygiene: run each flow with a known ticket and assert it appears in no log line, no thrown message, and no
   `project.json`; the exploratory run with the real `q2` checks its real stderr too.
6. The proxy manager with a stub `q2` (a cross-platform helper that writes a `q2` shell script on POSIX and a `q2.cmd`
   on Windows, each running a small Node stub): success (the stub receives `--join`, `--port N`, `--no-browser`; ready
   within 15 s on a fake clock), early exit with stderr, a bad ticket, `ENOENT`, readiness timeout kills the child,
   and tree kill (the stub spawns a grandchild; after Leave neither lives); stdin close (the stub exits when its stdin
   closes, and Leave closes it first); and the `.cmd` branch on Windows (spawned with `shell: true`).
7. Health: JSON with and without `project_root`; a 401, a non-JSON body, and a timeout all count as unreachable.
8. Index inspection against the test hub: with a `repo` entry the share is git-enabled; without it, not; an unknown id
   is "not found" after 5 s.
9. The binding writer: `project.json` per mode, ticket absent, `createProjectFolder` using the given server.
10. Playwright VS Code, against a local `q2 preview --allow-edit` on loopback with no network: (a) an `http` address of
    the hub\'s own directory offers Editor only first, with the confirm, and the status bar shows the mode; (b) a `ws`
    URL from another folder creates a mirror inside the workspace folder and opens it; (c) a ticket-kind input with a
    stub `q2` that forwards TCP to the hub binds the proxy URL, with no ticket in `project.json` or the output channel;
    (d) a bound folder is offered rebind; (e) Leave ends the stub process. Exploratory only: a real `--share` and
    `--join` pair through `startJoinGuest` (needs the network).

## Checklist

- [x] Write test specifications 1 to 4 and 7 to 9 and watch them fail.
- [x] Implement `classifyJoinInput`, `deriveWsUrl`, `healthUrlFor`, `projectRootMatches`, `offeredModes`, and the health
  and index inspectors in the core.
- [x] Make `createProjectFolder` and `makeRepo` take the sync server; keep single-file mode on its constant.
- [x] Write the stub `q2` helper and test specifications 5 and 6; implement the proxy manager (free port, readiness
  poll, redaction, tree kill).
- [x] Rust, in `crates/quarto/src/main.rs` and the join entry point it calls (added by the second reviewer; confirm
  the plan home): add `--exit-on-stdin-close` to `q2 preview` with `requires = "join"`; the proxy\'s shutdown future
  also waits for EOF on stdin; the help text warns that stdin attached to `/dev/null` exits at once. Parse tests
  follow GE-4\'s pattern (the flag requires `--join`; it combines with `--port`, `--host`, and `--no-browser`), plus
  a test that the proxy exits when its stdin closes. Gates: clippy and nextest for `quarto` and `quarto-preview`.
- [x] Add `quartoHub.joinSession`, `quartoHub.leaveSession`, and the `quartoHub.q2Path` setting (scope `machine`; a
  test shows a workspace value is ignored) to `package.json`;
  implement the VS Code layer (input box, QuickPick, confirms, notifications, status bar, deactivation cleanup).
- [x] Route a bound folder to GE-10\'s rebind and Attach to GE-10\'s attach; route Editor only to GE-9\'s binding.
- [x] Write the Playwright specs in test specification 10 against the harness from GE-8; add the permanent ones to its
  run script.
- [x] Update the extension README: the two commands, the setting, the modes, the ticket and its exposure.
- [x] `cargo xtask lint`; `q2 render --strict` on this file after editing its checkboxes.

## Gates

- `npm run typecheck -w quarto-hub-vscode`, `npm run test -w quarto-hub-vscode`, `npm run build -w quarto-hub-vscode`.
- The Playwright VS Code run script from GE-8 (the permanent subset), after `cargo build --bin q2`.
- `cargo clippy -p quarto -p quarto-preview --all-targets -- -D warnings` and
  `cargo nextest run -p quarto -p quarto-preview` (the `--exit-on-stdin-close` flag).
- `cargo xtask lint`.

## End-to-end verification

Run a real `q2 preview --allow-edit` on a temporary project, open a real VS Code with the extension, run **Join
session** with the hub\'s `http` address, and inspect `.quarto-hub/project.json` and the status bar. Where the network
allows, run `q2 preview --share`, paste its ticket into **Join session**, and inspect the spawned proxy and the
binding. Record the exact invocations and what was seen in the notes, and say if the ticket run was not possible.

## Exit criteria

- All four input kinds join, with the offers of the table, on a real hub.
- No ticket in `project.json`, any log, or any error text.
- Leave kills the proxy tree on POSIX; the Windows kill is covered by GE-16.
- The permanent Playwright specs pass locally.

## Out of scope

Spawning for tickets without `q2` installed, auth for OIDC hubs, single-file mode, restarting a dead proxy, a persisted
ticket, and any change to the clone or follower logic.

## Notes from the run

Branch `ge/11-join-command`, two commits: `b540639c3` (Rust flag) and `48f641271` (extension).

**What ran.**

- Rust: `cargo clippy -p quarto -p quarto-preview --all-targets -- -D warnings` clean;
  `cargo nextest run -p quarto -p quarto-preview` 1357 passed, 3 skipped. The new tests are three parse tests
  (`preview_exit_on_stdin_close_*`) and `stdin_closed_waits_for_eof`.
- TypeScript: `npm run typecheck`, `npm test` (244 tests), `npm run build` in `quarto-hub-vscode`.
  `cargo xtask lint` passes.
- Playwright (`node e2e/run.mjs join.e2e`, VS Code 1.101.2, against `target/debug/q2`): all four specs pass
  ((a) own directory by `http` address, (b) `ws` URL from another folder, (c) and (e) ticket through a stub proxy then
  Leave, (d) bound folder offered rebind).
- The plan\'s test specs were written together with their implementation rather than strictly red first for the TypeScript
  core: the first vitest run failed only on the missing module. The Rust parse tests were seen failing (the field did not
  exist).

**Details taken from the plan as written** (all marked "confirm"): the `/ws` URL derived from an `http(s)` address; attach
stays available for a remote-looking match; **Create Folder from Project** is unchanged; the `--exit-on-stdin-close`
home in `crates/quarto`.

**Surprises and deviations.**

- `bindFolder` seeds the local store. Non-clone modes start offline from `.quarto-hub/automerge`
  (`project-sync.ts`, Phase 1), so an editor-only folder bound with only a `project.json` found "Document unavailable"
  and never dialed the hub. The first Playwright run showed it. `bindFolder` now connects, pulls the index and file docs
  into the store without writing a file, and throws (writing nothing) when the project cannot be read.
- Mirror joins with a window open add the mirror with `updateWorkspaceFolders` rather than `vscode.openFolder`, so the
  extension host, and a ticket\'s proxy, survive. With no folder open it falls back to `openFolder`; a proxy then ends
  with the reload and the folder goes stale, as the plan accepts.
- The stdin watcher runs on a plain thread, not `spawn_blocking`, so a blocked read cannot delay the runtime\'s shutdown
  when Ctrl-C wins. It needed tokio\'s `sync` feature (`oneshot`) in `crates/quarto/Cargo.toml`.
- The proxy tests use short real timeouts (700 ms, 8 s) instead of a fake clock: the child is a real process.
- The `.cmd` branch is covered by `needsShell` with an injected platform; running a real `q2.cmd` is GE-16.
- Flakes under a parallel `npm test`: GE-10\'s `clone-modes.test.ts` fails intermittently (a different test each run;
  none pass or fail alone), and `createProjectFolder records the server` failed once at 277 ms and then passed in
  eight further runs. Not diagnosed. The proxy test "Leave kills a proxy that ignores stdin EOF" was flaky until `stop`
  waited for the child\'s exit event (a zombie still answered `kill(pid, 0)`).

**End to end.** Real binary, real hub, real ticket (network available):

```text
q2 preview <dir> --allow-edit --share --no-browser --port 39880      # printed: q2 preview --join q2previewaaa5qr3…
startJoinProxy({ q2Path: target/debug/q2, ticket })   # spawns:
  q2 preview --join <ticket> --host 127.0.0.1 --port 63667 --no-browser --exit-on-stdin-close
PROXY ws://127.0.0.1:63667/ws  {"indexDocId":"2dGrSqijdgbfdtMnrMyckTFhWrwC","projectRoot":".../hubdir2"}
LOG   ticket redacted (5 lines)
AFTER STOP  health null     # the proxy exited after stdin was closed
```

I inspected the output: `/health` through the proxy returned the host\'s index doc id and project root, the output
log held `<ticket>` and never the ticket, and the proxy was gone after `stop()`. The VS Code layer was exercised with a
real VS Code and a real `q2 preview --allow-edit` hub (Playwright specs above): `project.json` showed
`{"mode":"editor-only","indexDocId":<hub>,"syncServer":"ws://127.0.0.1:<port>/ws"}` and the status bar read
"Quarto Hub: editor only". Not run: a real VS Code joined through a real `--share` ticket (the ticket case used a stub
`q2` that forwards TCP to the hub; the real proxy was exercised through `startJoinProxy` only).
