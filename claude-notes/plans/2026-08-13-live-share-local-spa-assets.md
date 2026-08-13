# Live-share join: cut first-join payload (embedded-SPA serving + compression)

**Epic:** bd-puc7xt6e
**Date:** 2026-08-13
**Status:** Phase 0 complete (2026-08-13) — baseline recorded, skeletons landed red
**Parent context:** `claude-notes/plans/2026-08-03-q2-preview-live-share-iroh.md` (live-share design; spike measured 13.2 s cross-network first-render)

## Overview

A guest running `q2 preview --join <TICKET>` currently fetches **everything**
through the iroh tunnel: the SPA (`q2-preview-spa/dist/` is ~50 MB, dominated
by the ~42 MB `wasm_quarto_hub_client_bg.wasm`), the config endpoints, and the
`/ws` sync. The join payload is the dominant term in first-render latency,
especially on relay paths.

But the guest necessarily has the q2 binary installed — and that binary embeds
the same SPA bundles (`EMBEDDED_SPA`, `EMBEDDED_EDITOR` in
`crates/quarto-preview/src/lib.rs`). When the guest's embedded copy is
byte-identical to the host's, asset fetches can be served locally and only the
dynamic traffic (`/ws`, `/api/*`, `/auth/*`, `/.quarto/*`, `/health`) needs
the tunnel.

**Compatibility is verified, never assumed.** The host advertises a manifest
hash in `GET /api/preview/config` (which `run_join` already fetches through
the tunnel as a preflight). The guest serves assets locally iff its embedded
manifest hash matches exactly; any mismatch, missing manifest (fresh-clone
placeholder embed), or `SPA_DIR_OVERRIDE` dev session falls back to today's
full-tunnel behavior. Hash match implies byte-identical assets, so the trust
boundary does not move.

**Order of work matters.** Phase 1 (wasm-opt + release-profile tuning +
precompressed assets) requires no architectural change and may cut the
payload 3–4× on its own. Phase 3 (L7 local serving) is gated on post-Phase-1 measurements showing
first-join latency still hurts.

## Design decisions (recorded up front)

1. **quarto-p2p stays a pure transport crate.** Split
   `TunnelClient::bind` into:
   - `TunnelClient::connect(cfg, ticket) -> TunnelConnection` — endpoint,
     initial dial, supervisor/re-dial, status watch; exposes
     `open_stream() -> (SendStream, RecvStream)` which applies the token
     prefix internally.
   - `TunnelClient::bind(...)` — unchanged signature and behavior; reimplemented
     as `connect` + the existing splice accept loop. All existing
     `crates/quarto-p2p/tests/integration/tunnel.rs` tests must stay green
     unmodified.

2. **The L7 frontend lives in quarto-preview** (new module
   `src/join_frontend.rs`), not in quarto-p2p and not in the CLI crate.
   quarto-preview already owns the embedded bundles, `lookup_embedded`, and
   `asset_response`. Note: `asset_response` today sets only Content-Type —
   the cache-header and `Content-Encoding` behavior is created in Phase 1,
   and the frontend must share that exact helper (never fork it) so header,
   encoding-negotiation, and HEAD semantics stay in one place.
   `run_join` in `crates/quarto/src/commands/preview.rs` calls it in place of
   `TunnelClient::bind` when local mode is active.

