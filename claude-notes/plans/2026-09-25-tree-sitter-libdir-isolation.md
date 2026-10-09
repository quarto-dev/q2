---
title: 'Isolate tree-sitter''s compiled-grammar cache per checkout (bd-agsgrbfn)'
date: 2026-09-25
---

**Date:** 2026-09-25
**Braid:** bd-agsgrbfn
**Branch:** `braid/bd-agsgrbfn-tree-sitter-libdir-isolation` (topic branch in the main checkout, based on `main` @ `ce01489c4`)
**Status:** Design settled with user (2026-09-25); implementing.

## Triage verdict

**Ready to design.** The mechanism is understood, the fix (`TREE_SITTER_LIBDIR`)
is confirmed to work with the locally installed CLI, and the call sites
are few (two in xtask). The open questions are about how far to go
beyond xtask (hand-run `tree-sitter test`, CI).

## Issue context

P3 bug, filed today (2026-09-25) by Carlos, during
bd-angle-bracket-u27e8-parse-error-r6l55zmh work in room-5.

The tree-sitter CLI caches compiled grammars at
`$TREE_SITTER_LIBDIR/<grammar-name>.dylib`. When the variable is unset,
it uses `~/.cache/tree-sitter/lib`. The cache key is only the grammar
*name* (`markdown`). The cache counts as fresh when the dylib's mtime is
newer than `src/parser.c` / `scanner.c`. As a result, every q2 checkout
on a machine (all `rooms/room-*/q2` and every `.worktrees/*`) shares one
`markdown.dylib`. The checkout that compiled most recently wins.

In the observed failure, room-2 regenerated its parser after room-5 did.
room-5's `verify` step 4 then saw a dylib newer than its own `parser.c`,
reused room-2's grammar, and failed 9 corpus tests with nonsense trees.
`tree-sitter test --rebuild` only helps until the next foreign run.

## Dependency graph

- **discovered-from**: bd-angle-bracket-u27e8-parse-error-r6l55zmh
  (in_progress) — a grammar fix for U+27E8 `⟨` parse errors. That session
  was regenerating `parser.c` in room-5 while room-2 did the same, and
  hit this bug in `verify` step 4. The two strands are otherwise
  unrelated; this one does not block that one (a `--rebuild` is a
  workaround there).
- No `blocks` or `related` edges, and no parent epic. Nothing depends on
  this strand, so there is no pressure beyond "multi-room grammar work
  gives false failures (or, worse, false passes)."

## What the code looks like today

All paths in the description still exist:

- `crates/xtask/src/verify.rs:298` — step 4 calls
  `run_command("tree-sitter", &["test"], &ts_dir, None, …)`.
  `run_command` (`verify.rs:741`) builds the command with
  `crate::util::nested_command` and sets only `RUSTFLAGS`.
- `crates/xtask/src/treesitter_crlf.rs:34` — the CRLF parity run calls
  `Command::new("tree-sitter").arg("test")` in a tempdir copy of the
  grammar. The copy has the same grammar name, so it hits the same
  cache entry. (On macOS, `std::fs::copy` may keep the source mtimes,
  which makes the copied `parser.c` look older than a foreign dylib.)
- `crates/xtask/src/dev_setup.rs:32` only checks that the CLI exists.
  No other xtask runs tree-sitter.
- `AGENTS.md:647` tells agents to run
  `tree-sitter generate; tree-sitter build; tree-sitter test` by hand
  in `crates/tree-sitter-qmd/tree-sitter-markdown`. `tree-sitter test`
  there also uses the shared cache. (`tree-sitter build` writes
  `./markdown.dylib` into the grammar dir, which is gitignored. It is a
  different file from the cache.)
- The cargo build does not go through the CLI:
  `crates/tree-sitter-doctemplate/build.rs` and the qmd crate compile
  `parser.c` with `cc`. So the Rust crates are not affected. Only
  `tree-sitter test` / `parse` runs are.
- The doctemplate grammar is **not** tested by `verify` today. Its
  `doctemplate.dylib` in the shared cache comes from hand-run
  `tree-sitter test` in `crates/tree-sitter-doctemplate/grammar`.
- CI (`ts-test-suite.yml`, `build-wasm.yml`, `hub-client-e2e.yml`)
  installs tree-sitter v0.25.8. CI runners are single-checkout, so the
  bug can't happen there. Locally we have 0.26.8. Both versions honor
  `TREE_SITTER_LIBDIR`.

**Confirmed at HEAD (this session):**

```
$ cd crates/tree-sitter-qmd/tree-sitter-markdown
$ TREE_SITTER_LIBDIR=<scratch>/tslib tree-sitter test
Total parses: 721; successful parses: 721; failed parses: 0; ...
$ ls <scratch>/tslib
markdown.dylib
```

So tree-sitter 0.26.8 honors the variable and compiles fresh into the
new directory. `~/.cache/tree-sitter/lib/` on this machine currently has
`markdown.dylib` (17:12 today), `doctemplate.dylib` (Jul 15), and a
stray `probe.dylib`. These are exactly the shared entries the strand
describes.

I did not write a two-checkout repro. It needs two checkouts with
diverging `parser.c`, and the strand's room-5/room-2 observation already
documents the failure. The Phase 0 tests below cover the xtask side
mechanically.

## Decisions (user, 2026-09-25)

