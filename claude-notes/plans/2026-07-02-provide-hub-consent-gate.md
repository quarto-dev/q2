# Harden `q2 provide-hub`: interactive consent gate + one-shot default

**Strand:** bd-9lgiulr4 (feature, P1). Discovered from bd-sfet3264.
**Date:** 2026-07-02.
**Status:** IMPLEMENTED on `feature/hub-execution-provider` (2026-07-02).
All phases done; `cargo xtask verify --skip-hub-build` green; E2E-verified
in the terminal + browser (see "End-to-end evidence"). Not pushed yet.
All open questions (Q1–Q7) resolved below.

## Decisions locked (user, 2026-07-02)

- **Q1 — one-shot target: always require `--file <path>`.** No
  "run-the-sole-doc" convenience; the target document is always named
  explicitly. Omitting it is an error.
- **Q2 — retire `--allow-all`.** The interactive consent gate is the
  safety control; `--dangerously-accept-requests` is the explicit
  unattended opt-out. (Phase-5 `ProviderOnly` per-project requester
  gating remains a separate, later concern.)
- **Q4 — beacon + editor Run button are `--watch`-only.** Default
  one-shot broadcasts **no** beacon and does **not** listen on the
  request channel; it is purely operator-initiated.
- **Q5 — non-TTY stdin ⇒ reject (fail safe).** With no interactive
  terminal, refuse to execute and print guidance to pass
  `--dangerously-accept-requests` for unattended use.
- **Q3 — confirmed.** "accept all future" appears only under `--watch`;
  persists for the **process lifetime only** (a fresh invocation always
  re-prompts).
- **Q6 — confirmed.** Reject writes nothing (no capture, no error
  sidecar) + logs; surfacing "rejected by operator" to the requester's
  editor is a follow-up.
- **Q7 — confirmed.** For one-shot flush-before-exit, **try for a real
  synced-with-peer confirmation** from the samod fork; a **bounded
  settle-then-`stop()` workaround is acceptable** if there is no easy
  API. **Report the actual mechanism chosen** when the work lands
  (record it in the Phase 4 notes + tell the user).

## Why

A colleague security review flagged the current `q2 provide-hub
--allow-all` posture: it is an **always-on server that auto-executes any
`exec/request` arriving on the CRDT's ephemeral channel**. If an
attacker hijacks (or spoofs onto) the automerge document, they can drive
arbitrary `{r}`/`{python}` execution on the volunteer's machine with **no
human in the loop**. Running remote code is exactly the operation that
should *not* be fully automated by default.

Two default-behavior changes make the provider safe-by-default:

1. **Interactive consent gate (default ON).** Before any execution, the
   operator is shown the *resolved document that will be evaluated* and
   must affirmatively accept. Bypassable only with an explicit
   `--dangerously-accept-requests`.
2. **One-shot by default (no watching).** The default no longer sits
   online auto-serving requests. It connects ephemerally, pulls the
   project, executes once (still gated by the consent prompt), and exits.
   The persistent "watch the ephemeral channel and serve requests"
   behavior moves behind `--watch`.

## Current behavior (verified 2026-07-02)

`crates/quarto/src/commands/provide_hub.rs` + `crates/quarto-hub-provider/`:

- `q2 provide-hub <project>` → `join()` (ephemeral memory `Repo`, dial
  with `BearerDialer`, `find` the index doc), print the file list.
- **Without `--allow-all`:** fail-closed — print guidance, exit.
- **With `--allow-all`:** build a `Provider`, then `provider.run(ctrl_c)`:
  - `run_beacon_loop` broadcasts an `exec/beacon` every `BEACON_INTERVAL`.
  - `run_request_loop` consumes the index handle's `ephemera()` stream;
    for each `exec/request` it checks `AuthzPolicy` (`AllowAll`/`Deny`),
    claims the path (in-flight dedup), and `spawn_blocking`s
    `execute_document(path)` — **no human confirmation**.
- `execute_document` → `run_and_store`: `materialize_project` to a fresh
  temp dir → `ProjectContext::discover` → **uncached** `record_capture`
  (runs the engine) → gzip+store capture binary doc → `set_capture`
  sidecar. Concurrent requests run concurrently.
