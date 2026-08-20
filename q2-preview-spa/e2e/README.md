# Running the `q2 preview` end-to-end tests

These Playwright specs drive the **real `q2 preview` binary** (embedded SPA +
WASM) in a real Chromium and assert on the live preview pane. They are the
top of the preview test ladder — everything below them (unit, jsdom,
render-tier integration) lives elsewhere.

## 1. Build a fresh binary (the `include_dir!` trap)

The binary **embeds** the SPA bundle and the WASM at compile time. A plain
`cargo build --bin q2` re-embeds whatever was last built — after any Rust
change under `quarto-core`/`pampa`/`wasm-quarto-hub-client`, or any SPA
change, you must run the full chain, in order, from the repo root:

```bash
cd hub-client && npm run build:wasm && cd ..   # 1. WASM from current Rust
cargo xtask build-q2-preview-spa               # 2. bundle WASM+SPA into q2-preview-spa/dist/
cargo build --bin q2                           # 3. re-embed dist/ via include_dir!
```

Skipping 1–2 embeds stale artifacts; skipping 3 leaves the binary stale.
Deep background + staleness diagnosis:
`claude-notes/instructions/preview-spa-rebuild.md`.

**Build the default (debug) profile.** The harness hardcodes
`<repo-root>/target/debug/q2` (`e2e/helpers/globalSetup.ts`) and asserts it
exists before any spec runs — a `--release` build will not be picked up.

## 2. One-time setup

```bash
npm install                      # from the REPO ROOT (npm workspaces — never inside subdirs)
npx playwright install chromium  # browser binary (from q2-preview-spa/)
```

## 3. Run the suite

From `q2-preview-spa/`:

```bash
# Default suite — fast; live-engine specs skip
npx playwright test --project=chromium

# One spec
npx playwright test basic-preview --project=chromium
```

## 4. Live-engine specs (opt-in env flags)

Specs that spawn a real language engine are opt-in so the default suite
stays fast and environment-independent:

| Flag | Spec | Engine | Notes |
|---|---|---|---|
| `QUARTO_PC6_LIVE=1` | `engine-capture-splice-julia` | julia | multi-second server boot; needs `julia` on PATH |
| `QUARTO_SC21_LIVE=1` | `engine-capture-splice-marimo` | marimo (via `uv`) | needs `deno` + `uv`; ~10s warm. **This spec is a limitation CANARY**: it asserts marimo output does *not* reach the pane (plan-4c FINDING #5, strand `bd-5jxcio5d`) and is expected to redden — then be flipped to a positive test — when that strand's fix lands. See the spec header. |

```bash
QUARTO_PC6_LIVE=1 npx playwright test engine-capture-splice-julia --project=chromium
QUARTO_SC21_LIVE=1 npx playwright test engine-capture-splice-marimo --project=chromium
```

(`QUARTO_SC21_REVERT=1` additionally runs the marimo canary's binding-proof
revert mode — normally only used when re-verifying the seam.)

The echo-engine spec (`engine-capture-splice.spec.ts`) needs no flag — it is
the always-on CI guard for the capture-delivery chain.

## 5. CI

`cargo xtask verify --e2e` runs this suite as its final step (without the
opt-in flags). The default `cargo xtask verify` skips it.