1. **Hand-run coverage:** (c) + (a). Add a `cargo xtask ts-test [--grammar qmd|doctemplate] [--rebuild] [-- <tree-sitter test args>]`
   wrapper (generate → build → test with the isolated libdir). Point
   AGENTS.md at it, and document the `export` for other hand runs
   (e.g. `tree-sitter parse`).
2. **Doctemplate in verify:** out of scope. Filed as bd-0njf3lab
   (discovered-from this strand). `ts-test --grammar doctemplate` does
   cover hand runs.
3. **CRLF run:** a private libdir inside the parity run's tempdir, so it
   always compiles fresh.
4. **Directory:** the literal `<project_root>/target/tree-sitter-lib`,
   deliberately *not* `CARGO_TARGET_DIR`. Leave a code comment explaining
   why, so future agents don't "fix" it.
5. **User-set `TREE_SITTER_LIBDIR`:** always overridden by xtask.

## Checklist

### Phase 0 — Tests first
- [x] Unit tests in a new `crates/xtask/src/tree_sitter.rs`:
  `libdir()` is `<root>/target/tree-sitter-lib`; `command()` sets
  `TREE_SITTER_LIBDIR` to it; the grammar dirs resolve to existing
  directories in the repo.
- [x] Confirmed they fail (E0425/E0433: helpers missing), then pass (3/3).

### Phase 1 — xtask plumbing
- [x] `tree_sitter::{libdir, command, Grammar}` helpers.
- [x] verify step 4 runs `tree-sitter test` through the helper.
- [x] CRLF parity run: grammar copy at `<tmp>/grammar`, private libdir at `<tmp>/lib`.

### Phase 2 — `cargo xtask ts-test`
- [x] Subcommand: `--grammar qmd|doctemplate` (default qmd), `--rebuild`,
  trailing args after `--` passed through to `tree-sitter test`.

### Phase 3 — Docs
- [x] AGENTS.md tree-sitter bullet: use `cargo xtask ts-test`; explain the
  shared-cache hazard and the `export` fallback.
- [x] xtask module doc list in `main.rs`.

### Phase 4 — Verification
- [x] E2E (below).
- [x] `cargo xtask verify --skip-hub-build --skip-hub-tests`: all steps passed (14999 Rust tests; step 4 LF + CRLF 721/721). The hub-client
  WASM tests fail at HEAD in this checkout only because
  `wasm_quarto_hub_client_bg.wasm` is stale (built Sep 18); this change is
  xtask-only and touches nothing hub-client depends on.

### E2E record (inspected)

I didn't want to touch the real `~/.cache/tree-sitter/lib`, which other
rooms share, so I pointed `HOME` at a scratch dir. That moves
tree-sitter's default cache to `$S/home/.cache/tree-sitter/lib`. I then
planted a grammar compiled from `206826fd6^` (before the flanking-rules
commit) in it:

```
$ cd <old grammar copy> && env -u TREE_SITTER_LIBDIR HOME=$S/home tree-sitter test
# -> $S/home/.cache/tree-sitter/lib/markdown.dylib (17:38)

# The bug, reproduced: bare `tree-sitter test` in this checkout uses the planted grammar
$ cd crates/tree-sitter-qmd/tree-sitter-markdown
$ env -u TREE_SITTER_LIBDIR HOME=$S/home tree-sitter test
Total parses: 721; successful parses: 678; failed parses: 43

# The fix; an exported TREE_SITTER_LIBDIR pointing at the poisoned dir is overridden
$ rm -rf target/tree-sitter-lib
$ HOME=$S/home CARGO_HOME=~/.cargo RUSTUP_HOME=~/.rustup \
    TREE_SITTER_LIBDIR=$S/home/.cache/tree-sitter/lib cargo xtask ts-test
TREE_SITTER_LIBDIR=/Users/cscheid/rooms/room-4/q2/target/tree-sitter-lib
Total parses: 721; successful parses: 721; failed parses: 0

$ <same env> cargo xtask verify --skip-<everything but step 4>
━━━ Step 4/14: Testing tree-sitter grammars ━━━
Total parses: 721; successful parses: 721; failed parses: 0
  ↳ Re-running with CRLF line endings...
Total parses: 721; successful parses: 721; failed parses: 0
  ✓ CRLF parity check complete
```

Afterwards, the planted `markdown.dylib` still had its 17:38 mtime, so
neither run read or rewrote it. `ts-test`\'s `tree-sitter generate` left
no tracked files modified.

## Risks / tradeoffs (draft)

- **Cold compile per checkout.** The first `verify` in each fresh
  checkout or worktree compiles the grammar once (the parser is large,
  so expect several seconds). After that it is cached per checkout.
  This is small next to the rest of `verify`.
- **Name collisions beyond q2.** The grammar is named `markdown`, so
  *any* other `markdown` grammar tested on the machine (e.g. upstream
  tree-sitter-markdown) also collides in the default cache. The per-checkout
  libdir fixes that too, as a side effect.
- **Windows.** The file would be `markdown.dll`, but the env var works
  the same way. Set paths with `Path::join`; no hardcoded separators.
- **Low urgency.** P3. It only bites when two checkouts regenerate the
  grammar close together, but then it gives false results that look
  real. A false *pass* is possible too, if the foreign grammar happens
  to accept the corpus.