- Dev escape hatch `--token <bearer>` uses a `StaticTokenSource` (local
  no-auth hub) instead of the Node OAuth bridge.

Key structural facts that shape the design:

- **The reviewable artifact already exists.**
  `quarto_core::engine::preview_record::compute_input_qmd(path, project,
  runtime)` runs the pipeline up through `PreEngineSugaringStage` and
  serializes the AST to QMD — **the post-include, pre-engine bytes that
  `EngineExecutionStage` hands the engine**, byte-identical to the
  `EngineCapture.input_qmd` field. This is exactly "the same thing we
  would send to knitr." It is already `pub` and the provider crate
  already depends on `quarto-core`.
- **The provider's own stdin is free** for an interactive prompt: the
  Node auth bridge pipes the *Node child's* stdio (`token_bridge.rs`),
  not the provider process's terminal. (`--token` dev mode has no child
  at all.)
- **samod exposes `Repo::stop() -> impl Future`** which "wait[s] until
  all storage tasks have completed" — but does **not** documentedly
  guarantee outbound *network* sync has been acked by the server. The
  existing execute integration test confirms sync with sleep-based
  polling. One-shot mode needs a real flush-before-exit story (see
  Technical risk R1).

## Target behavior

### Modes

| Invocation | Behavior |
|---|---|
| `q2 provide-hub <project> --file <path>` (default) | **One-shot.** Connect (ephemeral) → pull docs → for the **required `--file`** doc: materialize, synthesize the review file, **prompt** (accept/reject), execute on accept, write capture, **flush sync** → shut down. No beacon, no request channel. |
| `q2 provide-hub <project> --watch` | **Watch.** Stay online, broadcast the beacon, serve `exec/request`s — each execution **prompt-gated** (accept/reject/accept-all) unless `--dangerously-accept-requests`. Runs until Ctrl-C. |

### Flags

- `--file <path>` — **required in one-shot (default) mode**; the project-
  relative document to execute once. (Ignored/irrelevant under `--watch`,
  where the requested path comes from the `exec/request`.)
- `--watch` — opt into the persistent request-serving loop (the old
  default-with-`--allow-all` behavior, now consent-gated).
- `--dangerously-accept-requests` — skip the interactive prompt
  (auto-accept every execution). The name is deliberately alarming.
  This is the **only** way to get fully-unattended execution.
- `--token <bearer>` — unchanged dev hatch.
- `--server <url>` — unchanged.
- **`--allow-all` is removed** (Q2). Its fail-closed role is superseded by
  the consent gate; unattended runs use `--dangerously-accept-requests`.

### The consent prompt

Before executing, from the **already-materialized** snapshot, synthesize
the post-include QMD to a reviewable temp file and print:

```
An execution request has arrived for "<rel-path>".
The resolved document to be evaluated is at:
    /tmp/quarto-provide-hub-XXXX/<rel-path>.resolved.qmd
Review it, then choose:
  1) accept
  2) reject
  3) accept this and all future requests   [watch mode only]
>
```

- **accept** → execute this one.
- **reject** → do not execute (write nothing); one-shot exits, watch
  continues.
- **accept all future** → execute this and set a session flag so
  subsequent requests skip the prompt (watch mode; see Q3).

### The review-equals-execute guarantee (core security property)

The bytes the operator reviews **must** be the bytes that execute. So:

1. **Materialize once** to a temp dir (no re-pull between review and
   execution).
2. Compute the review artifact via `compute_input_qmd` **from that same
   materialized snapshot**.
3. On accept, run `record_capture` **against the same temp dir** — its
   internal pre-engine pipeline re-derives identical post-include bytes
   from identical on-disk inputs (deterministic), so what the engine
   receives equals what was shown. No second `find`/pull can smuggle in
   different content after approval.

(Multi-engine docs: `compute_input_qmd` serializes the whole document,
covering every code cell regardless of engine — one review file per doc.
Noted; the review file is complete.)

## Design

### 1. A `ConsentGate` seam in `quarto-hub-provider`

Introduce a trait the execute path consults before running:

```rust
pub enum ConsentDecision { Accept, Reject, AcceptAll }

#[async_trait(?Send)]
pub trait ConsentGate {
    /// Called after the review file is written, before the engine runs.
    async fn review(&self, path: &str, review_file: &Path) -> ConsentDecision;
}
```

- **`InteractivePrompt`** (CLI default): prints the prompt, reads a line
  from stdin on a blocking thread (`spawn_blocking` / a dedicated stdin
  reader), maps 1/2/3. Serialized — only one prompt at a time.
- **`AlwaysAccept`** (`--dangerously-accept-requests`, and tests): returns
  `Accept` without prompting.
- The `AcceptAll` decision flips provider-held state so future `review`
  calls short-circuit to `Accept` (watch mode).

This keeps `Provider` testable (inject `AlwaysAccept`/`AlwaysReject`) and
keeps stdio out of the core loop.

### 2. Refactor `execute.rs` to split materialize → review → execute

`run_and_store` currently materializes *inside itself*. Split so the
provider:

1. materializes once (`materialize_project`),
2. `discover`s the project,
3. `compute_input_qmd` → write `<review_dir>/<rel>.resolved.qmd`,
4. `consent.review(path, review_file)` → decision,
5. on `Accept`/`AcceptAll`: `record_capture` against the same temp dir →
   write capture doc → `set_capture`,
6. on `Reject`: skip (no capture; optional `tracing::info!`).

The running/error sidecar status handling stays as-is around step 5.

### 3. `provide_hub.rs`: one-shot vs watch

- Parse `--file`, `--watch`, `--dangerously-accept-requests`; remove
  `--allow-all`.
- Build the `ConsentGate`: `InteractivePrompt` unless
  `--dangerously-accept-requests` (→ `AlwaysAccept`). If interactive but
  stdin is **not a TTY**, refuse (Q5) — construct a gate that rejects and
  prints the `--dangerously-accept-requests` guidance, or fail before
  connecting.
- **One-shot (default):** require `--file`; after `join`, validate the
  path is in the index, run the execute path **once** for it with the
  gate (accept/reject only — no "accept all"), then **flush and `stop()`**
  (R1) and exit. No beacon, no request loop.
- **Watch:** `Provider::run` as today, but the request loop calls the
  gate before executing (accept/reject/accept-all), prompts serialized
  (one at a time). Beacon + request channel active.

### 4. Tests (TDD)

- `ConsentGate` unit tests: prompt parsing (`1`/`2`/`3`, junk → reject or
  re-ask), `AcceptAll` state flip.
- `execute.rs`: `AlwaysReject` writes **no** capture; `AlwaysAccept`
  writes one (port the existing integration test to inject a gate).
- New integration test: one-shot path executes exactly once and the
  capture syncs to the server, then the provider stops cleanly.
- Existing execute integration tests updated to pass a gate.
- CLI: `--watch` / `--dangerously-accept-requests` parsing; help renders.
- E2E (manual, documented): local harness — one-shot with a real
  accept/reject at the terminal; `--watch` + interactive accept.

## Technical risks

- **R1 — Flush-before-exit (the hard one).** One-shot must not exit
  before the capture doc **and** the index sidecar update have reached
  the server, or collaborators never see the output. `Repo::stop()`
  drains storage but does not documentedly confirm network sync. Need to
  investigate the samod fork (0.12.1 `access-policy`) for a
  document/connection-level "synced with peer" signal; if none exists,
  fall back to a **bounded wait** (poll/settle then `stop()`), and
  document the residual race. This is the main implementation unknown.
- **R2 — stdin + async runtime.** Reading a line interactively must not
  block the reactor; use a dedicated blocking read. In watch mode,
  prompts must be strictly serialized (no two questions at once) — so
  watch-mode execution becomes effectively one-at-a-time while a prompt
  is open. Acceptable given a human is the bottleneck anyway.
- **R3 — Non-interactive stdin.** If the default (one-shot, interactive)
  runs with no TTY (CI, piped), the prompt would hang or EOF. Decide:
  EOF/`no-TTY` → treat as **reject** (safe) and print guidance to use
  `--dangerously-accept-requests`. (Proposed; see Q5.)

## Open questions

