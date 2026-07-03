# End-to-end test: run code from hub-client via a `q2` executor

A hands-on test of the remote code-execution feature (bd-sfet3264, Phases
4a + 4b): a connected `q2` process executes the document's code and streams the
output back into the preview — the path a real collaborator would use.

> **`q2 provide-hub` is consent-gated and one-shot by default (bd-9lgiulr4).**
> Every execution requires you to accept an interactive prompt that shows the
> *resolved document* (the post-include QMD the engine will receive). Two modes:
> - **one-shot (default):** `q2 provide-hub --file <doc> …` connects, executes
>   that one document once (after you accept), pushes the result, and exits.
> - **`--watch`:** stay online and serve the editor's **Run** button until
>   Ctrl-C (each request still consent-gated).
>
> Add `--dangerously-accept-requests` to skip the prompt (unattended — only in a
> fully-trusted session). A non-interactive stdin refuses to execute (fail-safe).
> The old `--allow-all` flag is gone.

You run hub-client locally (`npm run dev`) plus a `q2 provide-hub` executor.
Both talk to an automerge **sync server**, which stores and relays the project
docs (index, files, captures) and the ephemeral run requests/beacons. There are
two ways to provide that sync server — pick one:

```
 ┌───────────────┐   automerge sync (index+file+capture docs, ephemeral msgs)
 │ hub-client    │◄─────────────────────┐
 │ (npm run dev) │                       │
 │  Run button ──┼──── exec/request ─────┤   ┌──────────────────────┐
 │  shows output │◄─── capture doc ──────┤◄─►│  sync server          │
 └───────────────┘                       │   │  (A: sync.automerge.org
                 ┌──────────────────────┐│   │   B: local q2 hub)    │
                 │ q2 provide-hub        ├┘   └──────────────────────┘
                 │  (consent-gated)      │  runs engines, writes captures
                 └──────────────────────┘
```

## Option A — public sync server (simplest; verified)

