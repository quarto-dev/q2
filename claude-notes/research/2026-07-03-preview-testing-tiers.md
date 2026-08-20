# Why q2-preview features test green but arrive broken — test-tier analysis + recommendations

**Date:** 2026-07-03
**Context:** written at the end of the bd-h4rhohhy preview-capture-delivery work
(plan: `claude-notes/plans/2026-07-02-preview-capture-delivery.md`), which is the
best specimen we have of the recurring pain: every tier green, `q2 preview` shows no
engine results, and only a playwright test forced the feature to actually work.
Evidence base: a full read-the-tests survey of every tier on branch
`braid/bd-h4rhohhy-q2-preview-engine-capture` (agent-produced; key findings inlined
below with file:line citations).

**Question answered:** (1) what lower-tier testing can obviate playwright, (2) what
non-"nuclear" tiers exist in between, (3) which gaps genuinely have no substitute.

## 0. Reference bug classes (all shipped green through every tier below playwright)

- **(a)** abandoned-busy julia worker → oneShot close fails → whole capture discarded
- **(b)** test fixture emitting output shaped differently from real engines (echo's
  bare paragraph vs the `::: {.cell}` wrapper the splice requires)
- **(c)** child-process stdout leaking into the engine-host's JSON framing channel;
  one bad line killed the whole host + every in-flight capture
- **(d)** stale embedded artifacts (q2 embeds `q2-preview-spa/dist/` via
  `include_dir!`; dist bundles a separately-built WASM) — also the 2026-05-20 incident

## 1. The diagnosis: coverage clusters at the ends of a 14-link chain

The chain from CLI parse to pixels (L1..L14; full map in §5) has nine test tiers,
but they cluster at the bottom (library functions with in-process engine doubles)
and the very top (full playwright). The survey's central findings:

1. **Links covered ONLY by playwright today: L9 (real capture sync round trip),
   L10 (React state from real events), L11 (`render_page_for_preview` WASM glue),
   L13 (pane iframe DOM).** Of these, only **L13 is structurally browser-bound**.
   The rest are incidental gaps — the machinery to test them headless already
   exists in-repo (see §3).
2. **L14 — embedded-artifact freshness — is covered by NO tier at all**, and
   `cargo xtask verify --e2e` *guarantees* one-generation staleness: step 3 builds
   the q2 binary (embedding whatever dist exists then), step 13 rebuilds the SPA
   dist, step 14 runs playwright against the **step-3 binary**
   (`crates/xtask/src/verify.rs`). Even our strongest tier verifies "the last
   embedded build works," not "current sources work." No fingerprint/freshness
   check exists for preview (grep empty), though the exact precedent exists for
   `q2 mcp --launcher-info` (`crates/quarto-mcp-launcher/`).
3. **The mocks encode the author's assumptions, and assumptions drift.** The
   jsdom tier (`q2-preview-spa/src/PreviewApp.integration.test.tsx`) mocks
   everything below the `@quarto/preview-runtime` facade; capture bytes are
   `Uint8Array([1,2,3,4])` sentinels checked by identity; the renderer iframe is
   replaced by a div. All four bug classes are invisible there *by construction*.
   Bug (b) was exactly a fixture-shape drift: the test double spoke a dialect no
   real engine speaks.
4. **Entry-point divergence.** The preview integration tier enters via in-process
   `quarto_preview::run(...)` with an injected `engine_registry` — bypassing clap,
   project resolution, and the CLI's default-registry wiring. The only tests that
   spawn the real `q2` binary check `--help`
   (`crates/quarto/tests/integration/preview_cli.rs`). This is the same divergence
   class as the 2026-04-20 CodeHighlightStage incident.
5. **The real-engine coverage that exists is gated off by default.** deno-gated
   tests `test.skip`/skip *silently* when deno is absent; `pc4a` is `#[ignore]` +
   `QUARTO_PC4A_LIVE=1`; PC6 (real julia in a browser) is opt-in
   `QUARTO_PC6_LIVE=1`. A silent skip reads as green. And the user hit the bugs in
   Firefox while default engine e2e coverage is chromium-only.
6. A false friend: hub-client's `q2-preview-*` e2e specs run hub-client's renderer
   against the **hub** binary — no capture driver, no engines. They cover a
   different chain than `q2 preview` engine capture.