3. **Per-connection routing via head-peek, not a per-request reverse proxy.**
   The frontend reads the request head (bounded: 64 KiB cap, 5 s timeout),
   and routes the *whole connection*:
   - `GET`/`HEAD` whose path — after `spa_handler`'s exact normalization
     (query string stripped, `trim_start_matches('/')`, empty →
     `index.html`, raw percent-encoded path, no decoding) — is an **exact
     hit in the manifest** → serve from the embedded bundle via the shared
     `asset_response` helper (which handles HEAD as headers-only with the
     correct `Content-Length`), plus `Connection: close`.
   - Anything else → `open_stream()`, write the consumed head bytes
     verbatim, then `copy_bidirectional`. That explicitly includes `/ws`,
     `/api/*`, `/auth/*` (the hub registers `/auth/me`, `/auth/actor`,
     `/auth/logout`, `/auth/logout-everywhere` unconditionally and the SPA
     queries auth at boot), `/health`, `/.quarto/*`, all non-GET methods,
     and **all unknown paths — there is no local SPA-index fallback**.
     WebSocket upgrades and keep-alive follow-up requests flow through the
     splice untouched — byte fidelity is total, which is why this beats
     reconstructing requests through hyper.
   - **No local index fallback, deliberately.** Mirroring `spa_handler`'s
     "any unmatched path gets `index.html`" locally would shadow any
     present-or-future host route the tunnel list fails to name (a new
     non-`/api` route with unchanged assets → hash match → shadowed), and
     would have shadowed `/auth/*` today. `index.html` is a few KB fetched
     once per boot, so tunneling it (and client-side-route paths) costs
     nothing measurable, and the whole class of shadowing bugs disappears:
     the host stays the single authority on what is dynamic.
   - `Connection: close` on local responses avoids the mixed-connection
     problem (browser reusing one connection for both local and tunneled
     paths) at the cost of extra loopback accepts, which are microseconds.
     A full per-request proxy with hyper upgrades was considered and
     rejected: far more code for no user-visible gain.

4. **Manifest generation at build time, embedded with the dist.** `cargo
   xtask build-q2-preview-spa` writes `spa-manifest.json` for the viewer
   dist; the **editor manifest is written by `build.rs`**, because only
   `build.rs` knows the post-dedupe file set (it strips editor-dist files
   byte-identical to the viewer dist; `lookup_embedded` falls back
   editor → viewer). The recorded view is post-resolution: what
   `lookup_embedded(ui, path)` would actually return. Entries are sorted
   `(path, sha256, size, content_type, content_encoding?)` plus a top-level
   hash; the manifest **excludes itself** from its own entries (a manifest
   cannot contain its own hash). Placeholder embed dirs (fresh clone) ship
   no manifest → hash `None` → tunnel mode, self-healing. Hash equality
   assumes the embedded dist is byte-identical across host and guest
   binaries, which release CI builds on different platforms/jobs — a
   mismatch is safe (tunnel fallback) but silently disables local mode, so
   Phase 2 adds a CI check comparing manifests across release artifacts.

5. **Config handshake.** `preview_config_handler`
   (`crates/quarto-preview/src/lib.rs`) gains
   `assets: { "viewer": "<sha256…>", "editor": "<sha256…>" }` (fields omitted
   when the corresponding embed has no manifest, and the whole `assets` block
   omitted when `SPA_DIR_OVERRIDE` is active — disk-served bytes are not
   described by the embedded manifest). The guest compares the hash for the
   session's UI (viewer unless `editorBoot` is present) against its own
   embedded manifest.

6. **Sequencing improvement over today.** With `connect` (no listener), the
   join path can run the `/health` + `/api/preview/config` preflight over a
   raw bi-stream *before* binding the local port, decide the mode, and then
   start the right frontend. No browser connection can arrive before the mode
   is fixed.

7. **Security posture unchanged.** Token auth applies to every tunneled
   stream exactly as today. Locally served assets never touch the host and
   need no token — the guest already carries those bytes in its own binary,
   and hash equality with the host's manifest proves they are the bytes the
   host would have served.

## Work items

### Phase 0 — Baseline measurement + test specifications (TDD)

- [x] Measure and record in this file: (a) exact set of requests the SPA
  issues at boot (index, JS/CSS, both WASM blobs, tree-sitter WASM, fonts on
  demand) with byte sizes; (b) first-render wall time for a join forced onto
  a relay path; (c) same over a simulated slow link (e.g. 10 Mbps / 100 ms).
  *(done 2026-08-13 — see "Phase 0 baseline" below)*
