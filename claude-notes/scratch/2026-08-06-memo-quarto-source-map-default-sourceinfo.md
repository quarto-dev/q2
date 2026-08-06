# Memo: `SourceInfo::default()` fabricates a plausible-looking real location

**To:** whoever picks up `posit-dev/quarto-source-map`
**From:** q2 session on bd-9yh3pzfu / bd-2mxo (2026-08-06)
**Checkout:** `~/repos/github/posit-dev/quarto-source-map` @ `b61b447` (release-0.1.1)
**Consumer evidence:** `~/rooms/room-1/q2` (q2 monorepo)
**Scope:** small and self-contained. This is the *only* piece of the
bd-2mxo investigation that lives outside q2.

## The problem

`SourceInfo`'s `Default` impl returns a well-formed `Original` span:

```rust
// src/source_info.rs:139-147
impl Default for SourceInfo {
    fn default() -> Self {
        SourceInfo::Original {
            file_id: FileId(0),
            start_offset: 0,
            end_offset: 0,
        }
    }
}
```

Downstream this is **indistinguishable from a genuine span into file 0 at
offset 0**. It is not a sentinel — it is a claim, and a false one. A
diagnostic carrying it renders with a real-looking file, line, and caret
pointing at the first byte of whatever file happens to be `FileId(0)`.
Nothing anywhere can tell that apart from a true location, because there is
nothing to tell apart: the variant, the field values, and the rendering path
are all identical to the real thing.

The crate is already half-aware of this. `SourceInfo::default()` is
deprecated via an inherent method that shadows `Default::default()`
(`src/source_info.rs:149-168`), and its doc comment is explicit about the
remaining hole:

> This inherent method shadows `Default::default()` so that callers writing
> `SourceInfo::default()` see a deprecation error under `deny(deprecated)`.
> The trait impl is retained (and called by this method) so that
> `unwrap_or_default()` and `#[derive(Default)]` still compile; **those are
> caught by separate grep tooling.**

## Why this is worth doing now

**The grep tooling does not exist.** I looked for it on the consumer side,
where the crate's lints live: `q2/crates/xtask/src/lint/` contains exactly
two rules (`external_sources.rs`, `metadata_as_str.rs`), neither related.
There is no CI grep for `unwrap_or_default` on `SourceInfo` either. So the
shadowing trick catches the explicit call form and nothing else, and the
`unwrap_or_default()` escape hatch has been entirely unpoliced.

**That hole is load-bearing in a live q2 bug.** q2's config materializer
reaches for it at exactly the three sites at the heart of bd-2mxo:

- `q2/crates/quarto-config/src/materialize.rs:113` — materialized array
  container span
- `q2/crates/quarto-config/src/materialize.rs:154` — first-entry array
  fallback
- `q2/crates/quarto-config/src/materialize.rs:158` — materialized **map**
  container span

The user-visible symptom that started this: a listing diagnostic (`Q-12-7`)
whose message talks about `template:` while its caret underlines an
unrelated sibling key. The map-span synthesis is q2's bug to fix and we are
fixing it in-tree — but the reason a bogus fallback could travel that far
without tripping anything is that `Default` hands out something that looks
real.

**The consumer already has a written contract this violates.** q2's
`claude-notes/designs/provenance-contract.md` §10 opens its do-not list
with:

> **Don't emit `SourceInfo::default()` for new synthesized nodes.** ...
> `default()` survives in the Pandoc JSON reader by design (the source bytes
> genuinely don't exist there) and in test scaffolding; everywhere else it's
> a bug.

The contract and the crate agree on the intent. The type just doesn't
enforce it.

## Recommendation

**Change what `Default` returns rather than removing the impl:**

```rust
impl Default for SourceInfo {
    fn default() -> Self {
        SourceInfo::Generated { by: By::unknown(), from: SmallVec::new() }
    }
}
```

Why this shape:

- **It uses a concept the contract already sanctions.** `By::unknown()`
  exists and is documented in the provenance-contract table as the "We don't
  know" placeholder (currently used by
  `json::read_completing_source_info` for nodes deserialized without `s:`).
  This is not a new idea, just applying the existing one to the default.
- **It closes the hole without a type-level break.** Every
  `unwrap_or_default()` and `#[derive(Default)]` keeps compiling. The value
  simply stops lying: it renders as "no known location" instead of
  fabricating file 0 offset 0. That converts a silent wrong-caret into a
  visible absent-caret, which is the behavior you want from a fallback.
- **It is strictly better than the status quo even if nothing else
  changes.** No consumer migration is required for the improvement to land.

Removing the `Default` impl outright is the more principled end state and
worth considering, but it is a genuinely breaking change with an unmeasured
ripple through `#[derive(Default)]` on containing structs. I'd sequence it
after the above, if at all.

## What to check before landing

I did not verify these; they're the risks I'd want closed.

1. **Does anything pattern-match `Default`'s output expecting `Original`?**
   Grep the crate and consumers for matches on `SourceInfo::Original` that
   could receive a defaulted value.
2. **Tiling / ordering logic.** q2's Plan 7g has a "tiling precondition"
   over source ranges. If any of it assumes a defaulted `SourceInfo` is
   `Original` and therefore orderable by offset, a `Generated` default
   changes that. Search q2 for tiling code that consumes container spans.
3. **Serialization.** `Generated` serializes with `by`/`from` rather than
   offsets; confirm the wire format and any snapshots that currently
   round-trip a defaulted value.
4. **`for_test()`.** Confirm the test-scaffolding path (`SourceInfo::for_test()`)
   is unaffected — tests that want a real-looking span should keep using it
   explicitly.

## Complementary work that is *not* yours

For completeness, so the two sessions don't collide:

- The **map/array container-span synthesis** in `materialize.rs` is q2's
  bug (bd-2mxo), being fixed in-tree in `crates/quarto-config`. It needs no
  change to quarto-source-map, and no change to quarto-yaml (which already
  emits correct key spans).
- The **missing lint** for `unwrap_or_default()` on `SourceInfo`-typed
  expressions belongs in q2's `crates/xtask/src/lint/`, not in this crate —
  that's where the "separate grep tooling" the doc comment promises should
  have lived. Filing it on the q2 side.