**Reframe:** playwright kept being "the thing that makes features real" not because
browsers are magic, but because it was the only tier with all three of: the real
entry point, real bytes (no mocks), and the real built artifacts. Any tier given
those three properties catches the same bug classes at a fraction of the cost.

## 2. Recommendations, in priority order

### R1. Artifact-freshness checks (kills bug class (d)) — highest leverage
- Embed a source fingerprint (git commit + dirty flag + build time, plus a content
  hash of the WASM) into the SPA dist and the q2 binary at build time; expose
  `q2 preview --embedded-info` (mirror `q2 mcp --launcher-info`).
- A cheap always-on Rust test asserts embedded hash == on-disk dist hash; playwright
  `globalSetup` refuses to run against a stale binary (today it only checks the
  binary *exists*: `q2-preview-spa/e2e/globalSetup.ts`).
- Fix `verify --e2e` step ordering (rebuild dist before — or rebuild/re-embed the
  binary after — the hub-build leg).
- Same treatment for the committed engine bundles (`quarto-engine-host-deno/dist/`,
  test-fixture `dist/echo-engine.js`): a src↔dist parity check in CI instead of the
  manual md5 comparisons we did by hand this branch.
This is not "more tests"; it is making the build graph unable to lie. Several
"green but broken for me" episodes (including 2026-05-20) were this and only this.

### R2. Headless Node chain harness (covers L9 + most of L10, kills the "silent
delivery break" class)
The pattern already exists: `ts-packages/quarto-sync-client/src/
offline-creation-rust-hub.test.ts:92` spawns the real `target/debug/hub` binary and
drives the real sync-client over real websockets. Clone it pointed at
`target/debug/q2 preview --no-browser`: assert `onCapturesChange` fires with the
real capture sidecar, `getBinaryDocById` returns real
`application/x-engine-capture+gzip` bytes, and (optionally) feed those bytes into
the WASM running in Node. Vitest-speed, no browser, and it would have localized
Bug B's break in one run instead of a browser bisection.

