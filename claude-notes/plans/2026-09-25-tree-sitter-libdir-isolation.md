# Isolate tree-sitter's compiled-grammar cache per checkout (bd-agsgrbfn)

**Date:** 2026-09-25
**Braid:** bd-agsgrbfn
**Branch:** `braid/bd-agsgrbfn-tree-sitter-libdir-isolation` (topic branch in the main checkout, based on `main` @ `ce01489c4`)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

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

## Proposed phases (draft)

- **Phase 0 — Tests first.**
  - Add a unit test for a new helper (e.g.
    `util::tree_sitter_command(project_root) -> Command`). It asserts
    that `TREE_SITTER_LIBDIR` is set to `<project_root>/target/tree-sitter-lib`,
    using the same `env_overrides(&cmd)` pattern as
    `nested_command_strips_pkg_vars` in `util.rs`. It fails until the
    helper exists.
  - Optionally add an integration-style check: run `tree-sitter test`
    through the helper and assert that the dylib appears under
    `target/tree-sitter-lib`. This depends on the CLI being on PATH, so
    it may only be worth doing as a manual e2e step.
- **Phase 1 — xtask.** Route `verify.rs` step 4 and
  `treesitter_crlf::run_parity_check` through the helper. Pass
  `project_root` into the CRLF check, or use a separate subdirectory for
  CRLF; see Q3.
- **Phase 2 — Hand-run path.** Depends on Q1: a documented `export`,
  a checked-in `.envrc`, and/or a `cargo xtask ts-test` wrapper.
- **Phase 3 — Doctemplate grammar.** Depends on Q2.
- **Phase 4 — Docs.** Update the tree-sitter bullet at `AGENTS.md:647`
  and say where the cache now lives and why.
- **E2E.** Poison the shared cache on purpose, e.g. copy a dylib built
  from a modified `parser.c` into `~/.cache/tree-sitter/lib/markdown.dylib`
  and `touch` it. Then show that `cargo xtask verify` step 4 still
  passes.

## Open design questions for the user

1. **Hand-run coverage (Q1).** How should hand-run `tree-sitter test`
   (AGENTS.md:647) be isolated? Options:
   (a) AGENTS.md only: document `export TREE_SITTER_LIBDIR=$PWD/target/tree-sitter-lib`;
   (b) a checked-in `.envrc` for direnv users, plus (a);
   (c) a `cargo xtask ts-test [--grammar qmd|doctemplate] [--rebuild]`
   wrapper that runs generate/build/test with the right env, with
   AGENTS.md pointing agents at it instead of raw `tree-sitter`.
   My lean is (c) + (a). Agents follow AGENTS.md literally, and a wrapper
   makes the safe path the easy one. direnv isn't universal.
2. **Doctemplate scope (Q2).** Should this strand only isolate the cache
   for doctemplate's hand-run tests? Or should it also add doctemplate
   corpus tests to `verify` step 4, which don't run today? Adding them
   widens the scope and needs a CI counterpart per the verify/CI sync
   rule. That probably deserves its own strand.
3. **CRLF run's libdir (Q3).** Should the CRLF parity run share
   `target/tree-sitter-lib` with the LF run? It's the same grammar name
   and the same `parser.c` contents, but the copied files have
   different mtimes. Or should it get its own `target/tree-sitter-lib-crlf`
   (or a dir inside its tempdir) so it always compiles fresh? A private
   dir costs one extra grammar compile per verify (a few seconds) and
   can never be confused.
4. **Directory choice.** Is `<project_root>/target/tree-sitter-lib` the
   right place? It is per-checkout, gitignored, and wiped by
   `cargo clean`. The main risk is a user-set `CARGO_TARGET_DIR`
   pointing outside the checkout. Should we respect `CARGO_TARGET_DIR`
   (and risk sharing again if it's shared across worktrees), or always
   use the literal `<root>/target`? I lean toward the literal path. Your
   memory notes that shared target dirs across worktrees are already
   discouraged.
5. **Respect a user-set `TREE_SITTER_LIBDIR`?** If the caller already
   exported one, should xtask override it (always isolated) or keep it
   (the user knows best)? I lean toward always overriding, because a
   globally exported value would bring the bug back.

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