**Resolved 2026-07-02** (see "Decisions locked" at top): Q1 (require
`--file`), Q2 (retire `--allow-all`), Q4 (beacon/Run are `--watch`-only),
Q5 (non-TTY ⇒ reject).

**Still open (carrying proposed defaults — confirm or correct during
iteration):**

**Q3 — "Accept all future" scope.** Proposed: single-doc one-shot shows
only accept/reject; `--watch` shows all three; "accept all" persists for
the process lifetime only. (No cross-run persistence — a fresh
invocation always re-prompts.)

**Q6 — Reject side effects.** Proposed v1: reject writes nothing (no
capture, no error sidecar) + logs. Surfacing "rejected by operator" to
the requester's editor (a new `CaptureState`/message) is a follow-up.

**Q7 (new, technical) — R1 flush guarantee.** If the samod fork has no
"synced-with-peer" confirmation, is a **bounded settle-then-`stop()`**
acceptable for v1 one-shot (with the residual race documented), or is a
hard delivery guarantee required before we ship one-shot? (Answerable
once R1 is investigated in Phase 4.)

## Phase checklist (TDD)

### Phase 1 — `ConsentGate` seam + review artifact (provider crate)
- [x] **1A** `consent.rs`: `ConsentDecision { Accept, Reject, AcceptAll }`
      + `#[async_trait(?Send)] ConsentGate { async fn review(&self, path,
      review_file) -> ConsentDecision }`. Impls: `AlwaysAccept`,
      `AlwaysReject` (test/`--dangerously-accept-requests`), and
      `parse_prompt_line(&str) -> Option<ConsentDecision>` pure helper.
      Unit tests: `1`/`2`/`3` → Accept/Reject/AcceptAll; junk/empty →
      None (re-ask). RED→GREEN.
- [x] **1B** review-file synthesis: a helper that, given the materialized
      temp dir + rel path, calls `compute_input_qmd` and writes
      `<review_dir>/<rel>.resolved.qmd`, returning the path. Unit/integration
      test: the written bytes equal `compute_input_qmd` for the doc.

### Phase 2 — refactor `execute.rs` to materialize → review → consent → execute
- [x] **2A** Split `run_and_store`: materialize once; `discover`; write the
      review file; call `gate.review(...)`; on Accept/AcceptAll →
      `record_capture` against the same temp dir → write doc → set sidecar;
      on Reject → return an `Rejected` outcome (no capture).
- [x] **2B** Thread a `Arc<dyn ConsentGate>` into `Provider` (field) and/or
      `execute_document`. `AcceptAll` flips a provider `AtomicBool` so later
      `review` calls short-circuit (watch mode).
- [x] **2C** Port existing execute unit/integration tests to inject a gate;
      add `AlwaysReject` → no capture written (assert over a broadcast
      window, mirroring the `Deny` test). RED→GREEN.

### Phase 3 — CLI: one-shot vs watch (`provide_hub.rs`)
- [x] **3A** Add `--file`, `--watch`, `--dangerously-accept-requests`;
      remove `--allow-all` (+ its `AuthzPolicy` plumbing, or keep `Deny`
      unused — decide minimal). Build the gate: `AlwaysAccept` under
      `--dangerously-accept-requests`; else `InteractivePrompt`; if
      interactive and `!stdin.is_terminal()` → refuse (Q5).
- [x] **3B** One-shot control flow: require `--file`; validate in index;
      execute once with the gate; then flush + stop (Phase 4); exit.
- [x] **3C** Watch control flow: `Provider::run` with the gate; beacon +
      request loop; prompts serialized. Unit tests: flag parsing; one-shot
      requires `--file`; help renders.

### Phase 4 — one-shot flush-before-exit (R1)
- [x] **4A** Investigate the samod fork (0.12.1 `access-policy`) for a
      doc/connection-level "synced with peer" signal (`DocHandle` sync
      state, connection acks). Prefer a real wait.
- [x] **4B** Implement the best available: real confirmation if present,
      else a bounded settle-then-`stop()` with the residual race
      documented. **Record which was chosen here + tell the user.**
- [x] **4C** Integration test: one-shot executes once, the capture syncs
      to a server peer, provider stops cleanly (extend the execute
      harness with a second in-process peer that observes the capture).