### R3. Native twins + wasm smokes for `wasm-quarto-hub-client` (covers L11)
The crate exporting `render_page_for_preview` has **zero tests of any kind** (no
`tests/`, no `#[test]` in `src/`). Rule going forward: every exported entry point
gets (i) a native test of its glue (capture gunzip/parse, VFS `/project/` prefixing,
registry threading, response shaping — mostly target-independent code), and (ii) one
`wasm_bindgen_test` smoke. Note this **complements, not duplicates, PR #109**
(Christophe's wasm-testing work): that PR gives pampa real *wasm32-target* tests +
CI ("does the code survive the target?"); the twins test *behavior* natively
("does the glue do the right thing?") where iteration is seconds. PC-B
(`capture_splice_seam.rs`) is effectively the first native twin, one layer down in
quarto-core; the crate-local glue above it is still dark. P2's diagnosis showed the
payoff: driving the splice natively found in minutes what the browser hid for days.

### R4. Fixture-parity contracts (kills bug class (b) recurrence)
Any test double that speaks a wire/output format gets a parity test against the real
producer's shape — e.g. echo fixture output must parse as the same `::: {.cell}`
structure `mdFromCodeCell` emits (`ts-packages/quarto-api/src/jupyter/
to-markdown.ts:671-878`). Cheap standing version: golden capture fixtures recorded
once from a real engine run, committed, and shared across the Rust, Node, and jsdom
tiers — so every tier chews real bytes instead of `Uint8Array([1,2,3,4])`.

### R5. One real-binary smoke below playwright (closes the entry-point divergence)
One or two Rust tests that spawn `target/debug/q2 preview <temp-project>` to
readiness and assert a capture got recorded + served over HTTP. Between
"in-process with injected registry" and "playwright" there is a currently-unoccupied
tier: *real binary, no browser*. This is where CodeHighlightStage-class bugs die.

### R6. Un-silence the gates
CI should *assert* deno is present so PC5-class guards actually run (a skipped
guard is a false green); the julia legs (pc4a, PC6, j1–j6) belong in a scheduled
julia-installed CI job rather than opt-in-forever. Consider one firefox project in
the default e2e matrix for the capture spec — the user's browser was Firefox.

### R7. Loud silent-drops (observability as testing)
Bug B produced no error anywhere — the splice simply no-oped. Every silent-drop
branch of the browser-side capture path (`onCapturesChange` with no matching file,
`getBinaryDocById` miss, splice-map empty) should emit a structured console
diagnostic (browser-side analogue of the Q-code discipline). This shrinks how much
e2e you need: one smoke per chain suffices when failures identify themselves. It
also fixes the L10/L13 debugging experience for the cases that do reach a browser.

### R8. Keep playwright — thin
L13 (sandboxed iframe DOM, real WASM instantiation, no-reload repaint) and true
cross-browser behavior have no substitute. Target state: **one smoke per
user-visible chain** (PC5 is the template — full chain, fail-on-revert-proven,
seconds-scale assertion), not one spec per feature. New features should bind their
links at R2/R3/R5 tiers and ride the existing chain smoke; a new playwright spec is
only warranted when a genuinely new chain appears.

## 3. Structural vs incidental (the honest ledger)

| Playwright-only link | Verdict | Path to lower tier |
|---|---|---|
| L9 capture sync round trip | Incidental | R2 (hub-spawn pattern exists; swap target binary) |
| L10 React state from real events | Mostly incidental | R2 + jsdom with real runtime; final DOM leg stays e2e |
| L11 WASM entry glue | Incidental | R3 (wasm-bindgen-test precedent: `crates/wasm-qmd-parser/tests/web.rs`) |
| L13 iframe DOM/no-reload repaint | **Structural** | none — this is what playwright is for |
| L14 artifact freshness | Incidental (nobody built it) | R1 (launcher-info precedent) |
| L7 real-engine lifecycle (a) | Incidental-by-gating | R6 (repros exist: pc4a, PC6) |

## 4. What this branch already fixed vs what remains

Post-branch, bug classes (b) and (c) are pinned natively (fixture emits real `.cell`
shape + `capture_splice_seam.rs` binds both directions; `ts_process_framing_probe.rs`
reproduces the stdout leak with a real deno child; the reader's bounded escalation
has unit coverage), and (a) has a native repro (`pc4a`) + upstream unit seam
(`worker-close.test.ts`). The playwright specs PC5/PC6 cover the browser half. So
for *this* feature area the gaps are closed or explicitly gated. The
recommendations above are about the *next* feature: R1–R5 are what would let it be
developed green-and-actually-working without reaching for a new playwright spec.

## 5. Appendix: chain map (tiers touching each link)

```
L1  CLI parse/project resolution   — tier 4 (--help only), 7
L2  server boot (axum+samod+SPA)   — 2, 7
L3  eager capture driver           — 1, 2, 7
L4  engine registry resolution     — 1 (deno-gated pair), 3, 7
L5  engine host spawn (deno child) — 3, 6 (synthetic streams), 7
L6  JSON framing / leak resilience — 3 (real-child probe), 6, 7-PC6 (opt-in)
L7  real engine lifecycle (QNR)    — 3 (gated), 9 (mocked/other-consumer), 7-PC6 (opt-in)
L8  capture record write (sidecar) — 1, 2, 7-PC5 (fail-on-revert proven)
L9  samod sync → sync-client       — 6 (real hub, capture path MOCKED), 7 ONLY for real
L10 React state from real events   — 5 (mock-injected), 7 ONLY for real
L11 WASM entry glue                — none natively (zero tests in crate), 7 ONLY
L12 CaptureSplice stage            — 3 (13 unit + 3 seam), 7
L13 iframe DOM presentation        — 7 ONLY (structural)
L14 embedded-artifact freshness    — NO TIER (verify --e2e is one generation stale)
```

Tier key: 1 preview in-file unit · 2 preview integration (in-process) · 3
quarto-core engine e2e · 4 CLI binary · 5 SPA jsdom · 6 ts-packages · 7 playwright ·
8 hub-client (different chain) · 9 upstream engine repo.

Full survey with counts, mock inventories, and per-tier evidence: produced in-session
(SDD scratch `.superpowers/sdd/test-tier-survey.md`, not committed); this note is the
durable record.
