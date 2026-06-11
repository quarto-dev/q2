# `q2 mcp` — embed & delegate to the TypeScript hub MCP server

**Strand:** bd-81cfshmw
**Status:** DESIGN — iterating with Carlos; do not start implementation
until he gives the go-ahead.

## Overview

Goal (Carlos, 2026-06-11): give anyone with `q2` installed an easy path
to let LLM apps and agents (Claude Desktop, Claude Code, etc.) read and
write quarto-hub.com documents *as a participant in the multiplayer
automerge session*. quarto-hub.com happens to be gated behind auth, so
`q2 mcp` must speak the auth flow — but auth is the gate, not the goal.

**Architecture (v2, Carlos's proposal):** the TypeScript
`ts-packages/quarto-hub-mcp` server is and remains the canonical MCP
implementation — it already has the 11-tool surface, the loopback+PKCE
auth (shipped 2026-06-10, `2272451f`), keyring storage, and the
connection/threat-model work. `q2 mcp` becomes a thin Rust launcher:

1. Bundle the TS server into a self-contained distribution
   (single `.mjs` + the platform keyring addon) during the build.
2. Embed that bundle in the `q2` binary (`include_dir!`-style, same
   pattern as the q2-preview SPA).
3. At `q2 mcp` invocation: extract the bundle to a per-user cache dir,
   find an ambient `node`, and delegate execution verbatim
   (`q2 mcp [args…]` → `node <bundle>/index.mjs [args…]`, stdio
   inherited).

The same bundle is published to npm so non-q2 users get it with one
`npx` command. One implementation, two delivery channels.

Cost: `q2 mcp` requires Node on the user's machine. Accepted tradeoff
(2026-06-11) given Node's ubiquity — and given that hub-client +
quarto-hub constraints keep the auth/sync code TypeScript for the
foreseeable future, so a Rust port would mean maintaining
security-critical code twice.

### Decision history

- **v1 (2026-06-11, superseded):** Rust-native port — new
  `crates/quarto-mcp` with `rmcp`, a port of the loopback+PKCE flow,
  a samod `BearerDialer`, and a re-implemented document layer. Rejected
  in favor of v2: large scope, and worst of all it *duplicates the auth
  and sync semantics in two languages*, which then have to be kept in
  security-parity forever. The v1 research survives below
  (§ Rust-native findings) since it tells us what a future native port
  would cost if the Node dependency ever becomes unacceptable.
- **Auth flow question resolved:** "the device flow we use" meant "the
  auth flow the hub MCP uses" — which is loopback+PKCE as of
  `2272451f`. v2 inherits it for free, along with every future auth
  change, because the TS code is the single source of truth.
- **Risk review confirmed (Carlos, 2026-06-11):** all six
  problem/mitigation pairs below accepted as designed, including the
  execution model they imply — `q2 mcp` extracts the embedded bundle
  to a per-user cache dir and runs ambient `node` against it (one-time
  extraction per content hash). Node-discovery friction to be handled
  with layered discovery + documentation. Stale-embed precautions
  (xtask wiring, build stamp, build.rs guard) explicitly endorsed.

## What v2 dissolves (from the v1 open questions)

- *Which auth flow?* — inherited from the TS server, automatically.
- *OAuth client identity* — same client as the TS server; the keyring
  entries are **shared by design** (same code, same service name), so
  a user who authenticated via `npx quarto-hub-mcp` is already
  authenticated in `q2 mcp` and vice versa. No new
  `--additional-audiences` deployment change.
- *Where does the Rust document-model live?* — nowhere; no Rust doc
  model needed.
- *Tool-surface parity* — definitionally exact.
- *Fate of the TS package* — it's the canonical implementation, kept.

## Known problems & mitigations

Ordered by how much design they need. (1) and (2) are the load-bearing
ones; the rest are foreseeable engineering.

### 1. `@napi-rs/keyring` is a native addon — it cannot live in a single `.mjs`

The credential store loads a platform-specific `.node` binary. esbuild
cannot inline it; `external`-izing it breaks the extracted bundle
(nothing to `require` outside `node_modules`).

**Mitigation:** the "bundle" is a small **directory**, not one file:
`index.mjs` + exactly one `keyring.<platform>.node`. esbuild's `.node`
file-loader (or a tiny plugin) rewrites the addon require to a
bundle-relative path. Since each `q2` binary is already
platform-specific, it embeds only its own platform's addon (~hundreds
of KB). The npm channel is unaffected (normal npm resolution installs
the right addon). Risk to verify in spike: napi-rs's runtime loader
sometimes probes `node_modules` paths by package name — confirm the
rewrite cleanly bypasses that, or pin the load with an explicit
`process.env`/argument escape hatch.

