# Lua marshaling conformance suite

This directory holds Quarto 2's Pandoc-Lua-API conformance suite: the
test corpus that pins down how closely pampa's Lua marshaling layer
(constructors, coercions, properties, list types) matches real
Pandoc's. It is the "Track 1" harness of
`claude-notes/plans/2026-07-13-lua-api-pandoc-parity.md` (strand
bd-grkrb9nj).

## Layout

| Path | What |
|---|---|
| `upstream/test-*.lua` | Pandoc's own marshaling tests, vendored **unmodified** from [pandoc/pandoc-lua-marshal](https://github.com/pandoc/pandoc-lua-marshal) |
| `tasty.lua` | The pure-Lua test runner those files `require`, vendored **unmodified** from [hslua/hslua](https://github.com/hslua/hslua) `tasty-lua/tasty.lua` |
| `prelude.lua` | Q2-side environment adapter (see below) — ours |
| `xfail.txt` | Expected-failure list (the parity scoreboard) — ours |

Runner: `crates/pampa/tests/integration/lua_conformance.rs`.

## Vendored versions

Update these lines when re-vendoring (copy from
`external-sources/pandoc-lua-marshal` / `external-sources/hslua`;
never reference `external-sources/` from the tests themselves):

- pandoc-lua-marshal: commit `c2dc4e117766d1bb1a8d036f9e0c52d6ee8574c9` (2026-04-21)
- hslua (tasty.lua): commit `82c983a91e1750a29357b6c80f3e0757cdd258ba`

Both projects are MIT-licensed (© Albert Krewinkel and contributors);
the vendored files retain their upstream content verbatim.

## How it works

Upstream files follow the tasty-lua convention: executing the file
*runs* the tests (each `test_case` pcall-executes its callback at
tree-construction time) and returns a tree of
`{name = ..., result = true | error-string | nested-list}` nodes.

The upstream Haskell driver (`test-pandoc-lua-marshal.hs`,
`registerDefault`) exposes every constructor as a **bare global**
(`Str`, `Div`, `Attr`, `Blocks`, …), the `List` module as a global,
and every enum constant as a global holding its own name as a
**string** (`AlignLeft = 'AlignLeft'`, `SingleQuote = 'SingleQuote'`,
…). `prelude.lua` replicates exactly that environment on top of the
`pandoc` table that `create_filter_environment` provides, and preloads
`tasty`.

The Rust runner builds the **production filter environment** (the same
`create_filter_environment` used by `apply_lua_filter` — deliberately
not a synthetic registration, so conformance measures what real
filters see), runs prelude + test file, flattens the result tree into
`Group / Subgroup / test name` ids, and checks them against
`xfail.txt`.

## The xfail ratchet

`xfail.txt` lists tests that are *known* not to pass yet, one id per
line, with `# reason` comments allowed (line-level or standalone).
The runner fails on:

- an **unexpected failure** — a test not in `xfail.txt` fails
  (regression), and
- an **unexpected pass** — a test in `xfail.txt` passes (progress!
  delete the line; the ratchet only tightens).

Fixing a mismatch therefore always shrinks `xfail.txt`, and the file
doubles as the live mismatch catalog. Deliberate divergences (places
where q2 intentionally differs from Pandoc — see the plan's
divergence-registry track) must carry a `# DIVERGENCE:` comment
explaining and pointing at the registry entry.

## Adding coverage

- New upstream files: copy from `external-sources/pandoc-lua-marshal/test/`,
  add the filename to `UPSTREAM_FILES` in the runner, run, and append
  the new failures to `xfail.txt`.
- Q2-specific regression cases: prefer the Track-2 differential
  corpus (oracle: real `pandoc`), not this directory — this directory
  stays byte-identical to upstream so re-vendoring is a plain copy.
