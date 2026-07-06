# JS ↔ Rust automerge interop repro (bd-bm0vaetl)

Minimal, self-contained harness (no quarto-hub / hub-client / feature code) that
pinned why `q2 provide-hub` read **0 files** from a project created in
hub-client.

## The finding

hub-client uses `@automerge/automerge` 3.x, which stores a plain string
map-value (`doc.files[path] = docId`) as an automerge **`Text` object**, not a
scalar string. quarto's Rust `IndexDocument::get_all_files()` read values with
`Value::to_str()`, which returns `None` for `Text` — so a hub-client-authored
`files` map read as empty and the provider materialized nothing.

The document itself syncs **fine** JS→Rust (this was NOT a samod sync bug — an
early wrong theory). Proof, via `crates/quarto-hub-provider/tests/integration/sync_probe.rs::probe_root_keys`:

```
files=map{index.qmd=Some("Object(Text)")} value=Scalar(Int(42))
```

Fix: `crates/quarto-hub/src/index.rs` reads `Str` **or** `Text` values.

## Pieces

- `js-peer/server.mjs` — a minimal JS `automerge-repo` sync server (a control:
  removes samod-as-server from the equation).
- `js-peer/peer.mjs` — create/read a `{ value, files }` doc.
- `js-peer/create-project.mjs` — create a realistic project (index with a
  `Text`-valued `files` map + a `.qmd` file doc), like hub-client.
- `js-peer/e2e-run.mjs` — full editor-side stand-in: create a project, broadcast
  an `exec/request` (the Run button), and read back + decode the capture.
- `js-peer/dump.mjs` / `js-peer/decode-capture.mjs` — inspect the index /
  decode a capture doc's engine output.
- Rust side: `sync_probe.rs` (ignored, network) + `relay_sync.rs` (self-contained).

## Run the full end-to-end (JS project → Rust provider → executed output)

Needs R (`engine: knitr`) or edit the qmd to `engine: jupyter` + Python.

```bash
# 1. a local storage hub
cargo run --bin q2 -- hub --no-project --data-dir /tmp/hd --port 3055 &

# 2. editor side: create a project + click "Run" (broadcast exec/request), read the capture
node interop-repro/js-peer/e2e-run.mjs ws://127.0.0.1:3055 &
#    → prints INDEX_DOC_ID=<idx>

# 3. the executor (any non-empty --token; the no-auth hub ignores it)
cargo run --bin q2 -- provide-hub --server ws://127.0.0.1:3055 --allow-all --token dev <idx>
```

Observed: the provider lists the file, runs knitr, writes the capture; the JS
side prints `ENGINE=knitr` and `STDOUT=1 2 3` (from `cat(1, 2, 3)`).

> Node scripts must be run from the repo root (ESM resolves the repo's
> `node_modules`).