- [x] Audit `hub-client/scripts/build-wasm.js`: is the WASM built
  `--release` and wasm-opt'd? **Answer (2026-08-13): release — yes;
  wasm-opt — no.** The script runs `cargo build --target
  wasm32-unknown-unknown --release` + `wasm-bindgen`, and
  `crates/wasm-quarto-hub-client/Cargo.toml` has no `[profile.release]`
  tuning (no `lto`, no `opt-level = "s"`, default codegen-units). Current
  blob: 41,889,256 B. Phase 1's headroom is real.
- [x] Write the failing test skeletons for Phases 1–3 (specs below) so each
  phase starts red. *(done 2026-08-13 — see "Phase 0 test skeletons"
  below)*

#### Phase 0 baseline (measured 2026-08-13)

Harness: `scripts/join-boot-baseline/` (driver + throttle proxy + exact
invocations; committed so Phase 1's gate re-run is reproducible). Binary:
`target/debug/q2` built 2026-08-13 09:39, embedding the 2026-08-12 dist.
Fixture: `_quarto.yml` (website) + `index.qmd` (`MARKER-0`) + `about.qmd`.
Browser: Playwright Chromium 1223; first render = fixture marker text
visible in the renderer iframe. All output inspected.

**(a) Boot request set** (viewer SPA, plain document, direct loopback —
first render 668 ms). 20 network requests + 1 WebSocket upgrade;
**54,672,999 B** over the wire:

| Request | Bytes | Notes |
|---|---|---|
| `GET /?page=index.qmd` | 1,115 | index.html |
| `GET /assets/main-*.js` | 67,791 | app entry |
| `GET /assets/main-*.css` | 3,186 | |
| `GET /assets/meta-*.js` | 495,574 | **fetched twice** (app + iframe) |
| `GET /assets/wasm_quarto_hub_client-*.js` | 21,275 | wasm-bindgen glue |
| `GET /assets/wasm_quarto_hub_client_bg-*.wasm` | 41,889,256 | the dominant term |
| `GET /assets/automerge_wasm_bg-*.wasm` | 3,571,259 | **fetched twice** (app + iframe) |
| `GET /assets/sass-*.js` + `sass.default-*.js` | 2,062 + 3,309,890 | in-browser theme compile |
| `GET /q2-preview.html` + iframe assets | 773 + 85,111 + 1,158,524 | renderer iframe doc/CSS/JS |
| `GET /health` (×2), `/api/preview/config`, `/api/preview/deps`, `/api/preview/diagnostics` | 351 | dynamic, must tunnel |
| `WS /ws` | — | upgrade; sync + presence |

Findings beyond the byte list:

- **4,066,833 B are duplicate fetches** (`meta-*.js` + automerge wasm,
  once by the app and once by the renderer iframe). With no cache
  headers today the iframe's copies re-download; Phase 1's `immutable`
  `/assets/*` headers should make the second fetch a cache hit.
- The 333,798 B `text/css` "request" to a `/<uuid>` path seen in the
  capture is **local**, not network: `blob:` URL from
  `URL.createObjectURL` (`ts-packages/preview-renderer/src/iframe/
  Q2PreviewIframe.tsx:431`, the compiled theme CSS). Excluded from the
  wire total.
- **Tree-sitter WASM** (`web-tree-sitter-*.wasm`, 200,297 B) is in the
  dist but **not fetched at boot** for a plain document.
- **Fonts are on demand** as expected: none for a plain document; adding
  math (`$E=mc^2$` + display block) adds exactly 2 KaTeX woff2 files
  (16,440 + 26,272 B) — 22 requests, 54,715,711 B.
- **No `/auth/*` request observed at viewer boot** (design decision 3's
  tunnel set keeps `/auth/*` regardless — the hub registers those routes
  unconditionally and the editor UI does query them).
- Static inventory: viewer dist 74 files / 51,883,391 B; editor dist
  (pre-dedupe) 187 files / 73,956,901 B. **The editor-embed dedupe is
  currently not firing**: viewer wasm sha256 `9f960de8…` ≠ editor wasm
  `48a45e97…` (the drift the parent plan's operational note predicts —
  `build:preview-embed` re-ran `build:wasm` after the viewer dist was
  built), so today's editor embed carries its own 41.9 MB wasm copy
  (post-dedupe embed dir: 124 files / 65,802,474 B vs ~23.9 MB when
  aligned). Affects binary size only, not served bytes; Phase 1 rebuilds
  both dists anyway.

**(b) Relay-pinned join: first render 3,215 ms.** Methodology: Gate 0
spike pair (`spike-tunnel-host` → `spike-tunnel-client --relay-only`;
the real `--join` has no relay-pinning knob) in front of the real
preview server; same 54,672,999 B payload; selected path stayed
`euc1-1.relay.n0.iroh.link` (rtt ~200–256 ms) with **zero DIRECT
selections** for the whole boot. Same-machine/fast-uplink number, so it
understates residential guests; the parent plan's cross-network leg
(Azure↔residential, all-relay) measured **13.2 s** on a ~47.5 MB
payload — that remains the residential-class reference.

**(c) Simulated slow link (10 Mbps down, 100 ms RTT): first render
48,041 ms** through the *real* `--share`/`--join` stack, throttle proxy
between browser and guest port (one shared downstream bucket — a real
link is shared across Chromium's parallel connections). Theoretical
floor at 10 Mbps for 54.67 MB is 43.7 s; the ~4.3 s above floor is RTT
and queueing. Gate context for Phase 1: ≤ 5 s at 10 Mbps needs ≲ 6 MB
delivered — brotli on the wasm alone (~3.5–4×) lands ~15 MB total, so
compression likely closes most but not all of the gap; that is exactly
the gate measurement Phase 1 re-runs.

#### Phase 0 test skeletons (landed 2026-08-13)

All skeletons are `#[ignore = "…"]`d so the workspace suite stays green
until the owning phase lands; each phase un-ignores its tests and starts
red (the ignore messages name the phase strand). Observed with
`cargo nextest run -p quarto-preview -p quarto-p2p --run-ignored only
--no-fail-fast`: **22 skeleton tests fail for the right reasons**
(todo!() panics on the structural stubs; assertion failures naming the
missing behavior on the complete ones), output inspected.

- **Phase 1** — `crates/quarto-preview/tests/integration/asset_serving.rs`
  (complete bodies against today's public seams, runtime-red):
  `br_served_when_accepted` (fails: no `Content-Encoding: br`),
  `br_bytes_roundtrip_to_identity` (fails same way; uses the new
  `brotli` dev-dep), `cache_headers_match_local_prod_contract` (fails:
  no `Cache-Control`; pins `no-cache` for `/` and `public,
  max-age=31536000, immutable` for `/assets/*` per
  `scripts/local-prod-server.mjs`), `identity_served_without_br_acceptance`
  (**passes today by design** — it guards the identity path against
  regression once `.br` serving lands).
- **Phase 2** — `config_endpoint.rs` gains
  `config_reports_embedded_asset_manifest_hashes` (fails: no `assets`
  block; placeholder-aware so fresh-clone CI stays green) and
  `config_omits_assets_under_spa_dir_override` (**not ignored, green
  today** — guards design decision 5's carve-out). New
  `asset_manifest.rs` holds 8 structural stubs (todo!() + spec):
  manifest determinism / hash sensitivity / self-exclusion /
  post-resolution editor view, and the four mode decisions (match →
  Local; mismatch / missing manifest / override → Tunnel).
- **Phase 3** — `crates/quarto-p2p/tests/integration/connect.rs`
  (`connect` + `open_stream` round-trip; rejected token → terminal
  `TunnelStatus::Rejected`) and
  `crates/quarto-preview/tests/integration/join_frontend.rs` (8 stubs:
  local serving on match incl. zero host asset requests, full tunnel on
  mismatch, unknown-path tunneling, WS splice survival, 431 on oversize
  head, head-peek timeout close, HEAD headers-only, editor-UI local
  boot). Harness notes for each live in the file headers.
- Pre-existing, unrelated: the already-ignored
  `staleness::cell_edit_flips_staleness_in_sidecar` (bd-9brz, FSEvents
  starvation) also fails when forced — not caused by this work; left
  ignored.

### Phase 1 — Payload reduction (no architecture change)

- [ ] Add wasm-opt (`-Oz`) to `build:wasm` after the wasm-bindgen step, and
  tune `[profile.release]` in `crates/wasm-quarto-hub-client/Cargo.toml`
  (`lto`, `opt-level = "s"`, `codegen-units = 1` — measure each
  independently). The build is already `--release` per the Phase 0 audit;
  record before/after sizes.
- [ ] Emit precompressed `.br` siblings **only** (no `.gz` — decided
  2026-08-13 to minimize binary growth) at build time (vite plugin or an
  xtask post-pass over both dist dirs); extend `asset_response` to serve
  them when `Accept-Encoding` allows, with correct `Content-Encoding` and
  `Vary`. Identity bytes stay embedded for clients that don't send `br`.
  Verify in Phase 4's browser session that the target browsers actually
  advertise `br` for `http://127.0.0.1` origins (potentially-trustworthy,
  but confirm — a browser that doesn't gets identity and the full payload).
- [ ] Record the binary-size delta (embedded identity + `.br` vs. identity
  alone) in this file, the same way the parent plan tracked the iroh
  dep-tree delta.
- [ ] Audit cache headers on `/assets/*` (content-hashed → `immutable`) and
  `/` (`no-cache`), matching the local-prod contract. (Today
  `asset_response` sets only Content-Type — this phase creates the
  behavior.)
- [ ] Tests: asset handler returns `Content-Encoding: br` for an
  `Accept-Encoding: br` request and identity otherwise; compressed bytes
  round-trip; cache headers as specified.
- [ ] Re-run the Phase 0 measurements. **Gate:** if first-render over the
  simulated slow link is now acceptable (target: ≤ 5 s), Phases 2–3 can be
  deferred; note the decision here.

### Phase 2 — Manifest + config handshake

- [ ] Manifest generation per design decision 4: xtask writes the viewer
  manifest; `build.rs` writes the editor manifest after the dedupe strip
  (post-resolution view per UI). Deterministic: sorted entries, stable
  hash, manifest excludes itself. Unit test: byte-identical regeneration;
  hash changes when any asset byte changes.
- [ ] `preview_config_handler`: add the `assets` block per design decision 5.
  Test in `crates/quarto-preview/tests/integration/config_endpoint.rs`.
- [ ] Guest side: parse the `assets` block in the join preflight (extend the
  hand-rolled fetch in `preview.rs` or move preflight onto a bi-stream — see
  Phase 3), compare against the embedded manifest for the session UI, and
  log the decision (`using embedded UI assets (hash match)` / `tunneling
  assets (hash mismatch)`).
- [ ] Unit tests: match → Local; mismatch → Tunnel; missing manifest →
  Tunnel; override active → Tunnel.
- [ ] CI: compare the viewer/editor manifest hashes across the per-platform
  release artifacts and fail the release on drift — a cross-platform
  mismatch is safe at runtime (tunnel fallback) but silently disables
  local mode for cross-platform share pairs.

### Phase 3 — L7 join frontend

- [ ] quarto-p2p: extract `TunnelClient::connect` + `TunnelConnection` per
  design decision 1; `bind` reimplemented on top; existing tunnel tests
  unmodified and green. New test: `open_stream` round-trip + token rejection
  still maps to `TunnelStatus::Rejected`.
- [ ] quarto-preview `join_frontend.rs`: loopback listener, bounded head-peek,
  routing per design decision 3, local responses byte-identical to what
  `asset_response` produces (shared helper — do not fork header logic).
- [ ] `run_join`: preflight-first sequencing (design decision 6); select
  frontend vs. plain `TunnelClient::bind` by mode.
- [ ] Integration tests (`crates/quarto-preview/tests/integration/join_tunnel.rs`
  harness): matching manifest → host request log shows **zero** asset
  requests, while `/ws` sync, `/auth/me`, and `/api/preview/config` still
  traverse the tunnel; mismatched manifest → all requests tunnel; unknown
  path (no manifest hit) tunnels and receives the host's `index.html`,
  never a locally synthesized one; WebSocket survival through the fallback
  splice; oversize head → 431 + close; head-peek timeout → close; HEAD
  request → headers only with correct `Content-Length`; editor-UI session
  boots from the locally served editor index (`/` normalizes to an exact
  `index.html` manifest hit).
- [ ] Full workspace: `cargo nextest run --workspace` and `cargo xtask
  verify --skip-hub-build` green (hub-build leg needed only if the embed
  inputs changed — they do in Phase 1, so run full `cargo xtask verify`
  before committing Phase 1).

### Phase 4 — End-to-end verification + docs

- [ ] Real end-to-end per CLAUDE.md: `cargo run --bin q2 -- preview --share`
  on one machine/profile, `q2 preview --join` on another, browser session
  inspected; record the exact invocations, the observed asset-serving
  behavior (host log shows no asset fetches), and before/after first-render
  timings in this file.
- [ ] `q2 preview --join --help` text mentions embedded-asset serving and the
  mismatch fallback.
- [ ] Close out braid strands; snapshot via `cargo xtask braid-snapshot`.

## Risks / edge cases

- **Mixed keep-alive connections** — mitigated by `Connection: close` on
  local responses (design decision 3).
- **Editor/viewer dedupe** — manifest must hash the resolved per-UI view or
  editor-mode guests will spuriously mismatch.
- **Dev mode (`SPA_DIR_OVERRIDE`)** — config omits `assets`; guests tunnel.
- **Fresh-clone placeholder embeds** — no manifest; guests tunnel.
- **WASM streaming compilation** — `WebAssembly.compileStreaming` requires
  `Content-Type: application/wasm`; the manifest carries content type and the
  local server must reproduce it exactly.
- **Future endpoints** — any new host route automatically tunnels: local
  serving is exact-manifest-hit only, with no local index fallback, so an
  unrecognized path can never be shadowed by a locally synthesized
  `index.html`. (`/auth/*` is the existing case this rule protects.)
- **Binary-size growth** — embedding `.br` siblings adds roughly the
  compressed size of the dist (~a quarter of identity at brotli q11, so
  on the order of 10–13 MB) to every `q2` binary. `.br`-only (no `.gz`)
  caps this; Phase 1 records the measured delta.
- **Browser `br` support on plain HTTP** — if a target browser omits `br`
  in `Accept-Encoding` for `http://127.0.0.1`, that browser gets identity
  and the full payload. Phase 4's real browser session must confirm the
  negotiation; if a major browser omits `br`, reconsider per-request
  compression for the identity path rather than re-adding `.gz`.

## Communication record

- 2026-08-13: Plan drafted from the network-adapter review discussion.
  Design decisions 1–7 recorded before implementation. Awaiting Phase 0
  measurements.
- 2026-08-13: Phase 0 done (bd-lbvtfejg). Baseline: 54.67 MB / 21
  requests at viewer boot; first render 0.67 s direct, 3.2 s
  relay-pinned (same machine), 48.0 s at 10 Mbps/100 ms through the real
  join stack. Skeletons for Phases 1–3 landed ignored (22 red when
  forced, 2 deliberate guards green); `brotli` added as a quarto-preview
  dev-dep for the round-trip test. Editor-embed dedupe drift recorded
  (viewer/editor wasm hashes diverged; binary-size only).
- 2026-08-13: Review fixes applied: `/auth/*` added to the tunnel set
  (hub registers auth routes unconditionally; the SPA queries auth at
  boot); local SPA-index fallback removed in favor of exact-manifest-hit
  serving, resolving the unknown-path contradiction and eliminating route
  shadowing; Phase 0 WASM audit answered (release yes, wasm-opt no, no
  profile tuning); precompression scoped to `.br` only to cap binary
  growth, with the size delta to be recorded in Phase 1; HEAD semantics
  assigned to the shared `asset_response` helper; manifest self-exclusion
  and `build.rs` ownership of the post-dedupe editor manifest clarified;
  cross-platform manifest CI check added to Phase 2.
