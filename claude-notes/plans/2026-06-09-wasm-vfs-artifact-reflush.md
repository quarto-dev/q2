# Perf: WASM render re-flushes ALL artifacts into the VFS on every render (bd-q3bxnq2e)

**Date:** 2026-06-09
**Beads:** bd-q3bxnq2e
**Worktree:** main checkout (branch `main`, based on `main` @ `ade34bed`)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design.** The code matches the strand's description, the blast-radius
question (Automerge propagation) is now answered — artifacts are purely
in-memory-ephemeral — and the fix direction is clear and small. The main open
work is quantification (per the performance-profiling playbook, measure before
fixing) and a handful of scoping decisions listed below.

## Issue context

Filed 2026-06-09 (same day, fresh) by Carlos, priority 1, type task, label
`revealjs`. Discovered while scoping the embed-iframe preview work
(bd-z1smhvuo). Claim: the WASM render entry point re-flushes the **entire**
artifact set into the VFS on **every** render with no change detection —
`artifact.content.clone()` + HashMap insert per artifact per render — so every
edit-triggered re-render pays CPU/alloc cost proportional to *total artifact
bytes*, not *changed bytes*. Strand asks to (1) quantify, (2) pin down whether
the writes propagate into Automerge, (3) design a change-detection fix,
(4) preserve the bd-3gtn empty-content skip and the iframe post-processor's
read-back contract.

## Dependency graph

- **discovered-from: bd-z1smhvuo** (Embed mechanism for
  `.embed-example-iframe` doc placeholders, in_progress). Phases 1–2 of that
  feature landed (commits `de20ca96`, `867aa7c1`); its remaining item is
  verifying `q2 preview` serves staged static assets via the VFS. This strand
  surfaced while reading that VFS path. The `revealjs` label is inherited from
  that context — **reveal.js itself is not implicated** (its assets are
  inlined into the HTML via `include_str!`, see `crates/quarto-core/src/revealjs/assemble.rs:26-32`;
  they never become artifacts).
- No blockers, no dependents. Fresh strand, no staleness concerns.

## What the code looks like today

All paths verified at `main` @ `ade34bed`. Pre-flight
`cargo xtask verify --skip-hub-build` is green.

### Three flush sites, all unconditional

1. **Single-doc render tail** — `crates/wasm-quarto-hub-client/src/lib.rs:1417-1425`
   (exactly as the strand quotes). Used by `render_qmd` /
   `render_qmd_content` and by `render_page_in_project`'s no-project
   fall-through. Flushes **all** of `ctx.artifacts` (page- and project-scope;
   the single-doc path never drains): one `content.clone()` + insert per
   artifact per render.

2. **Project render, page-scoped artifacts** —
   `crates/wasm-quarto-hub-client/src/lib.rs:1626-1643`. Same loop over
   `active_output.page_artifacts` (engine figures, resource copies for the
   active page).

3. **Project render, project-scoped artifacts** —
   `WebsiteProjectType::post_render` → `flush_site_libs`
   (`crates/quarto-core/src/project/website_post_render.rs:81-109`), which runs on
   **every** `ProjectPipeline::run` (orchestrator.rs: Pass 1 → pre_render →
   Pass 2 → post_render — i.e., every preview render). Each artifact is cloned
   **twice**: `sink.write(on_disk, artifact.content.clone())` into the
   `OutputSink`, then `sink.flush(runtime)` → `WasmRuntime::file_write` →
   `contents.to_vec()` (`crates/quarto-system-runtime/src/wasm.rs:596-598`).

   (The `RenderToHtmlRenderer` default-project branch at
   `pass2_renderer.rs:826-838` routes through the same `flush_site_libs`.)

### Upstream of the flush: producers also rebuild the bytes per render

The flush is the *second* per-render copy of mostly-static bytes. The
producers re-store them into `ctx.artifacts` every render:

| Artifact | Producer | Size | Per-render source |
| --- | --- | ---: | --- |
| Theme CSS `quarto/quarto-theme-<fp>.css` | `compile_theme_css.rs` | ~200–400 KB (Bootstrap-based) | SASS LRU cache hit → clone of cached bytes (compile itself **is** cached) |
| `bootstrap.bundle.min.js` | `bootstrap_js.rs:77` | 81 KB | `include_bytes!` static → `.to_vec()` |
| bootstrap-icons CSS | `website_bootstrap_icons.rs:37` | 99 KB | `include_bytes!` static → `.to_vec()` |
| bootstrap-icons woff | `website_bootstrap_icons.rs:41` | 180 KB | `include_bytes!` static → `.to_vec()` |
| clipboard JS ×2 | `clipboard_js.rs:71,81` | ~10 KB | `include_bytes!` static → `.to_vec()` |
| listing JS/CSS | `listing_render.rs:153-159` | small | per render |
| plot images, resource copies | engines / `ResourceCollectorTransform` | unbounded | page-scoped |

So a theme-heavy website page costs roughly **0.6–1 MB of byte cloning ×2–3
copies per keystroke render**, plus HashMap/PathBuf churn, regardless of
whether anything changed. On top of that, the TS side re-reads and re-hashes
CSS from the VFS per render for `cssVersion`
(`ts-packages/preview-runtime/src/wasmRenderer.ts:1091-1099`).

### Strand question 2 — Automerge blast radius: ANSWERED, benign

The VFS writes are **purely in-memory-ephemeral**. Evidence (Explore-agent
sweep, 2026-06-09):

- Sync is strictly one-way Automerge → VFS
  (`ts-packages/preview-runtime/src/automergeSync.ts:88-108`: `onFileAdded` /
  `onFileChanged` / `onBinaryChanged` / `onFileRemoved` all call `vfsAddFile`-family;
  no reverse callback exists).
- `WasmRuntime`'s VFS is a `HashMap<PathBuf, Vec<u8>>` behind an `RwLock`
  (`crates/quarto-system-runtime/src/wasm.rs:229`), no persistence hooks.
- The only VFS artifact read-backs are the iframe post-processor
  (`hub-client/src/components/render/ReactAstSlideRenderer.tsx:770-775`, →
  data: URIs) and the `cssVersion` hash above. Neither writes to Automerge or
  IndexedDB. IndexedDB holds only project metadata, session state, and the
  Pass-1 profile cache.

So the cost is **CPU/alloc only** — no document growth, no sync traffic. This
caps the severity: the "much worse" branch of the strand did not materialize.

### Severity caveat (why we measure before fixing)

Memcpy of ~1–2 MB is sub-millisecond native and low-single-digit ms in WASM.
That is real per-keystroke waste but plausibly **not** the dominant preview
cost (each render also re-runs the whole project pipeline). Per
`claude-notes/instructions/performance-profiling.md`, Phase 1 quantifies with
a scaled fixture *before* the fix is designed in detail; if the flush turns
out to be <~5% of per-render time, we report that honestly and still decide
(question 1 below) whether the cheap fix is worth landing.

## Proposed fix direction (draft, pending measurements)

**Skip-if-byte-equal at the VFS boundary.** Add a compare-before-clone write,
e.g. `VirtualFileSystem::add_file_if_changed(&Path, &[u8]) -> bool` (and a
`WasmRuntime` passthrough), then route all three flush sites through it:

- Equal content → memcmp only (cheap, no alloc, no insert churn).
- Changed/new content → clone + insert exactly as today.

Properties: generic across artifact types (fingerprinted or not), no new
state to invalidate, preserves the bd-3gtn empty-content skip untouched, and
trivially preserves the iframe read-back contract (bytes at the path are
always present and current — we only skip writes that would be byte-identical
no-ops). The theme artifact's fingerprinted filename would make an O(1)
presence check possible, but it covers only one artifact; byte-compare covers
all of them at negligible extra cost.

Alternatives considered and deprioritized:
- **Flush-once-per-session epochs** for project-scope artifacts: artifacts
  *can* legitimately change mid-session (`_quarto.yml` theme edit), so this
  still needs change detection — it collapses into the same mechanism with
  more bookkeeping.
- **Content-hash registry**: avoids O(n) compare but adds state and hashing;
  memcmp on equal bytes is already ~as fast as hashing one side.

## Proposed phases (draft)

Skeleton only — contents wait on the design discussion.

