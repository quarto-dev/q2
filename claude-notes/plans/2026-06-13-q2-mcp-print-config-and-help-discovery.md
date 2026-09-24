# q2 mcp: `--print-config` + `--help` launcher-options discovery (bd-9a8yu2gw)

## Overview

`q2 mcp` is a deliberately *thin* launcher (bd-81cfshmw): clap captures
everything after `mcp` into a trailing-var-arg vec
(`crates/quarto/src/main.rs:450`, `disable_help_flag = true`) and
`quarto-mcp-launcher::run` passes it **verbatim** to the embedded
TypeScript server, intercepting exactly one launcher meta-flag today:

```rust
if args.len() == 1 && args[0] == "--launcher-info" { … }
```

Two gaps fall out of that design:

1. **No self-documenting config affordance.** There is no example
   anywhere in code/docs of a `.mcp.json` entry that drives `q2 mcp`.
   The one committed `.mcp.json` modelled the *old* `command: node …
   dist/index.js` path. The ecosystem-standard fix (cf. FastMCP's
   `install mcp-json`, braid's `agents-info`) is a command that prints
   the canonical, version-matched config snippet to stdout.

2. **Launcher flags are invisible at `q2 mcp --help`.** Because `--help`
   is part of the verbatim passthrough, it reaches Node and prints
   *only* the server's own usage (`--server`, `--read-only`,
   `--redirect-port`). `--launcher-info`, the new `--print-config`, and
   the `QUARTO_NODE` / `QUARTO_MCP_CACHE_DIR` env vars never appear. They
   are documented only in the clap long-about, reachable via the obscure
   `q2 help mcp` — not the `q2 mcp --help` everyone actually types.

## Design decisions

- **`--print-config`, not a `config` subcommand.** The passthrough has
  no subcommands, and a bare word (`config`) as `args[0]` would be
  ambiguous with a positional meant for the server. A `--flag` cannot
  collide with the server's args and sits as a natural sibling of
  `--launcher-info`. Output is **pure JSON on stdout** so it pipes
  (`q2 mcp --print-config > .mcp.json`); any prose goes to stderr or is
  omitted.
- **`--print-config` follows the sole-arg rule**, exactly like
  `--launcher-info`: honored only when it is the single argument.
  Combined with other args it falls through to the server (which will
  reject the unknown flag) — acceptable, since it is always invoked
  alone.
- **`--help` / `-h` is intercepted if present *anywhere* in args** (not
  sole-arg), because `q2 mcp --server x --help` should still show help.
  The launcher prints a short launcher-options **preamble** to stdout,
  then *still delegates* `--help` to the server so both halves show. On
  Unix the launcher's stdout flushes before `exec`, so ordering is
  clean: launcher section first, then the server's `Usage:` block.
  stdout is correct here for the same reason as `--launcher-info`:
  `--help` is never a live MCP session.
- **Testable seam.** Extract a pure `classify_args(&[String]) ->
  LauncherAction` classifier and pure string builders
  (`config_snippet()`, `help_preamble()`) so the interception logic is
  unit-testable without a real Node or bundle. Expose them `pub` and
  test via the crate's integration-test harness, matching the existing
  `node`/`defaults` module style.

## Test plan (TDD — write first, watch fail)

`crates/quarto-mcp-launcher/tests/integration/args_tests.rs`
(registered in `tests/integration/main.rs`):

- [ ] `classify_args` → `LauncherInfo` for `["--launcher-info"]` only;
      falls through when combined with other args.
- [ ] `classify_args` → `PrintConfig` for `["--print-config"]` only;
      falls through when combined.
- [ ] `classify_args` → `HelpPreambleThenDelegate` when `--help` or `-h`
      appears anywhere (`["--help"]`, `["--server","x","--help"]`,
      `["-h"]`).
- [ ] `classify_args` → `Delegate` for ordinary server args
      (`["--server","wss://…"]`, `[]`).
- [ ] `--help` + `--launcher-info` together → `HelpPreambleThenDelegate`
      (help wins; launcher-info's sole-arg rule isn't met).
