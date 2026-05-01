# QMD writer drops shortcode arguments and delimiters

**Beads:** bd-ylig

## Bug

User-reported round-trip failure:

```
$ printf '{{< video https://youtu.be/abc width="800" height="450" >}}\n' \
    | cargo run --bin pampa -- -t qmd
{{video}}
```

Expected: the writer should emit a shortcode that round-trips (semantically) to the original — i.e. with the `{{<` / `>}}` delimiters, the positional URL argument, and the named `width`/`height` attributes. Instead the writer emits `{{video}}`, losing every part of the shortcode except its name.

## Root cause

`crates/pampa/src/writers/qmd.rs:1716-1722`:

```rust
fn write_shortcode(
    shortcode: &crate::pandoc::Shortcode,
    buf: &mut dyn std::io::Write,
    _ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    write!(buf, "{{{{{}}}}}", shortcode.name)
}
```

The writer emits only `{{` + name + `}}`. It ignores `is_escaped`, `positional_args`, and `keyword_args`. The format string is also wrong — even for a name-only shortcode, qmd syntax requires `{{< name >}}`, not `{{name}}`.

The parser side is fine. `process_shortcode` (`crates/pampa/src/pandoc/treesitter_utils/shortcode.rs:48`) populates the `Shortcode` struct (defined in `crates/quarto-pandoc-types/src/shortcode.rs`) with all the data we need:

```rust
pub struct Shortcode {
    pub is_escaped: bool,
    pub name: String,
    pub positional_args: Vec<ShortcodeArg>,
    pub keyword_args: HashMap<String, ShortcodeArg>,
    pub source_info: SourceInfo,
}
```

The JSON / native writers also work correctly: they both convert `Inline::Shortcode` to a `Span` via `shortcode_to_span` (`crates/pampa/src/pandoc/shortcode.rs:56`), which preserves every argument as a child `Span` with `data-key` / `data-value` / `data-raw` attributes. Confirmed empirically — `pampa -t json` on the failing input keeps the URL and both attributes.

## Why no test caught this

The existing roundtrip suite (`crates/pampa/tests/test.rs:704` `test_qmd_roundtrip_consistency`, fixtures in `tests/roundtrip_tests/qmd-json-qmd/`) goes qmd → JSON → qmd. The JSON writer converts `Inline::Shortcode` → `Inline::Span` (via `shortcode_to_span`), so by the time the qmd writer runs in step 2, there is no `Inline::Shortcode` left for `write_shortcode` to mishandle. There are no fixtures under `qmd-json-qmd/` exercising shortcodes anyway, and no qmd snapshot fixture under `tests/snapshots/qmd/` either.

The unit test corpus in `tests/test_shortcode.rs` only covers parsing.

## Open design questions (resolved)

### Q1. Keyword-arg ordering — RESOLVED: option (b)

`Shortcode::keyword_args` is a `std::collections::HashMap<String, ShortcodeArg>`. HashMap iteration order is non-deterministic (different per run), which is a problem for any writer that wants stable output. Options:

- **(a) Sort alphabetically when writing.** Simple. Deterministic. Reorders the user's original ordering, e.g. `width="800" height="450"` round-trips to `height="450" width="800"`. Acceptable if we're only promising semantic round-trip, not byte-exact.
- **(b) Replace the storage with an order-preserving map** (e.g. `hashlink::LinkedHashMap` — already used in `Span` attrs in this codebase, or `indexmap::IndexMap`). Preserves insertion order from the parser, which matches source order. Touches the public-ish type in `quarto-pandoc-types`, so any consumer that builds a `Shortcode` (e.g. Lua filter constructors, JSON deserialization) needs an audit.
- **(c) Store an explicit `keyword_arg_order: Vec<String>` alongside the HashMap.** Less invasive than (b) but uglier; two sources of truth.

**Decision (user, 2026-04-30):** switch `Shortcode::keyword_args` to `LinkedHashMap`. `quarto-pandoc-types` is internal — touching it is fine.

### Q2. Quoting strategy for argument values — RESOLVED: option (b), with explicit rules derived from the grammar

The parser strips quotes — `width="800"` and `width=800` both end up as `ShortcodeArg::String("800")` (or `ShortcodeArg::Number(800.0)` for naked numbers). When writing back, we need a quoting policy:

- **(a) Always quote string values.** Safe, simple, but turns every `width=800` into `width="800"`.
- **(b) Only quote when necessary** (value contains whitespace, `>`, `=`, or starts with a quote). Closer to user intent, more code.
- **(c) Track an `is_quoted: bool` on each `ShortcodeArg::String`.** Most faithful, but invasive (changes the enum) and probably overkill.

**Decision (user, 2026-04-30):** quote-on-demand, but the rules must match what the parser actually accepts as a naked string. From `tree-sitter-qmd/tree-sitter-markdown/grammar.js:568-581`:

- `shortcode_name` — `[a-zA-Z_][a-zA-Z0-9_-]*` (the keyword-arg key, and any positional that the parser would route to `shortcode_name`).
- `shortcode_naked_string` — chars from the set `[A-Za-z0-9_.~:/?#\]@!$%&()+,;-]` plus `[`, with an optional `?…` second segment that additionally allows `=`. Notably this set includes `/`, `:`, `?`, `=` (in the second segment) — so URLs like `https://youtu.be/abc` are valid naked. It does **not** include whitespace, `>`, `<`, `{`, `}`, `'`, `"`, or `*`.
- `shortcode_number` — JSON-style numeric literal.
- `shortcode_string` — single- or double-quoted (with `\\'` / `\\"` escapes for the matching quote).

Concrete writer rule for a `ShortcodeArg::String(s)`:

1. If `s` matches `shortcode_number`'s pattern, prefer to quote it (otherwise the round-trip would re-parse it as `ShortcodeArg::Number`, changing the AST type).
2. Else if `s` is non-empty and every char is in the naked-string set (and there's no whitespace), emit naked.
3. Otherwise emit double-quoted, escaping any embedded `"` as `\\"` and any `\\` as `\\\\`.

Edge cases worth a test: empty string (must quote: `""`), value containing `>` (must quote — would otherwise close the shortcode), value containing a literal `"` (quote and escape).

For `ShortcodeArg::Number`, `f64::to_string` renders `800.0` as `"800"` — that's the round-trip we want. (`shortcode_number` accepts `800` as an integer literal.) Negative / scientific values still match the grammar regex.

For `ShortcodeArg::Boolean`, emit `true` / `false` naked (both match `shortcode_name`'s pattern).

For keyword arg keys: per `_key_specifier_token`, keys are emitted bare. Verify `_key_specifier_token`'s pattern during implementation and reject (panic? warn?) keys outside it — that's an invariant the parser already enforces, so any in-AST key violating it came from a non-parser source (e.g. Lua filter).

### Q3. Should we use `source_info` to copy source verbatim? — RESOLVED: no

`Shortcode::source_info` contains the original byte range. If the qmd writer had the source bytes, it could reproduce the input verbatim. It does not currently — `QmdWriterContext` (see `crates/pampa/src/writers/qmd.rs`) doesn't carry the source.

I propose we **do not** plumb source bytes into the writer. Reasons:

- Other writers (JSON / native) reconstruct from the AST and that's the contract.
- Plumbing source bytes adds coupling for one inline kind.
- The AST round-trip is the contract — if it loses information, the right fix is to fix the AST.

This means we accept that quoting/whitespace inside the shortcode may differ from the source, even if Q1 and Q2 are resolved well. That's fine for the user's bug — they want the URL and attributes back, not exact byte preservation.

**Confirmed (2026-04-30):** this matches how the rest of `writers::qmd` works. Verbatim preservation in this codebase lives in `writers::incremental` (`crates/pampa/src/writers/incremental.rs`), which is explicitly designed to "copy verbatim from `original_qmd`" for unchanged byte ranges and to call into `writers::qmd` only for ranges that actually changed. The non-incremental path is semantic-only by design.

### Q4. Block-level shortcode handling

The grammar treats shortcodes as inline only — a standalone `{{< video ... >}}\n` line parses as `Para [ Shortcode ]`. The qmd writer's paragraph path emits the inline correctly once `write_shortcode` is fixed, so no block-level work is needed. (Confirmed by reading `tree-sitter-qmd/tree-sitter-markdown/grammar.js:511`.)

## Plan

### Phase 0 — Resolve open questions

- [x] Q1 (keyword-arg ordering): switch `keyword_args` to `LinkedHashMap` in `quarto-pandoc-types`. (Confirmed by user 2026-04-30.)
- [x] Q2 (quoting): quote-on-demand using rules derived from `tree-sitter-markdown/grammar.js:568-581`. (Confirmed by user 2026-04-30; rules audited against grammar — see Q2 above.)
- [x] Q3 (no source-byte plumbing): semantic round-trip only; verbatim preservation already lives in `writers::incremental`. (Confirmed by user 2026-04-30; verified against `crates/pampa/src/writers/incremental.rs`.)

### Phase 1 — Failing tests (TDD)

Per `crates/pampa/CLAUDE.md`: write tests first, run, see them fail, *then* implement.

- [x] Add a **direct qmd → qmd writer** test (the existing qmd-json-qmd suite does not exercise `write_shortcode`). New fixtures under `tests/snapshots/qmd/shortcode-*.qmd` covered by `unit_test_snapshots_qmd` (`tests/test.rs:293`):
  - [x] `shortcode-name-only.qmd` — `{{< meta >}}`
  - [x] `shortcode-positional.qmd` — `{{< video https://youtu.be/abc >}}` (the user's URL case)
  - [x] `shortcode-keyword-args.qmd` — `{{< video https://youtu.be/abc width="800" height="450" >}}` (the user's full case)
  - [x] `shortcode-escaped.qmd` — `{{{< meta >}}}` to verify `is_escaped`
  - [x] `shortcode-naked-string.qmd` — `{{< meta foo >}}` (unquoted positional, no whitespace)
  - [x] `shortcode-with-quoted-string.qmd` — `{{< meta "foo bar" >}}` (positional that requires quoting)
- [x] Add a **round-trip parse-write-parse semantic equivalence** test (`test_qmd_to_qmd_shortcode_roundtrip` in `tests/test.rs`). Parses qmd, writes qmd, re-parses, and compares JSON forms with location fields stripped. Covers the same 6 cases.
- [x] Confirmed both tests fail. The roundtrip test fails with `failed to parse regenerated QMD ("{{meta}}\n")` — the buggy writer's output is not even valid syntax. Snapshot test produces `{{meta}}` instead of `{{< meta >}}`.

### Phase 2 — Fix `write_shortcode`

- [x] Switched `quarto-pandoc-types::Shortcode::keyword_args` from `HashMap` to `hashlink::LinkedHashMap`. All 18 construction sites updated:
  - [x] `crates/pampa/src/pandoc/treesitter_utils/shortcode.rs` — parser
  - [x] `crates/pampa/src/pandoc/shortcode.rs` — `shortcode_to_span` + 12 unit-test fixtures
  - [x] `crates/quarto-core/src/transforms/{shortcode_resolve,metadata_normalize}.rs` + `stage/stages/include_expansion.rs` — 5 sites
  - [x] `crates/quarto-analysis/src/transforms/shortcode.rs` (already used `Default::default()` — no change needed)
  - [x] `crates/quarto-ast-reconcile/src/generators.rs` — proptest `gen_shortcode_inner` (now collects HashMap into LinkedHashMap)
  - [x] `crates/quarto-pandoc-types/src/inline.rs` test fixture
  - [x] `crates/pampa/src/{filters,writers/plaintext,lua/diagnostics,lua/filter,lua/types}.rs` test fixtures
  - [x] Removed five now-unused `use std::collections::HashMap` imports.
  - [x] `cargo check --workspace --all-targets` clean (no warnings, no errors).
- [x] Implemented `write_shortcode` in `crates/pampa/src/writers/qmd.rs`:
  - [x] Open delimiter `{{<` (or `{{{<` when `is_escaped`).
  - [x] Name.
  - [x] Positional args, single-space separated. Strings via `write_shortcode_string_value` with quote-on-demand (`shortcode_string_needs_quoting`) — handles empty, number-shaped, and any char outside the naked set. Numbers via `f64::to_string`. Booleans as `true`/`false`. Nested shortcodes recurse. Positional `KeyValue` emits each pair as `key=value` (best-effort; parser doesn't produce this variant).
  - [x] Keyword args, in `keyword_args` insertion order, formatted `key=value` (with same value-quoting rules).
  - [x] Close delimiter `>}}` (or `>}}}` when escaped).
  - [x] Added 6 unit tests for the quoting helper (`shortcode_writer_tests`) covering naked strings, empty, whitespace, delimiter chars, number-shaped strings, and near-numbers.
- [x] Reviewed all 6 new snapshot files — every one round-trips byte-exactly (the keyword-args case preserves `width=...height=...` order thanks to LinkedHashMap).
- [x] `cargo nextest run -p pampa` — all **3663** tests pass, 0 failures.
- [x] `cargo nextest run --workspace` — all **7602** tests pass, 0 failures. No regression in `test_qmd_roundtrip_consistency`, proptest reconcile tests, or anything else.

### Phase 3 — End-to-end verification

- [x] Re-ran the user's exact reproduction:

  ```
  $ printf '{{< video https://youtu.be/abc width="800" height="450" >}}\n' \
      | cargo run --quiet --bin pampa -- -t qmd
  {{< video https://youtu.be/abc width="800" height="450" >}}
  ```

  Output is byte-for-byte identical to the input (modulo trailing newline behavior of `cargo run`). URL preserved, both keyword args preserved in original order, delimiters preserved.
- [x] Ran full `cargo xtask verify` (full, not `--skip-hub-build`, because we touched `quarto-pandoc-types` which `wasm-quarto-hub-client` depends on). All 9 steps passed: `cargo build --workspace`, `cargo nextest run --workspace` (7602 tests), hub-client lint, hub-client `npm run build:all` (WASM rebuilt), hub-client `npm run test:ci`, trace-viewer build + tests.

### Phase 4 — Wrap up

- [ ] `cargo fmt` on touched files.
- [ ] Stage, commit (do NOT push). Mention snapshot file count per project policy.
- [ ] Ask the user for permission to push.

## Files of interest (quick reference)

| Concern | Path |
|---|---|
| Bug | `crates/pampa/src/writers/qmd.rs:1716` |
| AST type | `crates/quarto-pandoc-types/src/shortcode.rs` |
| Parser | `crates/pampa/src/pandoc/treesitter_utils/shortcode.rs:48` |
| JSON/native conversion | `crates/pampa/src/pandoc/shortcode.rs:56` |
| Roundtrip suite | `crates/pampa/tests/test.rs:704` |
| QMD snapshot harness | `crates/pampa/tests/test.rs:293`, fixtures in `tests/snapshots/qmd/` |
| Inline grammar | `crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js:511,538,548` |