### Phase 5 — docs, verify, E2E
- [x] **5A** Updated the clap help (`main.rs`) + the e2e harness README
      (one-shot `--file` default; `--watch`; consent prompt;
      `--dangerously-accept-requests`; `--allow-all` gone).
- [x] **5B** `cargo xtask verify --skip-hub-build` green (workspace build +
      tests + clippy + q2-preview-spa build). hub-client untouched.
- [x] **5C** E2E on the local harness (see below) — all five CLI scenarios
      + browser confirmation.
- [x] **5D** No hub-client changelog (CLI-only change; no hub-client UI
      strings touched).

## End-to-end evidence (2026-07-02)

Local Option-B harness (no-auth `q2 hub` + `--token dev`), engine `knitr`
(`r-demo.qmd`). All observed directly:

| # | Invocation (abridged) | Result |
|---|---|---|
| A | `provide-hub --token dev <id>` (no `--file`) | Errors before connecting: "one-shot mode requires --file". |
| B | `provide-hub --file r-demo.qmd … < /dev/null` (non-TTY) | Connects, then **refuses** (fail-safe): "executions will be REFUSED"; "Execution declined; nothing was run." No engine ran. |
| C | `provide-hub --file r-demo.qmd --dangerously-accept-requests …` | Warns, runs knitr, "Pushing the result to the hub… Done." Exits in **0.37 s** (flush confirmed, no hang). |
| D | `provide-hub --file r-demo.qmd …` driven over a **pty**, answer `1` | Prompt shows the resolved-document path + only `1) accept` / `2) reject` (no option 3). `1` → executes + flushes. |
| E | same, answer `2` | `2` → "Execution declined; nothing was run." No engine ran. |
| watch | `provide-hub --watch --dangerously-accept-requests …` | Stays online, "Watching for execution requests…"; the editor's status bar lights up (green dot + Re-run) — beacon works. |

Browser confirmation (after C): a collaborator's hub-client preview shows
the executed R output spliced in (`[1] 2`, the R version string, `[1] 55`)
with the status bar reading "Showing executed output" + "Clear results…"
and **no live-executor dot/Run** — exactly the one-shot model (the provider
pushed results and left). This also closes bd-gthycd33's knitr path.

The true interactive TTY prompt was driven with a Python `pty` harness
(scenarios D/E) since the sandbox shell has no controlling terminal; the
prompt's parse/loop logic is additionally unit-tested in `consent.rs`.

### R1 flush mechanism actually implemented — REAL confirmation (2026-07-02)

**A hard delivery confirmation was available in the samod fork, so no
sleep-based workaround was needed.** `Provider::flush_to_hub`:

1. Reads the one hub connection id from `index.handle().peers()`.
2. Awaits `DocHandle::they_have_our_changes(conn)` on the **index handle**
   (the peer has our `CaptureRef` sidecar update) — this future resolves
   once the peer's `shared_heads` equal our local heads (a real ack, not
   a timer).
3. Then `repo.find(capture_doc_id)` and awaits `they_have_our_changes`
   on the **capture binary doc** handle (the peer has the output bytes).
4. The whole confirmation is wrapped in a **15 s `tokio::time::timeout`**
   as a pure safety net for a slow/dropped link; on timeout it logs and
   returns so one-shot still exits. In practice it resolves in
   milliseconds (the integration test completes in ~0.02 s).

So the guarantee is: one-shot does not exit until the hub has
acknowledged **both** the sidecar pointer and the capture doc — exactly
the "wait for a real result" the user preferred. The timeout is the
documented fallback, effectively never hit on a healthy connection.

## References

- Provider: `crates/quarto-hub-provider/src/{execute,provide? ,join,materialize}.rs`
- CLI: `crates/quarto/src/commands/provide_hub.rs`
- Review artifact: `crates/quarto-core/src/engine/preview_record.rs`
  (`compute_input_qmd`)
- Parent epic: `claude-notes/plans/2026-06-29-remote-execution-provider.md`
  (esp. D4 authz, Phase 4a mechanism-first/fail-closed)
- Local e2e harness: `claude-notes/hub-execution-e2e/`