- [ ] `config_snippet()` parses as JSON and has
      `mcpServers.quarto-hub.command == "q2"` and `args == ["mcp"]`.
- [ ] `help_preamble()` names `--launcher-info`, `--print-config`,
      `QUARTO_NODE`, and `QUARTO_MCP_CACHE_DIR`.

## Work items

- [x] Plan + strand (this doc, bd-9a8yu2gw)
- [x] Write failing integration tests (`args_tests.rs`) — confirmed red
      (unresolved imports) before implementing
- [x] Implement `LauncherAction`, `classify_args`, `config_snippet`,
      `help_preamble`; rewire `run`
- [x] Update the `Mcp` doc-comment in `crates/quarto/src/main.rs` to
      mention `--print-config`
- [x] Tests pass; `cargo nextest run -p quarto-mcp-launcher` (42/42)
- [x] End-to-end through the real binary (see record below)
- [x] README: add the `{command:q2,args:[mcp]}` stanza
- [\~] Full `cargo xtask verify` — see verification status below
      (Rust legs green; hub/WASM legs blocked by pre-existing
      environmental state in this checkout, unrelated to this diff)

## End-to-end verification record

Built `cargo build --bin q2`, then exercised the real binary
(`target/debug/q2`):

**`q2 mcp --print-config`** — pure JSON on stdout, redirected and
re-parsed with `jq` (valid JSON):

```json
{
  "mcpServers": {
    "quarto-hub": {
      "command": "q2",
      "args": ["mcp"]
    }
  }
}
```

**`q2 mcp --help`** — launcher preamble printed first, then the embedded
server's own usage (both halves visible):

```
q2 mcp — launch the Quarto Hub MCP server (embedded; needs Node.js).

Launcher options (handled by q2 before the server starts):
  --launcher-info       Print embed/cache/node diagnostics and exit
  --print-config        Print a .mcp.json entry for this server and exit
  --help, -h            Show this help (launcher options, then server options)

Launcher environment variables:
  QUARTO_NODE           Path to the Node.js binary (when PATH discovery fails)
  QUARTO_MCP_CACHE_DIR  Override the extracted-bundle cache location

Embedded MCP server options:
Usage: quarto-hub-mcp [--server <url>] [--read-only] [--redirect-port <N>]
  --server / --read-only / --redirect-port / --help, -h
```

(A pre-existing Node stderr line, `using deprecated parameters for
initSync()…`, prints between the header and the server `Usage:` block;
not introduced by this change.)

**`q2 mcp --launcher-info`** — regression check, still prints
embed/cache/node diagnostics unchanged. Output inspected.

## Verification status (2026-06-13)

This is a **Rust-only** change; `quarto-mcp-launcher` is not in the
`wasm-quarto-hub-client` dependency chain, so the WASM/hub legs cannot
be affected by it.

Green (legs this diff can affect):
- `cargo build --bin q2` — clean.
- `cargo nextest run -p quarto-mcp-launcher` — 42/42 (incl. 7 new).
- `cargo build --workspace` + `cargo nextest run --workspace` — passed
  (xtask verify reached the hub legs, which run after them).
- `cargo clippy -p quarto-mcp-launcher --lib` — **zero warnings in the
  edited `lib.rs`**. (The crate's `--all-targets`/`-D warnings` run trips
  on pre-existing debt in *untouched* files: `build.rs:61`,
  `cache.rs:240`, `node.rs:170` — the last is a clippy lint *rename*
  notice, i.e. local clippy is newer than the pinned toolchain.)
- End-to-end through the real binary — recorded above.

Blocked by pre-existing environmental state (NOT this diff):
- `cargo xtask verify` hub-client test leg fails `ERR_MODULE_NOT_FOUND`,
  and the q2-preview-spa build fails on missing generated WASM artifacts
  (`crates/wasm-quarto-hub-client/pkg/`,
  `ts-packages/wasm-js-bridge/src/template.js`) — WASM was never built
  in this checkout. Reproducible independent of this branch.
- Whole-workspace `clippy -D warnings` trips on debt in `quarto-source-map`
  and `lua-src` (untouched). Out of scope; candidate for a separate
  chore strand.
