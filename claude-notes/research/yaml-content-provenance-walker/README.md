# The lockstep walker prototype + fixture generator

`walker.rs` is the prototype that settled the derivation design for
`claude-notes/plans/2026-08-20-provenance-1-foundations.md`, and the generator
that produced `../2026-08-21-yaml-content-provenance-fixtures.md`. It is
committed because the fixtures are the plan's stated authority for Phase 2's
expected values, and an authority whose producer was thrown away cannot be
extended or re-checked.

Not production code: no `SourceInfo` output, no `SourceContext`, partial escape
table.

## Running it

It needs a path dependency on the `quarto-yaml` checkout, so it lives outside
the workspace. From a scratch directory:

```toml
# Cargo.toml
[package]
name = "walker"
version = "0.0.0"
edition = "2021"

[dependencies]
quarto-yaml = { path = "/Users/gordon/src/quarto-yaml/crates/quarto-yaml" }
yaml-rust2 = "0.11"
```

```bash
cp walker.rs src/main.rs && cargo run -q > fixtures-table.md
```

Output is the markdown tables in the fixtures note, so regenerating is a copy.

## Extending it

`emit` always looks a scalar up under `get_hash_value("k")`. Some shapes need
a different node — a hash **key** (`y.as_hash()`, then `entries[i].key`), a
**flow-collection item** (`v.as_array()`, then `items[i]`) — or a decoded
value `Yaml::as_str()` can't recover (`Yaml::Null`, `Yaml::Boolean`, whose
`as_str()` is `None` even though the event carried a value string). `emit_node`
is `emit`'s core with the node and the value made explicit parameters
(`val_override`, used only for the two non-string cases), so a new shape is
usually a few lines that resolve the right node and call `emit_node` directly
instead of `emit`. Added 2026-08-22 (Task 9) for the plan's seven missing
cases: `k: ~`, `k: true`, a quoted key, both items of a flow collection, a
tagged scalar, an unescaped double-quoted scalar, and `\n` as an escape.

## The two rules that are easy to get wrong

**`verbatim` is decided by byte-identity, never by length.** An earlier version
coalesced any length-preserving piece into the preceding run. That is unsound: a
fold whose source run is exactly `\n` and whose content is one space is 1→1 with
*different bytes*, and merging it into a verbatim run produces a piece claiming a
byte-identical source range it does not have — which any caller that mistook
`preimage_in`'s hull for a byte-identity claim would then Verbatim-copy,
emitting a newline where the content has a space. Reachable in the simplest possible document: `aaa⏎bbb` as a root-level
plain scalar. All 24 shapes measured before the fix happened to be unaffected
(block tails genuinely *are* byte-identical newline runs), so no fixture row
caught it.

**Zero-content pieces are stored, never dropped.** The piece list must tile its
source contiguously; dropping a deletion leaves a gap exactly where the deleted
bytes were, which `preimage_in` then reports as `None` instead of a hull.
Measured: dropped → `preimage_in` `None`; stored → `Some(4..14)`,
with byte-identical offset mapping either way. An earlier revision dropped them,
on a rationale that only held against an unfixed `Concat` exclusive-end branch.