- **Phase 0 — Quantify (playbook steps 1–4).**
  - [x] Un-gate `VirtualFileSystem` for native builds (decision 2): moved to
        target-agnostic `quarto-system-runtime/src/vfs.rs`; `WasmRuntime`
        stays wasm-only. The 7 `test_vfs_*` unit tests now run natively
        (they were dead under the wasm gate). Native + wasm32 both compile
        clean; workspace tests green.
  - [x] `perf-harness` driver `vfs-flush` (`crates/perf-harness/src/bin/vfs_flush.rs`):
        renders the committed theme-heavy fixture
        (`claude-notes/plans/wasm-vfs-artifact-reflush-investigation/theme-heavy.qmd`)
        in a loop against a session-persistent `VirtualFileSystem`,
        mirroring the wasm `render_qmd` tail byte-for-byte (incl. the
        bd-3gtn skip); `pad_bytes` arg scales total artifact bytes.
        Functional check: every iteration re-flushes 4 artifacts /
        400,692 B; flush ≈10 µs vs render ≈50 ms native (PRELIMINARY —
        busy machine, not the recorded numbers).
  - [x] Instrumentation (QUARTO_PERF_STATS=1, playbook conventions):
        gauge `perf.vfs-write` — counters on `VirtualFileSystem`
        (writes/bytes_written/skipped_writes/bytes_skipped; skip counters
        wired now, stay zero until Phase 2, so before/after share one
        format; `write_stats()` accessor for per-render diffing) — and
        gauge `perf.artifact-store` — producer-side counters on
        `ArtifactStore::store()` (stores/bytes_stored; drain/merge moves
        not counted) so bd-w5qyuzeg inherits real numbers. Smoke-tested:
        one themed render stores 4 artifacts / 400,692 bytes
        (`perf.artifact-store stores=4 bytes_stored=400692`).
        **Trimmed before merge — see "Instrumentation trim" below.**

### Instrumentation trim (2026-06-09, pre-merge)

The `perf.artifact-store` gauge was **removed before merging to main**.
Rationale: unlike the bd-h5l7-convention counters (plain fields, truly
free), this gauge required a `Drop` impl on `ArtifactStore`, and Rust
forbids moving fields out of `Drop` types — forcing a non-obvious
`mem::take` workaround inside `merge_into_project`. That is a permanent
complexity tax on a core type for a diagnostic whose purpose is already
served: the producer-side numbers bd-w5qyuzeg needed are measured,
recorded in the Findings table above, and noted on that strand. The
`vfs-flush` driver now computes `producer_bytes` by summing over the
store directly, so the measurement remains reproducible without the
core-type counters. The `perf.vfs-write` gauge on `VirtualFileSystem`
**stays**: it has no `Drop`-related cost on hot types beyond the VFS
itself, and its counters are what the change-detection tests assert
against.
  - [x] Geometric scaling + before/after run on an idle machine
        (2026-06-09, see Findings below).

## Findings (recorded 2026-06-09, idle machine)

Driver: `target/release/vfs-flush theme-heavy.qmd 12 <pad> <mode>`, 2
modes × 6 pad sizes × 12 iterations; steady-state = median of
iterations 1–11 (iteration 0 is cold: SASS compile + first writes).
Raw output preserved at
`claude-notes/plans/wasm-vfs-artifact-reflush-investigation/measurements-2026-06-09.txt`.

| mode | total artifact B | flush µs (median) | bytes written | bytes skipped | render ms |
|---|---:|---:|---:|---:|---:|
| legacy | 400,692 | 10.6 | 400,692 | 0 | 50.6 |
| legacy | 810,292 | 17.0 | 810,292 | 0 | 52.1 |
| legacy | 1,219,892 | 22.9 | 1,219,892 | 0 | 52.6 |
| legacy | 2,039,092 | 32.8 | 2,039,092 | 0 | 51.3 |
| legacy | 3,677,492 | 54.3 | 3,677,492 | 0 | 51.3 |
| legacy | 6,954,292 | 97.5 | 6,954,292 | 0 | 51.1 |
| skip | 400,692 | 11.4 | 0 | 400,692 | 52.3 |
| skip | 810,292 | 18.9 | 0 | 810,292 | 51.6 |
| skip | 1,219,892 | 26.1 | 0 | 1,219,892 | 51.6 |
| skip | 2,039,092 | 40.8 | 0 | 2,039,092 | 52.6 |
| skip | 3,677,492 | 69.8 | 0 | 3,677,492 | 51.3* |
| skip | 6,954,292 | 131.2 | 0 | 6,954,292 | 52.8 |

(*50.6 measured; table shows medians of independent runs — render time
is flat ~51 ms throughout, as expected.)

Conclusions:

1. **Complexity class confirmed: the flush is linear in total artifact
   bytes** (both modes; ratios track byte ratios to within noise).
   No accidental quadratic behavior.