### 2. Node discovery under GUI-launched MCP hosts

Claude Desktop (and most GUI MCP hosts) launch servers with a minimal
environment — on macOS, *not* the user's shell `PATH`. nvm/fnm-managed
Node lives in shell-init-added paths, so the naive `which node` fails
exactly in the flagship use case. (The user already has to make `q2`
itself findable, but `q2` is typically in a standard location; node
under a version manager is not.)

**Mitigation:** layered discovery in the Rust launcher:
`QUARTO_NODE` env override → `node` on `PATH` → well-known locations
(`/opt/homebrew/bin`, `/usr/local/bin`, volta/fnm/nvm shim and
`versions/node/*/bin` dirs, Windows `Program Files\nodejs`). Validate
with `node --version` against a minimum (floor TBD in spike — likely
≥ 20 LTS; automerge 3 / jose 6 / MCP SDK all want ≥ 18). On failure:
a clear stderr message naming the requirement and the `QUARTO_NODE`
override (MCP hosts surface server stderr in their logs).

### 3. The stale-embed trap (we have been burned by exactly this)

`cargo build --bin q2` will NOT rebuild the embedded `.mjs`; the binary
silently re-embeds whatever `dist/` was last built. This is the
2026-05-20 `q2 preview` stale-WASM incident, identically shaped (see
`claude-notes/instructions/preview-spa-rebuild.md`).

**Mitigation:** (a) wire the bundle build into `cargo xtask build-all` /
`verify` like `build-q2-preview-spa`; (b) stamp the bundle with the git
hash + build time at bundling, surfaced via `q2 mcp --launcher-info`
(and printed to stderr at startup in verbose mode), so staleness is
*diagnosable*; (c) `build.rs` fails with an actionable message when the
bundle dir is missing (fresh clone), pointing at the xtask — same
convention as the preview SPA. Document the rebuild chain in
CLAUDE.md alongside the existing preview-SPA section.

### 4. automerge's WASM in a single-file bundle

`@automerge/automerge` 3.x is wasm-backed; under plain `tsc` +
node_modules it loads fine, but single-file bundling needs the
base64-inlined variant or the `.wasm` emitted as a bundle asset
(another reason "bundle = directory" is the right shape). Known
territory — hub-client already bundles automerge under vite — but the
node/esbuild recipe must be proven in the spike, including
`@automerge/automerge-repo-storage-indexeddb` (a browser-only dep of
`quarto-sync-client`) not dragging browser globals into the node
bundle.

### 5. Extraction hygiene (where the bundle lands on disk)

Executing embedded code requires writing it to disk first.
**Mitigation:** extract to a per-user cache dir
(`dirs::cache_dir()/quarto/hub-mcp/<content-hash>/`, `0700`), atomic
rename from a temp sibling so concurrent `q2 mcp` launches don't race,
skip extraction when the hash-dir already exists. No `/tmp` (TOCTOU,
multi-user); macOS `/private/tmp` confusion avoided too.

**Cache GC (design settled 2026-06-11; Quarto 1 users have complained
about temp-storage pollution, so this is proactive, not optional).**
Naive delete-old-dirs is unsafe: node loads bundle pieces *lazily*
(the keyring `.node` addon loads at first auth use, not startup), so
deleting under a running instance breaks it mid-session. Refcount
files are equally wrong: a crashed process never decrements, counters
leak upward, GC never fires. Instead, the cargo/rustup pattern —
**lifetime advisory file locks** (`fd-lock` crate: flock / LockFileEx):

- Each instance takes a **shared** lock on `<hash>/.lock` after
  extraction and holds it for its lifetime. Shared locks coexist
  (reentrancy); the kernel releases them on process death, however it
  dies (crash safety, no state to corrupt).
