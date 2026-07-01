# Local end-to-end test: run code from hub-client via a `q2` executor

This is a hands-on test of the remote code-execution feature (bd-sfet3264,
Phases 4a + 4b). It wires up three local processes so you can click **Run** in
the hub-client editor and watch a connected `q2` process execute the document's
code and stream the output back into the preview — the same path a real
collaborator would use.

```
 ┌───────────────┐  automerge /ws (no auth)  ┌────────────────┐
 │ hub-client    │◄────────────────────────►│ q2 hub         │  (sync server,
 │ (browser,     │      index + file +       │  --project …   │   watches ./project)
 │  npm run dev) │      capture docs         └────────────────┘
 │               │                                   ▲
 │  Run button ──┼──── exec/request (ephemeral) ─────┤ /ws
 │  shows output │◄─── capture doc + sidecar ────────┤
 └───────────────┘                           ┌────────────────┐
                                             │ q2 provide-hub │  (executor:
                                             │  --allow-all   │   runs engines,
                                             └────────────────┘   writes captures)
```

Everything runs on `127.0.0.1` with **no authentication** — this is a local
testing setup, not how production auth works.

## What was verified vs. what you're checking

Verified directly (real binaries) while writing this:

- `q2 hub --project …` runs a no-auth local hub; `GET /health` returns the
  project's `index_document_id`.
- `q2 provide-hub --token dev --server ws://127.0.0.1:… <id>` connects to that
  hub, syncs the index, and lists the files.
- The Jupyter engine executes `2 + 3` → `5` on this machine (via `q2 render`),
  and the provider uses the same engine registry.
- The provider's execute loop (receive `exec/request` → run engine → write the
  capture back over automerge) is covered by an automated integration test
  (`crates/quarto-hub-provider/tests/integration/execute.rs`).

What this walkthrough adds is the **browser click-through**: the hub-client Run
button → the output appearing in the preview. That last mile needs a real
browser and can't be automated here, so you drive it by hand below.

## Prerequisites

- A built `q2` binary (the steps use `cargo run`, which builds on demand).
- Node.js + the repo's npm deps installed (`npm install` from the **repo root**).
- **A real execution engine on this machine**, matching the document:
  - `engine: jupyter` needs `python3` + `jupyter` with a `python3` kernel, or
  - `engine: knitr` needs `R` (with `knitr`).
  The example uses `engine: jupyter`. If you only have R, change the front
  matter to `engine: knitr` and the cell fence to ```` ```{r} ````.
- The hub-client **WASM must be built once** (the dev server does not build it):
  ```bash
  cd hub-client && npm run build:wasm
  ```
  Without it the preview won't render (and you won't see spliced output).

> **The `engine:` front-matter key is required.** A code cell is only
> *executable* when the document declares an engine (`engine: jupyter` /
> `engine: knitr`). Without it the cell renders as source, no Run button
> appears, and nothing executes. This is why `hello.qmd` starts with
> `engine: jupyter`.

## Run it (three terminals)

You can let the helper script do terminal 1 and print the exact commands/URL for
terminals 2 and 3:

```bash
cd claude-notes/hub-execution-e2e
./start-local-hub.sh            # builds q2, starts the hub, prints the id + URLs
```

Or do it by hand:

### Terminal 1 — the local hub (sync server)

```bash
# from the repo root
cargo run --bin q2 -- hub --project claude-notes/hub-execution-e2e/project --port 3031
```

Get the project's index-document id (needed for the URL and the provider):

```bash
curl -s http://127.0.0.1:3031/health | sed 's/.*"index_document_id":"\([^"]*\)".*/\1/'
# → e.g. 31JerQoChyQCsWnrQPCbFuCRxuiM
```

### Terminal 2 — hub-client, pointed at the local hub

```bash
cd hub-client
VITE_DEFAULT_SYNC_SERVER=ws://127.0.0.1:3031 npm run dev
```

`VITE_DEFAULT_SYNC_SERVER` points the app at the local hub, and because
`VITE_GOOGLE_CLIENT_ID` is unset the app runs with **auth disabled** (no login
screen). Open the example project (substitute the id from Terminal 1):

```
http://localhost:5173/#/share/<ID>?server=ws://127.0.0.1:3031&file=hello.qmd&name=Local%20demo
```

(The `#/share/…` route needs all three query params — `server`, `file`, `name`.)

### Terminal 3 — the execution provider

```bash
# from the repo root
cargo run --bin q2 -- provide-hub --server ws://127.0.0.1:3031 --allow-all --token dev <ID>
```

- `--token dev` skips the interactive OAuth bridge and hands the (no-auth) hub a
  placeholder bearer, which it ignores. **Local testing only.**
- `--allow-all` opts this machine in to running code for anyone in the session.
  Without it the provider is *fail-closed*: it connects, lists files, and exits.

You should see:

```
Using a static bearer token (dev mode; the hub must not require auth).
Connected. 1 file(s) in the project:
  hello.qmd
Execution ENABLED for all collaborators (--allow-all).
This project's code will run on THIS machine on request. Press Ctrl-C to stop.
```

### In the browser

1. Open `hello.qmd` and switch to the **preview** pane.
2. Because an executor is online and the file has executable cells, a green
   **Run** bar appears at the top of the preview (instead of the plain
   "Executor online" indicator).
3. Click **Run**. The button shows **Executing…**, the provider runs the cell,
   and within a moment the preview shows the executed output — `5` for
   `2 + 3` — spliced in place of the raw source.
4. Edit the cell (e.g. `2 + 40`) and click **Re-run**: the button reflects
   progress and the output updates. If the code changed since the last run, the
   bar notes "Code changed since the last run."

## Troubleshooting

- **No Run bar, only "Executor online" (or nothing):** the document has no
  executable cell — check the `engine:` front-matter key and that the fence is
  ```` ```{python} ```` / ```` ```{r} ````, not ```` ```python ```` or
  ```` ```{.python} ````.
- **No "Executor online" at all:** the provider isn't connected. Confirm
  Terminal 3 printed "Execution ENABLED" and used the **same** `--server` URL
  and index id, and that Terminal 2's `VITE_DEFAULT_SYNC_SERVER` matches.
- **Run does nothing / error in the bar:** the engine isn't installed or failed.
  The provider terminal logs the error (`exec request failed …`). Verify the
  engine runs standalone, e.g. `cargo run --bin q2 -- render claude-notes/hub-execution-e2e/project/hello.qmd --to html` should produce `<code>5</code>`.
- **Preview is blank / won't render:** the WASM isn't built — run
  `cd hub-client && npm run build:wasm`.
- **Port 3000 conflict:** hub-client's dev proxy default is 3000; keep the hub on
  a different port (this uses 3031).

## Notes & caveats

- **Single executor.** Running two `provide-hub` processes against one project
  makes both execute every request (v1 limitation; see the plan's "Known
  limitations").
- **`--allow-all` is the only mode wired today.** The safer provider-only
  default (only your own requests run) needs Phase 5. Here, any peer in the
  session can trigger execution — fine for a local solo test.
- **Every run creates a fresh capture doc** (always-fresh execution); old
  capture docs accumulate until server-side GC exists (Phase 5/6).
- Design + phase details: `claude-notes/plans/2026-06-29-remote-execution-provider.md`.