2. **The flush was never a meaningful native cost.** At the realistic
   400 KB themed-doc size it is ~11 µs against a ~51 ms render —
   **0.02 %**. Even inflated to 7 MB of artifacts it is ~0.1–0.26 %.
   The per-keystroke latency lives in the render itself, not the flush.
   The strand's "suspected to contribute to observed preview perf
   issues" is **not supported** for the native proxy; any WASM-side
   contribution would have to come from allocation pressure, not CPU.
3. **The skip's native win is allocation elimination, not wall time.**
   memcmp over equal buffers reads both sides to the end, so skip-mode
   flush wall time is slightly *higher* than legacy clone+insert
   natively (131 vs 98 µs at 7 MB) — while writing **zero** bytes
   (no Vec allocations, no old-buffer frees, no heap growth). In the
   WASM/browser environment allocation churn is relatively more
   expensive (wasm memory only grows; GC pressure on the JS boundary),
   so zero-allocation steady state is the right trade — and per
   decision 1 the fix lands regardless, with this share stated
   honestly.
4. **Producer-side cost (bd-w5qyuzeg) is the same magnitude** —
   `producer_bytes` equals flushed bytes (~400 KB/render realistic),
   i.e. another ~10–100 µs/render of memcpy natively. Recommendation
   recorded on the strand: deprioritize to backlog unless a WASM
   browser profile shows allocation pressure mattering.
- **Phase 1 — Test plan (TDD).** Committed red at `f2e328e8`; all five
  skip-behavior tests verified failing against an always-write stub.
  - [x] Unit tests for `add_file_if_changed` semantics (new / changed /
        identical / empty / path-normalization) in
        `quarto-system-runtime/src/vfs.rs`.
  - [x] Flush-level tests in new `quarto-core/src/artifact_flush.rs`:
        second flush of an unchanged store skips all writes
        (counter-observable); changed artifact re-written while
        unchanged siblings skip; bd-3gtn empty-content skip; pathless
        skip; resolver-path read-back.
  - [x] Read-back regression guard after a skipped flush (unit level +
        WASM e2e, see Phase 2).
- **Phase 2 — Implement.**
  - [x] `VirtualFileSystem::add_file_if_changed(&Path, &[u8]) -> bool` —
        memcmp against the existing entry; clone+insert only on change;
        skips counted in `skipped_writes`/`bytes_skipped`.
  - [x] Sites 1 & 2 (`wasm-quarto-hub-client/src/lib.rs`,
        `render_single_doc_to_response` +
        `render_project_active_page_to_response`): inline loops replaced
        by shared `quarto_core::flush_artifacts_to_vfs` via new
        `WasmRuntime::with_vfs_mut` — single code path with the native
        proxy and unit tests.
  - [x] Site 3: `WasmRuntime::file_write` routes through
        `add_file_if_changed` (decision 3 — in-memory VFS layer only;
        `flush_site_libs`/`OutputSink` and native disk writes
        unchanged).
  - [x] Driver `--mode legacy|skip` flag for single-session
        before/after timing. Functional check: skip-mode iteration 0
        writes 4 artifacts / 400,692 B, iterations 1+ skip all
        (`skipped_writes=4 bytes_skipped=400692`).
  - [x] WASM e2e regression tests
        (`hub-client/src/services/vfsArtifactReflush.wasm.test.ts`,
        runs under `npm run test:wasm`): steady-state double render →
        identical HTML + CSS artifact readable byte-identical from VFS;
        keystroke edit → new HTML, CSS artifact intact. Full WASM suite
        81/81 green against the rebuilt module.
  - Environment discovery: in Node/vitest the bootswatch theme sources
    are not in the VFS, so `theme:` docs fall back to the default CSS
    bundle at `/.quarto/project-artifacts/styles.css` — relevant nuance
    for bd-rrnn3se8 (the styles.css read is *correct* in the fallback
    case; `theme_fingerprint` is still present and is still the better
    signal). Also discovered `hub-client/test-wasm.mjs` is broken in
    bare Node (raw_module imports need Vite aliasing) → filed
    bd-ye90x1ga.