Uses `wss://sync.automerge.org` (hub-client's built-in default) as the sync
server. **No `q2 hub` needed** — the public server stores and relays every doc
any peer creates, so a project you make in hub-client is visible to the
provider. Requires internet, and note that **your document text + code are
pushed to a public server** (fine for throwaway test code; use Option B if you
care).

### Prerequisites

- A built `q2` (the commands use `cargo run`, which builds on demand).
- `npm install` from the **repo root**.
- **A real engine matching the document:** `engine: jupyter` needs
  `python3` + `jupyter` with a `python3` kernel; `engine: knitr` needs `R`.
- **Build the hub-client WASM once** (the dev server doesn't):
  `cd hub-client && npm run build:wasm`. Without it the preview won't render.

### Steps

1. **hub-client** (terminal 1), on its default sync server:
   ```bash
   cd hub-client
   npm run dev            # do NOT set VITE_DEFAULT_SYNC_SERVER
   ```
   Open http://localhost:5173, **create a new project**, and add a document
   with an executable cell — front matter **must** declare an engine:
   ```
   ---
   title: demo
   engine: knitr        # or: jupyter
   ---

   ```{r}
   cat(1, 2, 3)
   ```
   ```
   > **The `engine:` key is required.** Without it the cell renders as source,
   > no Run button appears, and nothing executes.

2. Get the project's **index-document id**: click **Share** in hub-client; the
   id is the part after `#/share/` in the URL it shows. (Shortcut: you can pass
   the *whole* Share URL to the provider — it extracts the id.)

3. **provider** (terminal 2) — to use the editor's **Run** button you want
   `--watch`:
   ```bash
   # from the repo root
   cargo run --bin q2 -- provide-hub --watch --server wss://sync.automerge.org --token dev <indexDocId>
   ```
   - `--token dev` skips the interactive OAuth bridge (the public server ignores
     the bearer). **Local testing only.**
   - `--watch` stays online and serves requests. Each request pauses for you to
     **review the resolved document and accept** at the terminal (or add
     `--dangerously-accept-requests` to auto-accept). Without `--watch` the
     command is one-shot and needs `--file <doc>` instead.

   You should see `Connected. N file(s)…` (N > 0) and `Watching for execution
   requests…`.

4. In the browser: open the document, switch to the **preview**, and click
   **Run**. Back in terminal 2, review the shown resolved-document path and
   press **1** to accept; within a moment the preview shows the executed output
   in place of the source. Edit the cell and **Re-run** to see it update; a
   staleness note appears when the code changed since the last run.

   To instead push a result **without** the Run button (one-shot), run:
   ```bash
   cargo run --bin q2 -- provide-hub --file <doc>.qmd --server wss://sync.automerge.org --token dev <indexDocId>
   ```
   accept the prompt, and the provider executes once, pushes the output to every
   collaborator, and exits (the editor then shows the output with no live
   executor).

## Option B — fully local (offline / private): `q2 hub`

Runs a local sync server so nothing leaves your machine. The example project
(`project/hello.qmd`, `engine: jupyter`) and the `start-local-hub.sh` helper
are for this path.

**Important:** `q2 hub --project <dir>` (project mode) only serves *its own*
watched project. So you must **open that project via the Share URL** — do **not**
create a new project in hub-client (a fresh hub-client project isn't stored by a
project-mode hub, and the provider will see `0 file(s)`).

```bash
cd claude-notes/hub-execution-e2e
./start-local-hub.sh          # builds q2, starts the no-auth hub, prints the URLs
```

It prints the project's index id (from `GET /health`) and the exact commands
for the other two terminals:

- **hub-client**, pointed at the local hub:
  ```bash
  cd hub-client
  VITE_DEFAULT_SYNC_SERVER=ws://127.0.0.1:3031 npm run dev
  ```
  Then open the printed **Share URL** (`…/#/share/<ID>?server=ws://127.0.0.1:3031&file=hello.qmd&name=Local%20demo`) — this opens the hub-owned project, not a new one.
- **provider** (watch mode, for the Run button):
  ```bash
  cargo run --bin q2 -- provide-hub --watch --server ws://127.0.0.1:3031 --token dev <ID>
  ```
  (accept each request at the prompt, or add `--dangerously-accept-requests`).
  For a one-shot push instead: `provide-hub --file hello.qmd --server
  ws://127.0.0.1:3031 --token dev <ID>`.

`q2 hub` runs with **no auth** (no `--oidc-client-id`), and hub-client runs with
auth off because `VITE_GOOGLE_CLIENT_ID` is unset — so there's no login screen.

(If you'd rather create projects freely in hub-client while staying local, run a
general relay instead of project mode: `q2 hub --no-project --port 3031`. It
stores/relays arbitrary docs the way `sync.automerge.org` does.)

## Troubleshooting

- **Provider prints `0 file(s)` / `project discovery failed: … No such file`:**
  the provider can't see the project's files. In Option B this means you created
  a *new* project instead of opening the hub-owned one via the Share URL (a
  project-mode hub only serves its own project). Use the Share URL, or switch to
  Option A / `q2 hub --no-project`.
- **No Run bar, only "Executor online" (or nothing):** the document has no
  executable cell — check the `engine:` front-matter key and that the fence is
  ```` ```{r} ```` / ```` ```{python} ````, not ```` ```r ```` or ```` ```{.r} ````.
- **No "Executor online" at all:** the provider isn't connected to the *same*
  sync server as hub-client, or against a different index id, **or you did not
  pass `--watch`** (only watch mode broadcasts the beacon that lights up
  "Executor online" / the Run button). Confirm the provider printed
  `Watching for execution requests…` and `N file(s)` with N > 0.
- **Run does nothing / error in the bar:** the engine isn't installed or failed;
  the provider terminal logs `exec request failed …`. Sanity-check the engine
  standalone: `cargo run --bin q2 -- render <doc>.qmd --to html` should contain
  the computed output.
- **Preview blank / won't render:** build the WASM — `cd hub-client && npm run build:wasm`.

## Notes & caveats

- **Single executor.** Two `provide-hub` processes on one project both execute
  every request (v1 limitation; see the plan's "Known limitations").
- **Consent-gated by default (bd-9lgiulr4).** Every execution requires you to
  accept an interactive prompt showing the resolved document; `--watch` adds a
  "3) accept all future" option. `--dangerously-accept-requests` skips the
  prompt. The per-project provider-only requester gating (only your own requests
  run) is still a separate Phase-5 concern.
- **Every run creates a fresh capture doc** (always-fresh execution); old
  capture docs accumulate until server-side GC exists (Phase 5/6).
- Design + phase details: `claude-notes/plans/2026-06-29-remote-execution-provider.md`.

### Verified directly while writing this

- Provider dials `wss://sync.automerge.org` (TLS) and syncs; an absent doc
  returns a fast "not found" (no hang).
- Provider against a hub-owned local project lists its files; the Jupyter/knitr
  engines execute on this machine (`q2 render`, same registry the provider uses).
- The provider execute loop (receive `exec/request` → run engine → write capture
  back) is covered by `crates/quarto-hub-provider/tests/integration/execute.rs`.
- The browser Run-click is the manual last mile this harness drives.