- Unix exec() wrinkle: flock travels with the fd, and fds survive
  exec unless CLOEXEC — so the launcher clears CLOEXEC on the lock fd
  and execs node; node then holds the lock. On Windows the launcher
  stays alive (spawn+wait) and holds it itself.
- GC runs opportunistically at launch, after securing its own dir:
  for each sibling hash dir, **non-blocking try-exclusive lock**;
  success proves no user → rename to `.trash-<rand>` (concurrent new
  launchers fail cleanly, re-extract) → delete. Failure → skip.
- Chronology without a version tag: **touch the dir mtime on every
  launch**; GC only dirs with mtime older than N days (14–30, pick in
  Phase 2). This also prevents thrash when a dev build and an
  installed release alternate on one machine (keep-only-current
  would have each binary evicting the other's bundle). The risk-3
  build stamp inside the bundle is diagnostics, not policy.
- Bounded blast radius either way: dirs are ~3–5 MB, one per distinct
  q2 build actually used; Windows can't delete in-use files at all,
  so a GC bug degrades to "skip". No further machinery (quotas,
  daemons) warranted.

### 6. Process plumbing

stdio must pass through untouched — the MCP protocol owns stdout.
**Mitigation:** on Unix, `exec()` (`CommandExt::exec`) replaces the
launcher entirely: signals, exit codes, and stdin-EOF semantics are
node's, no middleman. On Windows (no exec): spawn with inherited
stdio, wait, forward the exit code; rely on stdin-EOF (the MCP SDK
exits when the host closes stdin) for shutdown, which is how MCP hosts
terminate servers anyway. Launcher rule: **never write to stdout**
(stderr only), except clap's own pre-delegation `--help`. Prefer
delegating `q2 mcp --help` to the TS server so help stays
single-sourced; the launcher claims only flags the TS server doesn't
own (e.g. `--launcher-info`).

### 7. Smaller, listed for completeness

- **npm publishing is not set up**: `@quarto/hub-mcp` is
  `"private": true` and `tsc`-built. The "single `npx` instruction"
  channel needs a public name (`quarto-hub-mcp`?), publish workflow,
  and the same bundling step. Separable from the q2-embed work but
  shares the bundler config.
- **Binary size**: ~3–5 MB embedded (bundle + wasm + addon). Noise
  relative to q2.
- **License notices**: bundling vendors dependencies into one file;
  generate a `THIRD_PARTY_LICENSES` file into the bundle dir as part
  of the bundling step.
- **Node-version drift over time**: ambient-node means we don't
  control the runtime; the min-version check plus CI running the
  bundle under the floor version and current LTS keeps this honest.
- **`bin` name collision**: launcher and npm bin should present the
  same `--server`/`--read-only`/`--redirect-port` surface; they do by
  construction (same code), but docs should canonicalize one
  invocation per channel.

### Alternatives considered for the runtime (and rejected)

- **Node SEA / `bun build --compile`** — truly zero-dependency, but
  +80–100 MB per platform inside q2. Out of proportion.
- **`npx quarto-hub-mcp@<pinned>` at runtime** — no embed step, but
  adds registry/network dependency and cold-start latency to first
  launch, and `npx` still requires Node anyway. The embedded bundle is
  strictly more reliable; npx remains the non-q2 channel.
- **Remote MCP (hub serves MCP-over-HTTP)** — the "no local anything"
  endgame; needs OAuth resource-server semantics on the hub and is a
  separate, bigger design. Explicitly out of scope here (kept from v1).

## Open questions (v2)

1. ~~**Node version floor**~~ — **RESOLVED (2026-06-11, Carlos):**
   pin the *current LTS* (Node 24) if it runs the TS auth code —
   verify in spike; only if it doesn't, fall back to the oldest major
   that does. (Local dev + CI are already on 24.15.0.)
2. ~~**npm package naming/publishing**~~ — **RESOLVED (2026-06-11):
   deferred to follow-up strand bd-3tak0lyy**, gated on general
   public-release readiness (same gate as cargo-publishing the Rust
   crates). The bundler config built in Phase 1 is shared with it.
3. ~~**`q2 mcp` extra UX**~~ — **RESOLVED (2026-06-11): yes** —
   `--server` defaults to `wss://quarto-hub.com/ws` (the main hub
   sync server for the foreseeable future), implemented in the TS
   server so both channels get it.
4. ~~**Where the bundler config lives**~~ — **RESOLVED (2026-06-11,
   Carlos):** in `ts-packages/quarto-hub-mcp` as an `npm run bundle`
   script; xtask invokes it. The npm publish pipeline (bd-3tak0lyy)
   reuses it.

**GO-AHEAD given 2026-06-11** — implementation started on branch
`beads/bd-81cfshmw-q2-mcp-launcher`.

## Phases & work items (TDD — tests first in every phase)

> Checklist stays unchecked until go-ahead.

### Phase 0 — spikes (COMPLETE 2026-06-11; findings below)

- [x] Spike: esbuild-bundle `quarto-hub-mcp` into `dist-bundle/`;
      run it with plain `node` *outside* the repo tree against a
      local hub; exercise connect/read/write. Risks 1 and 4 retired.
- [x] Spike: confirm napi-rs addon load path (risk 1 caveat) — **no
      rewrite needed at all**; see findings.
- [x] Spike: node-version floor — Node 24 (current LTS) runs
      everything; per Carlos's rule ("pin current LTS if sufficient"),
      **floor = Node 24**, esbuild target `node24`.
- [x] Open questions 1–4 all resolved (see above).

**Phase 0 findings (2026-06-11):**

- `npm run bundle` (new `scripts/bundle.mjs` in
  `ts-packages/quarto-hub-mcp`, per resolved Q4) produces
  `dist-bundle/`: `index.mjs` (4.9 MB), `build-info.json` (git commit
  + dirty flag + build time + node target), and
  `node_modules/@napi-rs/keyring{,-<platform>}/`.
- **Keyring (risk 1): simpler than feared.** Keeping
  `@napi-rs/keyring` external and shipping it as a mini
  `node_modules` inside the bundle dir means node's ordinary
  resolution (relative to `index.mjs`) finds it — no loader rewrite,
  no esbuild plugin. Proven: the credential store imports it
  statically, so every successful bundle run loads the addon.
- **automerge wasm (risk 4): resolved via entrypoint steering.** The
  package's `node` export condition reads `automerge.wasm` with a
  `__dirname`-relative `readFileSync` (cannot survive bundling); the
  default `import` condition (`fullfat_base64.js`) inlines the wasm
  as base64 and bundles cleanly. A 6-line esbuild resolve plugin pins
  bare `@automerge/automerge` to the base64 entrypoint. (`/slim`
  imports untouched; they share the initialized singleton.)
- The `source` export condition on our workspace packages lets
  esbuild compile `quarto-sync-client`/`quarto-automerge-schema`
  straight from TS sources — the bundle can never embed a stale
  workspace `dist/`.
- esbuild hoists the entry's shebang above any banner — don't put a
  shebang in the banner (caused a syntax error on first attempt).
- **Verified end-to-end against a local hub** (standalone bundle in
  `~/.q2-mcp-bundle-spike`, hub at `ws://127.0.0.1:3941/ws`):
  MCP initialize + tools/list; `create_project` (index doc
  `33eeA9AcTQLdSrvLJ35Y1gybursu`); `connect_project`; `patch_file`
  with multibyte content (`Ünïcödé ✓ 🎉`); `read_file` round-trip;
  and a **fresh process** re-read proving hub-side persistence.
  Tool calls are handled concurrently — clients must await
  `connect_project` before issuing file ops.
- **Two pre-existing TS-server bugs discovered** (filed as strands,
  fixed under this plan since both gate `q2 mcp` quality):
  - **bd-sl4o01y0** — `quarto-sync-client` has 12 `console.log`
    sites ("Waiting for peer connection...", `[createNewProject]`,
    …) that land on **stdout**, corrupting the MCP protocol stream.
    hub-mcp itself has zero.
  - **bd-9jq2a060** — the server exits on SIGINT/SIGTERM but not on
    stdin EOF once a sync websocket/retry timer is alive; MCP hosts
    terminate servers by closing stdin, so every session leaks a
    node process. Confirmed: still running 4 s after stdin EOF.
- Observation (not yet filed): a first "Peer connection failed,
  continuing in offline mode (Timeout)" line sometimes precedes a
  successful connect — looks like first-attempt timeout noise with
  multiple Repo instances; revisit during Phase 3 if it persists.
- **Live playground for later phases (from Carlos):** index doc
  `SNHcgVzUkWpGFmcxkCkpCDfFtmu` on quarto-hub.com — follow the
  conventions of the existing files there.

### Phase 1 — bundling, owned by the TS package

- [x] Tests: CI-runnable smoke test (`src/bundle.test.ts`) — builds
      the bundle, copies it to `os.tmpdir()` (outside the repo tree),
      and drives it with plain node: artifact checks, `--help`, full
      MCP round-trip (create/connect/patch/read with multibyte
      content) against an in-process sync peer (`src/test-hub.ts`),
      stdout purity, stdin-EOF exit.
- [x] `npm run bundle` in `ts-packages/quarto-hub-mcp`
      (`scripts/bundle.mjs`: esbuild, automerge-base64 steering, mini
      node_modules for the keyring addon, build-info.json stamp).
      License-notice generation deferred to the npx-channel strand
      (bd-3tak0lyy) — the q2-embedded copy inherits q2's existing
      third-party-notice obligations, tracked there.
- [x] `cargo xtask build-hub-mcp-bundle`; wired into `build-all`
      (before the Rust build) and `verify` (new Step 11 also runs the
      sync-client + hub-mcp vitest suites, which verify previously
      didn't run at all; `--skip-hub-mcp-*` flags opt out).
      CLAUDE.md rebuild-chain documentation deferred to Phase 2 — it
      documents the embed, which doesn't exist until `q2 mcp` lands.

**Phase 1 discoveries:** three pre-existing TS-server bugs found and
fixed TDD along the way — bd-sl4o01y0 (sync-client `console.log`
corrupts MCP stdout; injectable `setSyncLogger` seam + stderr redirect
+ `console.log` rebind), bd-9jq2a060 (no exit on stdin EOF; the SDK's
StdioServerTransport never watches EOF — explicit `stdin 'end'`
watcher + guarded shutdown; hub-mcp test suite got 3× faster as a side
effect), bd-2d8ur7e9 (entry-module guard failed under symlinked
invocation paths because Node canonicalizes `import.meta.url` but
argv[1] was compared verbatim — macOS `/tmp`→`/private/tmp` and npm
`.bin` shims both hit this; `realpathSync` fix; this one would have
broken the npx channel outright).

### Phase 2 — the Rust launcher (CORE COMPLETE 2026-06-11; see notes)

- [x] Tests (`crates/quarto-mcp-launcher/tests/integration/`, 26
      passing): extraction payload+metadata, lifetime shared lock,
      idempotent reuse, 8-thread concurrency convergence (caught a
      real .tmp-name collision bug — fixed with an atomic uniquifier),
      corrupt-dir self-heal, GC age/lock/keep-hash/leftover semantics,
      crash-release collectability, re-extract after GC; QUARTO_NODE
      override (wins, and too-old is an error not a fallthrough),
      PATH lookup, floor fallthrough to well-knowns, actionable
      not-found error. Exit-code forwarding (Windows spawn path) has
      no unit test — needs a Windows CI run to exercise at all.
- [x] Lock-survives-exec + stdout purity + e2e handshake: verified
      against the real binary (see e2e evidence below); these are
      properties of the exec'd process, untestable in-process.
- [x] `q2 mcp` clap wiring (`disable_help_flag` + trailing varargs →
      verbatim pass-through; TS server owns `--help`); new crate
      `quarto-mcp-launcher` (bundle embed + hash, cache/locks/GC, node
      discovery, exec/spawn delegate incl. FD_CLOEXEC clearing);
      `--launcher-info`.
- [x] `build.rs` guard: placeholder embed + cargo warning on missing
      bundle (house style from quarto-preview — build succeeds, fresh
      clones work); `q2 mcp` errors at runtime pointing at
      `cargo xtask build-hub-mcp-bundle`. CLAUDE.md rebuild-chain
      section added (deferred Phase 1 item).

**Phase 2 e2e evidence (2026-06-11, per CLAUDE.md e2e policy).** All
through `./target/debug/q2` against a local hub
(`target/debug/hub --data-dir … -P 3941`):

1. `q2 mcp --launcher-info` → printed bundle-hash
   `3afafa38242075cc`, embedded build-info (git commit + dirty +
   builtAt + node24), cache root
   `~/Library/Caches/quarto/hub-mcp`, discovered node
   `/opt/homebrew/bin/node (v24.15.0)`.
2. `q2 mcp --help` → the TS server's usage on stderr (delegation
   works end-to-end: extract → discover → exec).
3. MCP session `q2 mcp --server ws://127.0.0.1:3941/ws` with
   initialize + `create_project` → response
   `{"indexDocId":"2rgwrw1rasNBHoBEwbyKSgh87rKk","files":["via-q2.qmd"]}`;
   stdout pure JSON-RPC (jq parsed every line).
4. Lock property, observed live: while a `q2 mcp` session ran,
   `flock(LOCK_EX|LOCK_NB)` on the cache `.lock` from another process
   **failed** (lock survived the exec into node); after the session
   exited it **succeeded** (kernel release). Output inspected.

Note for test authors: a `printf … | q2 mcp` one-liner closes stdin
immediately, and the server (correctly, bd-9jq2a060) shuts down on
EOF, racing in-flight tool calls — hold stdin open (real MCP host
behavior) when driving sessions by hand.

### Phase 3 — auth + hub e2e through the launcher

- [ ] Tests: against a local auth-enabled hub (reusing
      `crates/quarto-hub/tests/auth_bearer.rs` setup patterns):
      `authenticate` round-trip through `q2 mcp`, keyring reuse across
      launcher/npx channels (same credential visible to both), 401 →
      refresh path unaffected by the launcher.
- [ ] Default `--server` URL (`wss://quarto-hub.com/ws`, resolved
      open question 3) added in the TS server with its own test.

### Phase 4 — docs, publishing, verification

- [ ] README + docs/ user-facing page: Claude Desktop / Claude Code /
      Cursor config snippets for the `q2 mcp` channel (npx channel
      docs land with bd-3tak0lyy); Node requirement stated plainly.
- [ ] ~~npm publish pipeline~~ — moved to follow-up strand
      **bd-3tak0lyy** (gated on public-release readiness).
- [ ] **End-to-end verification (CLAUDE.md policy):** real
      `q2 mcp` configured in a real agent app on this machine;
      authenticate interactively against quarto-hub.com; edit a real
      project; observe the edit live in hub-client in a browser with
      correct attribution; record exact invocation + observed output
      here.

## Rust-native findings (v1 research, kept for the record)

If the Node dependency ever becomes unacceptable, the native port is
feasible; the v1 investigation established:

- samod 0.9 (our fork) supports outbound client connections —
  `Repo::dial_websocket` / `TungsteniteDialer`
  (`samod/src/websocket.rs:155-233`) with reconnect+backoff. The
  built-in dialer can't set headers, but the `Dialer` trait is public
  and `connect()` runs per reconnect attempt, so a custom
  `BearerDialer` gets refreshed-token pickup for free. No fork changes
  needed.
- The hub's Bearer path is fully built and tested server-side
  (`crates/quarto-hub/src/auth.rs`, 33 tests in `tests/auth_bearer.rs`);
  per-project actor IDs come from `GET /auth/actor?project=<id>`.
- `rmcp` is the official Rust MCP SDK (stdio supported).
- The index-doc schema lives in `crates/quarto-hub/src/index.rs`; a
  native client would want it extracted into a shared model crate.
- The expensive part is re-implementing and *maintaining* the auth +
  connection threat model in a second language — the reason v2 won.

## References

- TS implementation: `ts-packages/quarto-hub-mcp/` (server),
  `ts-packages/quarto-sync-client/` (automerge sync + `ws` Bearer
  adapter)
- Auth plans: [`2026-05-05-hub-mcp-device-flow-implementation.md`](2026-05-05-hub-mcp-device-flow-implementation.md)
  (superseded), [`2026-05-28-hub-mcp-loopback-pkce.md`](2026-05-28-hub-mcp-loopback-pkce.md)
  (current; Phase 3 device-flow removal pending)
- Original MCP design: [`2026-03-13-hub-mcp-server-design.md`](2026-03-13-hub-mcp-server-design.md)
- Embed precedent + stale-artifact post-mortem:
  `claude-notes/instructions/preview-spa-rebuild.md`
- Hub auth: `crates/quarto-hub/src/auth.rs`, `src/server.rs`,
  `tests/auth_bearer.rs`
- Deployment: `~/repos/github/quarto-dev/quarto-hub-deployment`