- **Phase 3 — Verify.** All complete (2026-06-09).
  - [x] Native before/after numbers across scales — see Findings table
        above (linear confirmed; skip mode writes zero bytes in steady
        state).
  - [x] Full `cargo xtask verify` (Rust build + tests, hub-client
        `build:all` incl. WASM rebuild, hub-client `test:ci`): exit 0.
  - [x] Browser cross-check (playbook step 8) — see end-to-end record
        below.

## End-to-end verification record (2026-06-09)

Per CLAUDE.md § End-to-end verification. Exercised through the real
`q2` binary + a real Chrome session, after rebuilding the full WASM
chain (`npm run build:wasm` → `cargo xtask build-q2-preview-spa` →
`cargo build --bin q2`, per the preview-spa-rebuild instructions):

- **Invocation:** `cargo run --bin q2 -- preview tmp-preview-check/doc.qmd`
  (doc = the committed theme-heavy fixture, `theme: cosmo`), then
  loaded `http://127.0.0.1:56232/` in Chrome via the devtools MCP.
- **Observed (initial render, inspected via script evaluation in the
  preview iframe):** heading "Theme-heavy flush fixture (bd-q3bxnq2e)"
  rendered; `getComputedStyle(body).fontFamily` = `"Source Sans Pro", …`
  — the **cosmo** theme font, proving theme SCSS compiled in-browser
  and the theme CSS artifact was read back from the VFS through the
  new flush path (browser env compiles themes, unlike the Node/vitest
  fallback).
- **Observed (keystroke steady state):** appended a `## Live edit
  marker bd-q3bxnq2e` section to the file on disk; the preview
  re-rendered (second flush — unchanged theme CSS now *skipped*),
  new heading present in the iframe, theme font still applied.
- **Console:** no errors; only the pre-existing iframe sandbox warning.
- The output was inspected directly (iframe DOM + computed styles),
  not inferred from absence of errors.

## Design decisions (settled with user, 2026-06-09)

1. **Measure-then-fix gate: land the skip regardless of measured share**,
   with the measured share stated honestly in Findings — *unless* the fix
   turns out to require an architectural change that could paint us into a
   future corner. (`add_file_if_changed` as drafted is not architectural; if
   Phase 0 pushes us toward something bigger, stop and re-align.)
2. **Native proxy: yes**, un-gate `VirtualFileSystem` (not `WasmRuntime`) for
   native builds so the perf-harness driver exercises the actual flush code.
   Framing note from the user: the perf concern is **entirely hub-client
   per-keystroke latency feel** — native builds are ~40–50× faster than
   Quarto 1 and not a worry. The native proxy exists to measure/iterate, not
   because native has a problem to fix.
3. **Scope of site 3: change detection only in the in-memory VFS layer**;
   native disk writes unchanged. User noted a slight preference for single
   code paths but accepts this as one of the unavoidable native-vs-wasm cfg
   splits.
4. **Producer-side clones: out of scope, filed as bd-w5qyuzeg**
   (discovered-from this strand). This strand stays flush-only. The
   `Artifact.content: Vec<u8>` → `Cow<'static, [u8]>` / `Arc<[u8]>` /
   `bytes::Bytes` refactor is gated on Phase 0's data: the instrumentation
   here will measure producer-clone cost alongside flush cost, and
   bd-w5qyuzeg is picked up only if the residual copy is a meaningful share
   of per-keystroke latency.
5. **TS-side `cssVersion`: filed as bd-rrnn3se8** (discovered-from this
   strand) — switch `cssVersion` to `RenderResponse.theme_fingerprint`,
   delete the per-render VFS read + hash.

All five design questions are now settled. Next step: Phase 0
(instrumentation + native proxy + measurements), on user go-ahead.

## Risks / tradeoffs (draft)

- **Phase-9 VFS contract** (`crates/wasm-quarto-hub-client/CLAUDE.md`): the VFS
  is load-bearing across renders; skipping byte-identical writes is contract-
  preserving by construction, but any test that asserts "write happened"
  (rather than "bytes present") could need updating.
- **bd-3gtn**: the empty-content skip must stay ahead of the new compare —
  empty content means "manifest entry, never write", not "write empty bytes".
- **`OutputSink` allowed-roots validation** (bd-cfl67) runs at `sink.write`
  time; if site 3 short-circuits before the sink, we must not lose the
  validation for the artifacts that *do* get written.
- The flush may turn out to be a minor contributor to the observed preview
  slowness — in that case the bigger fish (full pipeline re-run per keystroke)
  belongs in a separate strand; this plan deliberately does not grow to cover it.
