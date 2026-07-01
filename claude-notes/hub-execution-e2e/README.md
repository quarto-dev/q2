# End-to-end test: run code from hub-client via a `q2` executor

A hands-on test of the remote code-execution feature (bd-sfet3264, Phases
4a + 4b): you click **Run** in the hub-client editor and a connected `q2`
process executes the document's code and streams the output back into the
preview — the path a real collaborator would use.

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
                 │  --allow-all          │  runs engines, writes captures
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

3. **provider** (terminal 2):
   ```bash
   # from the repo root
   cargo run --bin q2 -- provide-hub --server wss://sync.automerge.org --allow-all --token dev <indexDocId>
   ```
   - `--token dev` skips the interactive OAuth bridge (the public server ignores
     the bearer). **Local testing only.**
   - `--allow-all` opts this machine in to running code for anyone in the
     session. Without it the provider is *fail-closed* (connect + list + exit).

   You should see `Connected. N file(s)…` (N > 0) and `Execution ENABLED…`.

4. In the browser: open the document, switch to the **preview**, and click
   **Run**. The button shows **Executing…**; within a moment the preview shows
   the executed output in place of the source. Edit the cell and **Re-run** to
   see it update; a staleness note appears when the code changed since the last
   run.

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
- **provider**:
  ```bash
  cargo run --bin q2 -- provide-hub --server ws://127.0.0.1:3031 --allow-all --token dev <ID>
  ```

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
  sync server as hub-client, or against a different index id. Confirm the
  provider printed `Execution ENABLED` and `N file(s)` with N > 0.
- **Run does nothing / error in the bar:** the engine isn't installed or failed;
  the provider terminal logs `exec request failed …`. Sanity-check the engine
  standalone: `cargo run --bin q2 -- render <doc>.qmd --to html` should contain
  the computed output.
- **Preview blank / won't render:** build the WASM — `cd hub-client && npm run build:wasm`.

## Notes & caveats

- **Single executor.** Two `provide-hub` processes on one project both execute
  every request (v1 limitation; see the plan's "Known limitations").
- **`--allow-all` is the only mode wired today.** The safer provider-only
  default (only your own requests run) needs Phase 5; here any peer can trigger
  execution — fine for a solo test.
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
