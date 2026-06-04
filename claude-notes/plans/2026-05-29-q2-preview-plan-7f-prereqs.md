# Plan 7f — Source-info prerequisites

**Date:** 2026-05-29
**Branch:** feature/provenance
**Status:** Landed. Source-info hygiene that shipped on the provenance branch.

> **Scope note (2026-06-05).** This plan originally had four workstreams.
> Two of them — *framework source_info preservation* (spreading `s:` through
> rebuilt wrappers) and *user-edit stamping* (`stampUserEdits` + a reserved
> `user_edit` pool slot) — were prerequisites for the Plan-7 write-back model
> and were **reverted** when that model was withdrawn (the replacement model
> is `target-incremental-writes.md`; the revert is git history). Their phases
> (originally Phases 1–3, plus the reserved-slot half of Phase 4) have been
> excised. What remains is the live, kept work: the strict/completing JSON
> reader split, the `SourceInfo::default()` deprecation, the production-residue
> `By::` cleanup, and the wire-format renames. Phase numbers are preserved for
> continuity with cross-references.

## Overview

Producer-side source-info hygiene, none of which involves the writer itself:

1. **`SourceInfo::default()` audit** — replace test usages with explicit kinds; deprecate the `Default` impl.
2. **Production-residue cleanup** — handful of non-test `SourceInfo::default()` sites in `quarto-pandoc-types` and `quarto-yaml-validation`. Each gets a deliberate `By::` kind (four new constructors, including `By::unknown()` for the source-info-completing reader's placeholder). Refactors `InlineAttr::new` to require explicit source_info, eliminating the empty-AttrSourceInfo sentinel. Splits `json::read` into a strict variant for q2-internal paths and `read_completing_source_info` for callers that consume JSON from outside the source-tracked world (qmd-syntax-helper Pandoc subprocess output, CLI `--from json`, external filter binaries, Lua AST handoff).

Plus two minor cleanups bundled along for the ride: wire-format renames `attrS` → `a`, `sourceInfoPool` → `p`.

## Phase 4 — The strict / completing JSON reader split

Split `json::read` into two readers, scoping leniency to specific call sites:

- **`json::read`** becomes strict: rejects nodes missing `s:` with `Err(JsonReadError::MissingSourceInfoRef { node_path })`. Used by q2-internal JSON consumers (e.g. the WASM bridge's `incremental_write_qmd`).
- **`json::read_completing_source_info(input, default_by: By)`** fills a missing `s:` by constructing `Generated{by: default_by, from: []}` in place per node (no pool grown on read; the writer allocates a fresh pool entry on re-serialize). Used by callers that consume JSON from outside the source-tracked world.

The leniency is a property of the explicit call site, not of the wire format — there is no compatibility shim.

### Research finding (2026-05-30) — per-caller verification for the reader split

Verified the five completing-reader callers in turn. All five consume `source_info` from the parsed AST downstream, so the placeholder choice matters (none are "ignored entirely" cases). Per-site detail:

| Site | Downstream use | Placeholder |
|---|---|---|
| `crates/pampa/src/json_filter.rs:221` | Filtered AST replaces the pre-filter AST in the main pipeline; downstream stages and the eventual writer consume `source_info`. | `By::filter(filter_path, 0)` would be more specific than `unknown`. `By::filter` already exists (`crates/quarto-source-map/src/source_info.rs:535`); reused by code-3 legacy reader at `readers/json.rs:305`. Recommend `By::filter`, with line `0` since we don't know which line in the filter produced each node. |
| `crates/qmd-syntax-helper/src/conversions/definition_lists.rs:182` | Parsed AST goes to `qmd::write(&pandoc_ast, ...)` to round-trip back to markdown. The qmd writer dispatches on source_info: `Original{FileId(0), 0..0}` (today's default) routes through R1 with empty range → emits nothing; `Generated{by: unknown, …}` routes through R5 (synthesize) → emits from structure. The change is the *correct* behavior here — the AST has no preimage. | `By::unknown` is the right placeholder. Flag in the commit: writer dispatch changes from R1-empty to R5 for these AST nodes; the new behavior is the correct one. |
| `crates/qmd-syntax-helper/src/conversions/grid_tables.rs:133` | Same shape as definition_lists.rs above. | Same: `By::unknown`. Same writer dispatch shift applies. |
| `crates/pampa/src/main.rs:290` | CLI `--from json`. The result flows through `transform_divs` and then into the standard render pipeline; downstream may consume `source_info` anywhere. | `By::unknown` is correct — the user passed JSON from outside, we genuinely don't know. |
| `crates/pampa/src/lua/readwrite.rs:447` | Result is exposed to Lua filters via `rust_pandoc_to_lua_table`. Whether a given Lua filter reads `source_info` is filter-dependent; can't be ruled out. | `By::unknown` is correct — we don't know what produced this JSON. |

Signature surfaced: `json::read_completing_source_info` should accept a placeholder, not bake `By::unknown` in. Two reasonable shapes:

```rust
// Option 1: parameterized placeholder
pub fn read_completing_source_info(input: ..., default_by: By) -> Result<(Pandoc, ASTContext)>;

// Option 2: caller overwrites after read
pub fn read_completing_source_info(input: ...) -> Result<(Pandoc, ASTContext)>;
// caller then runs a pass to overwrite Generated{by: unknown} with their kind.
```

Recommend **Option 1**. The placeholder is set once on read (cheap, simple); Option 2 requires an extra AST walk to overwrite, which both adds work and risks missing nodes. Option 1 also matches the named-parameter discipline already used by the Phase 4 design: the call site declares its provenance up front.

Concretely:

```rust
// json_filter.rs
let (filtered_pandoc, filtered_context) = readers::json::read_completing_source_info(
    &mut json_output.as_bytes(),
    By::filter(filter_path.to_string_lossy(), 0),
)?;

// the other three
readers::json::read_completing_source_info(&mut cursor, By::unknown())
```

Note: `By::filter` is atomic-kind (`is_atomic_kind()` returns `true` for `kind == "filter"` per `crates/quarto-source-map/src/source_info.rs:839`). That's the correct semantic for the `json_filter.rs` site: the completing reader only fires there on nodes the filter *added* (pass-through nodes keep their original `s:` references), and filter-added nodes shouldn't be source-editable in the preview. No `By::filter_output` alternative needed.

`By::unknown` is **non-atomic**. Nodes carrying it are editable in the preview; user edits re-stamp them as `By::user_edit` on save. This matches the `qmd-syntax-helper` round-trip and CLI `--from json` cases, both of which need their output to remain editable.

Work items (live; reserved-`user_edit`-slot items excised with the reverted write-back model):

- [x] Rust: add `JsonReadError::MissingSourceInfoRef { node_path: String }` variant to `crates/pampa/src/readers/json.rs`. `node_path` is a best-effort identifier (tag name + parent context, e.g. `"Block.Para"`, `"Inline.Str"`, `"Caption"`).
- [x] Rust: make `json::read` strict — reject missing `s:` with `Err(JsonReadError::MissingSourceInfoRef)`. Add `json::read_completing_source_info(input, default_by: By)` alongside; it fills missing `s:` by constructing `Generated{by: default_by, from: []}` in-place per node (no pool grown on read — the writer allocates a new pool entry on re-serialize). Applied uniformly across Block, Inline, Cell, Row, TableHead, TableBody, TableFoot, Caption, ConfigValue. Strict-reader bug-catches: the writer's `write_custom_block`/`stream_write_custom_block` were synthesizing `Plain`/`Div`/`Span` wrappers without `s:`; fixed to inherit the parent CustomNode's `s_id`. Figure block now emits `captionS` (same shape as Table) so the strict reader can recover the caption's source_info.
- [x] Rust: add `By::unknown()` constructor in `quarto-source-map` (`kind: "unknown"`, **non-atomic**).
- [x] Rust: switch the five outside-world callers to `json::read_completing_source_info` with explicit placeholders per the per-caller table above:
  - `json_filter.rs:221` → `By::filter(filter_path.to_string_lossy(), 0)`. Atomic-kind is the correct semantic.
  - `qmd-syntax-helper`'s `definition_lists.rs:182` and `grid_tables.rs:133` → `By::unknown()`. (Required adding `quarto-source-map` to `qmd-syntax-helper/Cargo.toml`.) Writer dispatch shifts from R1-empty to R5-synthesize for these nodes; new behavior is correct.
  - `pampa/src/main.rs:290` → `By::unknown()`.
  - `pampa/src/lua/readwrite.rs:447` → `By::unknown()`.
- [x] Rust: migrate reader tests that exercise hand-crafted JSON without `s:`. `json_reader_smoke_tests.rs` and `test_json_div_transforms.rs` route through `read_completing_source_info(By::unknown())`.
- [x] Rust test: strict reader rejects bare nodes; completing reader fills them. `strict_reader_rejects_nodes_missing_source_info` + `completing_reader_fills_missing_source_info_with_placeholder` in `json_reader_smoke_tests.rs`.
- [x] Documentation: `crates/pampa/src/readers/json.rs` module docs explain the two-reader split — q2-internal paths use strict, outside-world paths use completing with explicit `default_by`.

**Two readers — strict `json::read` for q2-internal JSON, `read_completing_source_info` for callers that need a fallback.** The current single `json::read` is consumed by both q2-internal paths (the WASM bridge's `incremental_write_qmd`, which reads q2-extended JSON with `s:` populated on every node) *and* by paths that consume JSON from outside the source-tracked world (`json_filter.rs` for external filter output, `qmd-syntax-helper` for Pandoc subprocess output, `pampa/src/main.rs` for CLI stdin, `lua/readwrite.rs` for Lua AST handoff). The outside-world paths produce JSON without `s:` because the upstream producer doesn't know about q2's extension; making the reader universally strict breaks them.

Split the reader, scoping leniency to specific call sites:

- **`json::read`** becomes strict: rejects nodes missing `s:` with `Err(JsonReadError::MissingSourceInfoRef { node_path })`. Used by the WASM bridge's `incremental_write_qmd` and any future q2-internal JSON consumer.
- **`json::read_completing_source_info(input, default_by: By)`** fills missing `s:` by allocating a fresh pool entry from `default_by` at read time. Used by the four outside-world consumers above with explicit placeholders per the per-caller table — `By::filter(filter_path, 0)` for filter output; `By::unknown()` for the other three.

The function name `read_completing_source_info` matches the surrounding `read_<thing>` convention in `readers/json.rs` (`read_inline`, `read_block`, `read_attr_source`, `make_source_info`) and says exactly what it does: read, then complete any missing source_info. There is no compatibility shim layer — the leniency is a property of the explicit call site, not of the wire format.

The strict-reader rule applies only to JSON under q2's source-tracking contract, and surfaces producer bugs there at the boundary rather than at the writer.

**Strictness precondition.** The strict `json::read` assumes every node it
parses carries an `s:` — i.e. that the producer populated source_info on every
node. The Rust writer satisfies this by construction. Outside-world JSON
producers do not, which is exactly why those call sites use
`read_completing_source_info` with an explicit `default_by` instead.

**Scope of the strict-reader rule.** Every JSON-wire-format struct that has an `s:` field must reject missing-`s:` on read. Per `crates/pampa/src/writers/json.rs:1068-1195` (Cell 1079, Row 1098, Head 1126, Body 1157, Foot 1187; Block at 1196; Inline at 718), the fields exist on: Block, Inline, Cell, Row, Head, Body, Foot. Apply the strict-reader rule uniformly to all of these in the reader update.

**Error variant.** `JsonReadError::ExpectedSourceInfoRef` exists today at `crates/pampa/src/readers/json.rs:31` but fires when the field is *present but malformed*; its message ("Expected SourceInfo $ref, got inline SourceInfo") is wrong for the missing-entirely case. Add a new variant `MissingSourceInfoRef { node_path: String }` carrying the path-to-the-offender context. A JS-side debugger seeing this error in an `incremental_write_qmd` response should be able to find the responsible producer site immediately.

(Phase 4 work items are listed under the per-caller research finding above, which supersedes the earlier checklist.)

## Phase 5 — Wire-format renames

Two JSON top-level fields in `crates/pampa/src/writers/json.rs` get single-character names to match the rest of the wire format:

- `attrS` (currently camelCase from `attr_s: AttrSourceJson`) → `a`. Apply `#[serde(rename = "a")]` to the field.
- `sourceInfoPool` (currently camelCase from `source_info_pool: Vec<SourceInfoJson>`) → `p`. Same mechanism.

Multi-character fields inside `AttrSourceJson` (`classes`, `id`, `kvs`) stay — they're Pandoc-standard. `pandoc-api-version` stays — Pandoc-legacy.

**Snapshot regeneration (scope audited 2026-06-01).** The renames + reserved pool slot change every JSON snapshot the writer produces, but the scope is narrow: **62 `.snap` files** in `crates/pampa/snapshots/json/` (the workspace has 229 `.snap` files total; the other 167 are native/text/qmd/error-corpus snapshots that don't carry source-info references). No other crate's snapshots are affected. Phase 6's R1-empty → R5-synthesize dispatch shift is expected to produce **zero** snapshot diffs (the snapshot harness parses real `.qmd` fixtures, so its AST carries real `Original` source_info, not defaults) — if any qmd-writer snapshot *does* regenerate during Phase 6, treat it as a red flag and investigate before accepting.

Commit-split for the 62-file regeneration (recommended by the audit):

1. **Phase 5 commit** — rename `attrS → a` and `sourceInfoPool → p`, regenerate the 62 snapshots. Diff is pure renames + alphabetic key reordering.
2. **Phase 4 commit** — pre-populate pool slot 0, regenerate the same 62 snapshots. Diff is pure numeric `+1` shifts on every `"s":N` reference plus one new pool entry.

Keeping these separate matters because the union looks like a wholesale rewrite, but each individually is mechanically reviewable.

**Wire-format breaking change.** The renames are a breaking change to the JSON envelope. q2's wire format isn't a documented public contract, but anyone holding cached JSON (test fixtures committed to disk, debug-dump files, recorded session traces under `claude-notes/`) will see breakage. The new fields are byte-equivalent in meaning; only the key names change. No semantic regression, but consumer-side coordination is needed.

Work items:

- [x] Rust: apply `#[serde(rename = "a")]` to the `attr_s` field. The struct's `#[serde(rename_all = "camelCase")]` at `crates/pampa/src/writers/json.rs:146` would otherwise serialize it as `attrS`; the per-field rename overrides that. No separate fallback to remove — the macro effect is what the override replaces.
- [x] Rust: apply `#[serde(rename = "p")]` to the `source_info_pool` field (same pattern).
- [x] Rust: update `crates/pampa/src/readers/json.rs` to read the renamed fields.
- [x] TS: update `ts-packages/pandoc-types/src/types.ts` (every `attrS` field decl; `sourceInfoPool` decl); `ts-packages/preview-renderer/src/types/sourceInfo.ts` and `framework/Ast.tsx` (wire-format-facing); `ts-packages/annotated-qmd/src/{index.ts,block-converter.ts,inline-converter.ts}` (wire-format field accesses + local parameter rename `attrS → attrSource`); annotated-qmd `test/`, `README.md`, `debug-figure.js`, and `check_mismatches.py`. **Audit result 2026-06-01:** `hub-client/src/types/wasm-quarto-hub-client.d.ts` does not reference these keys (verified by `grep`); `hub-client/` and `q2-preview-spa/` likewise do not pattern-match on the renamed keys — they delegate to the TS type packages.
- [x] Regenerate the 62 `.snap` fixtures in `crates/pampa/snapshots/json/`: `INSTA_UPDATE=always cargo nextest run -p pampa`. Diff confirmed pure-rename (`"attrS":` → `"a":`, `"sourceInfoPool":` → `"p":`) plus a refreshed snapshot-source header from the post-bd-xvdop integration-tests layout (`tests/test.rs` → `tests/integration/test.rs`). Per the commit-split note above, Phase 5 (renames) ships before Phase 4's pool-shift; both regenerate the same 62 files in sequence.
- [x] Grep `claude-notes/` for `attrS` / `sourceInfoPool`; updated the two *active* references — `designs/provenance-contract.md` and `instructions/performance-profiling.md` — to use the new keys. Historical plans and research notes (k-197 progress, 2025-10-* designs, etc.) intentionally retain the old names since they describe state-as-of-then.
- [x] Verify the hub server (`crates/hub/`) treats AST JSON as opaque blob and does not pattern-match on `attrS` / `sourceInfoPool` field names. **Audit 2026-06-01:** `grep -rn '"attrS"\|"sourceInfoPool"\|attrS\|sourceInfoPool' crates/hub/` returns nothing — `crates/hub/` does not inspect either field, treats AST JSON as an opaque blob, no changes needed.
- [x] Regenerate annotated-qmd example fixtures (`ts-packages/annotated-qmd/examples/*.json`, 20 files) and the `math-with-attr.json` test fixture by re-running the pampa CLI over their `.qmd` siblings. The committed fixtures were last regenerated 2025-10-24 (commit 2b2337be) and predate the rename; Phase 5's TS-side changes require the fixtures to use the new keys (`a`, `p`) or the tests can't read them at all.
- [x] **Side-issue discovered during fixture regeneration** — 2 of 156 annotated-qmd tests fail (substring-invariant for inline code, div-attrs key-source assertion). Both fail in the same shape: writer-recorded start offsets are 1 char too early on inline-code and key-source spans, capturing the preceding whitespace. Filed as bd-1d6io; pre-existing pampa source-tracking regression unmasked by fixture regeneration. Phase 5 only renamed JSON keys — no offset computation was touched, so this is not a Phase 5 regression. Tracked separately so the off-by-one fix doesn't block Phase 5.

## Phase 6 — Audit `SourceInfo::default()` in tests

Approximately 1,400 references across the workspace. Most are tests with one of three intents; replacements are mechanical.

Add a new constructor first:

```rust
// crates/quarto-source-map/src/source_info.rs
impl By {
    /// Producer kind for test scaffolding. Non-atomic; appears only in
    /// test code where source_info is required by a constructor but
    /// has no real provenance to record.
    pub fn test_scaffold() -> Self {
        Self {
            kind: "test-scaffold".to_string(),
            data: serde_json::Value::Null,
        }
    }
}

impl SourceInfo {
    /// Convenience for tests: produce a non-atomic Generated source_info
    /// that won't be confused with real provenance.
    pub fn for_test() -> Self {
        SourceInfo::Generated {
            by: By::test_scaffold(),
            from: smallvec![],
        }
    }
}
```

Per-test replacement guidance:

| Test intent | Original use of `SourceInfo::default()` | Replacement |
|---|---|---|
| XML/YAML structural; source_info is scaffolding | `SourceInfo::default()` | `SourceInfo::for_test()` |
| Proptest generator; source_info is consistent but not meaningful | `SourceInfo::default()` | `SourceInfo::for_test()` |
| Integration test with known fixture bytes | `SourceInfo::default()` | `SourceInfo::original(FileId(0), start, end)` with the actual offsets |
| Simulating React user-edit | `SourceInfo::default()` | `SourceInfo::Generated { by: By::user_edit(), from: smallvec![] }` |
| Comparison against "no source info" sentinel | `source == &SourceInfo::default()` | Use the `By::is_programmatic_sentinel()` predicate (Phase 6.5 introduces it). Only one site exists today (`crates/quarto-core/src/transforms/navigation_href.rs:382`). No `is_default()` is added — the predicate-on-`By` is more honest after the migration. |

Files to audit (highest concentration first):

- `crates/quarto-xml/src/types.rs` — structural scaffolding case.
- `crates/quarto-yaml-validation/src/tests.rs` — structural scaffolding case.
- `crates/quarto-ast-reconcile/src/generators.rs` — proptest generators.
- `crates/quarto-core/tests/*.rs` (jupyter_integration, navigation_e2e, navigation_merge) — integration tests with fixture bytes.
- Test modules under `crates/pampa/`.

**Production residue is handled in Phase 6.5** (below). The replacement target is **not** `user_edit`. `user_edit` applies only to React-constructed content. Every other caller decides their own provenance kind.

**Behavior change in writer-exercising tests.** Today, `SourceInfo::default()` is `Original{FileId(0), 0, 0}`. Under the writer, that has `preimage_in(target=FileId(0))` returning `Some(0..0)` — an empty range — so R1 fires and emits zero bytes. After the audit, those tests use `SourceInfo::for_test()` which is `Generated{by: test-scaffold, from: smallvec![]}`. `preimage_in` returns `None` for this shape, so R5 fires (or R3, if the node is a container) — different rule, different output. Any test that asserted on the *specific byte output* of running the writer over hand-constructed AST with `SourceInfo::default()` will see different (correct) bytes after the swap. Expect a small batch of test-expectation updates alongside the audit.

Work items:

- [x] Add `By::test_scaffold()` constructor in `quarto-source-map`.
- [x] Add `SourceInfo::for_test()` convenience in `quarto-source-map`.
- [x] Audit test-file usages of `SourceInfo::default()`; replace with one of the four patterns above. Swept ~700 sites across 3 commits: pampa batch (filter_tests.rs + 85 src test-mod sites + 156 tests/ sites), 4-crate scaffolding batch (quarto-xml, quarto-yaml-validation, quarto-ast-reconcile, quarto-core integration tests — 61 sites), and workspace-wide batch (~ 60 PURE_TEST files + 28 MIXED-file test-mod regions, ~570 sites).
- [x] Update writer-exercising test expectations where switching to `for_test()` changes the dispatch rule (R1-empty-range → R5/R3) — the new output is the correct one. Two assertion-pin fixes surfaced and addressed (`engine_execution.rs:1378`, `inline.rs:1459`); neither was a writer-byte-output test, both pinned production behavior that Phase 7's deprecation will surface for proper fix-up.
- [x] Verify: `cargo nextest run --workspace` passes after replacements (9736/9736 pass after the test-mod sweep + later Phase 6.5 commits).

## Phase 6.5 — Production-residue fix sweep

The non-test `SourceInfo::default()` usages turn out to be a small, well-characterized set after filtering out the `#[cfg(test)] mod tests` blocks. Per-site decisions follow; each gets a deliberate `By::` kind rather than the default sentinel. Add the two new `By::` constructors and one new predicate first, then apply each fix.

### New `By::` constructors + predicate

Add to `crates/quarto-source-map/src/source_info.rs`:

```rust
impl By {
    /// Empty-Map sentinel ConfigValue used during metadata merging when
    /// no value is present.
    pub fn config_default() -> Self {
        Self { kind: "config-default".to_string(), data: Value::Null }
    }

    /// Programmatic construction of ConfigValue (`ConfigValue::from_path`,
    /// intermediate maps created during `insert_path`, etc.) — no source
    /// bytes exist for these.
    pub fn programmatic_config() -> Self {
        Self { kind: "programmatic-config".to_string(), data: Value::Null }
    }

    /// True for kinds whose source bytes don't exist — `config-default`,
    /// `programmatic-config`, `unknown`. Used by code that needs to
    /// distinguish "no real source" sentinels from a genuine
    /// `Original{FileId(0), …}` pointing at a real document.
    pub fn is_programmatic_sentinel(&self) -> bool {
        matches!(
            self.kind.as_str(),
            "config-default" | "programmatic-config" | "unknown"
        )
    }
}
```

Both new constructors are non-atomic (never match `is_atomic_kind`) and require no `Invocation` anchor. `By::unknown()` (added in Phase 4) is the third sentinel kind recognized by `is_programmatic_sentinel`.

An earlier draft also added `By::reconcile_synthesize()`. We dropped it on 2026-05-30: no producer uses it at 7f-landing time, and it was a forward-looking primitive with no current call site. If reconciliation later grows a path that synthesizes new AST without an input `SourceInfo` to inherit from, add the constructor then.

### Per-site fixes

**`crates/quarto-pandoc-types/src/config_value.rs:415`** — `impl Default for ConfigValue`. The empty-Map sentinel used in metadata merging.

```rust
// Before
source_info: SourceInfo::default(),

// After
source_info: SourceInfo::Generated {
    by: By::config_default(),
    from: smallvec![],
},
```

**`crates/quarto-pandoc-types/src/config_value.rs:539`** — `ConfigValue::from_path`. WASM-bridge programmatic injection.

```rust
// Before
let source_info = SourceInfo::default();

// After
let source_info = SourceInfo::Generated {
    by: By::programmatic_config(),
    from: smallvec![],
};
```

**`crates/quarto-pandoc-types/src/config_value.rs:822, 826`** — `ConfigValue::insert_path`. The recursive descent creates intermediate map nodes (`new_map(vec![], SourceInfo::default())` at 822) and intermediate `key_source` slots (`key_source: SourceInfo::default()` at 826) when the path is deeper than the existing structure. Same provenance as `from_path` — programmatic, no source bytes. Replace both with `SourceInfo::Generated { by: By::programmatic_config(), from: smallvec![] }`.

**`crates/quarto-core/src/project_resources.rs:541`** — `canonicalize_within_project(project_root, &absolute, &raw_str, &SourceInfo::default())`. The comment there says "Engine/Lua-filter entries don't have a YAML source location; diagnostics degrade to a span-less message." Replace with `&SourceInfo::Generated { by: By::unknown(), from: smallvec![] }`. The receiver only uses the source location for diagnostic span rendering, which already degrades gracefully when the location can't be mapped to bytes. (Follow-up beads issue: refactor `canonicalize_within_project` to take `Option<&SourceInfo>` instead of requiring a sentinel — out of scope for 7f.)

**`crates/quarto-core/src/transforms/navigation_href.rs:382`** — `if source == &SourceInfo::default()`. The site detects "this is the programmatic sentinel, not a real source" and returns `raw` unchanged. After the migration, no single sentinel value exists; the programmatic-sentinel kinds (`config-default`, `programmatic-config`, `unknown`) all carry the same "no real source bytes" semantic. Replace with:

```rust
// Before
if source == &SourceInfo::default() {
    return raw.to_string();
}

// After
if let SourceInfo::Generated { by, .. } = source
    && by.is_programmatic_sentinel()
{
    return raw.to_string();
}
```

**`crates/quarto-yaml-validation/src/schema/merge.rs:32, 51, 88`** and **`schema/mod.rs:256`** — `SchemaError::InvalidStructure { location }`. These four sites describe bugs in the schema *definition* itself, not in the user's YAML; they pass `quarto_yaml::SourceInfo::default()` (a re-export of `quarto_source_map::SourceInfo`) as a placeholder. Change the variant's signature:

```rust
// In SchemaError (crates/quarto-yaml-validation/src/error.rs:9)
InvalidStructure {
    message: String,
    location: Option<SourceInfo>,   // None for schema-structure errors
}
```

The signature change has wider fanout than the four `None` sites suggest:

- **Schema-structure-error sites (4)** at `schema/merge.rs:32, 51, 88` and `schema/mod.rs:256` (the variant is actually constructed at line 250; line 256 in the plan refers to the closure's body) → set `location: None`.
- **User-yaml-validation sites (~11)** at `schema/helpers.rs:20, 40, 56, 70, 86, 95, 114, 125, 151, 158` already pass a real `value.source_info.clone()` → wrap each in `Some(...)`.
- **Formatter** at `crates/quarto-yaml-validation/src/error.rs:33-46` destructures `InvalidStructure { message, location }` and calls `location.start_offset()` → add a `match Some/None` arm; `None` renders without span.
- **Test pattern-matching** in `schema/helpers.rs:288, 332, 377, 428, 475, 489, 538, 589, 672, 686` already destructures with `..` → unchanged.

Single-crate change; no cross-crate ripple. The compiler walks you through every site once the enum changes.

**`crates/quarto-pandoc-types/src/inline.rs:333-348`** — `InlineAttr::new`. (Earlier plan drafts cited lines 304-311; the file has drifted.) The current `attr_source.combine_all().unwrap_or_default()` fallback is the source of the empty-AttrSourceInfo sentinel. Refactor the signature to require explicit source_info:

```rust
// Before
impl InlineAttr {
    pub fn new(attr: Attr, attr_source: AttrSourceInfo) -> Self {
        let source_info = attr_source.combine_all().unwrap_or_default();
        Self { attr, attr_source, source_info }
    }
}

// After
impl InlineAttr {
    pub fn new(attr: Attr, attr_source: AttrSourceInfo, source_info: SourceInfo) -> Self {
        Self { attr, attr_source, source_info }
    }

    /// Convenience: derive source_info from non-empty AttrSourceInfo.
    /// Panics if attr_source is empty (use new() with explicit source_info instead).
    pub fn new_from_attr_source(attr: Attr, attr_source: AttrSourceInfo) -> Self {
        let source_info = attr_source.combine_all()
            .expect("InlineAttr requires non-empty AttrSourceInfo; use new() with explicit source_info");
        Self { attr, attr_source, source_info }
    }
}
```

Then update every `InlineAttr::new` call site that uses `AttrSourceInfo::empty()` to provide explicit source_info. See the research finding below for the actual list (the line numbers cited in earlier drafts of this plan pointed to test scaffolding, not `InlineAttr::new` calls).

**Delete the obsolete test.** The `source_info_attr_empty` test at `crates/quarto-pandoc-types/src/inline.rs:1452-1463` asserts the fallback behavior we just removed. Delete it. Commit message should note: "removes test for empty-AttrSourceInfo sentinel; case is now structurally impossible after InlineAttr::new signature change."

### Research finding (2026-05-30) — reconciler/block-test "synthesis sites" are not InlineAttr::new sites

The earlier draft listed `crates/quarto-ast-reconcile/src/lib.rs:107, 116, 132, 322, 1178` and `crates/quarto-pandoc-types/src/block.rs:222, 235, 247` as `InlineAttr::new` call sites that needed the explicit-source_info update. Re-reading those line numbers shows the claim is wrong on two counts:

1. **None of those sites call `InlineAttr::new`.** They directly assign `attr_source: AttrSourceInfo::empty()` to a field of a `Block::Header` / `Block::CodeBlock` / `Block::Div` / `Inline::Code` / `Inline::Insert` struct. Those types each have their own `source_info: SourceInfo` and `attr_source: AttrSourceInfo` fields; the `combine_all().unwrap_or_default()` fallback in `InlineAttr::new` is never invoked through them.

2. **All eight sites are test code.** Lines 107-134 of `quarto-ast-reconcile/src/lib.rs` are inside the crate's `#[cfg(test)] mod tests` block (`make_header`, `make_code_block`, `make_div` test helpers). Line 322 is in `test_inline_code_replaced_with_result`. Line 1178 is in `make_insert_para`, a helper inside another `#[test]` function. Lines 222-247 of `quarto-pandoc-types/src/block.rs` are inside that file's `#[cfg(test)] mod tests` (`source_info_plain`, `source_info_paragraph`, `source_info_codeblock`). Phase 6.5 is production-residue cleanup; test sites belong to Phase 6.

**Where the real `InlineAttr::new` call sites live** (from a clean `grep -rn 'InlineAttr::new' crates/`):

| Site | Status | Treatment |
|---|---|---|
| `crates/quarto-pandoc-types/src/inline.rs:1455, 1474, 1491` | Test code (`#[cfg(test)] mod tests`). | Phase 6 — replace with explicit `source_info` once the new signature lands. |
| `crates/pampa/src/pandoc/treesitter.rs:559` | **Production** — tree-sitter intermediate → `Inline::Attr`. Destructures `(attr, attr_source)` from `PandocNativeIntermediate::IntermediateAttr`. | Widen the enum variant — see "Production callers via PandocNativeIntermediate" below. |
| `crates/pampa/src/pandoc/treesitter_utils/caption.rs:50` | **Production** — caption_attr → `Inline::Attr`. Same pattern. | Same treatment. |
| `crates/pampa/src/pandoc/treesitter_utils/paragraph.rs:30` | **Production** — paragraph attr inline → `Inline::Attr`. Same pattern. | Same treatment. |
| `crates/pampa/src/filters.rs:1503, 1513, 2123` | Test code. | Phase 6. |
| `crates/pampa/src/writers/plaintext.rs:887` | Test code (the surrounding context is a `let inlines = vec![make_str("text"), ...]` test fixture). | Phase 6. |
| `crates/pampa/src/lua/types.rs:2932` | Test code (`#[test] fn test_lua_inline_tag_name_attr`). | Phase 6. |
| `crates/pampa/src/lua/filter.rs:2254` | Test code (assert inside a `#[test]`). | Phase 6. |

**None of the three production `InlineAttr::new` callers passes `AttrSourceInfo::empty()`** — they all pass a real `attr_source` from the parse. The production-side migration of the `InlineAttr::new` signature happens via **widening the producer-side enum** rather than wiring source_info through each caller's local context, which would require chasing the tree-sitter node back up the call stack in three uneven ways.

### Production callers via `PandocNativeIntermediate` (decision 2026-06-01)

All three production call sites destructure `(attr, attr_source)` from the same enum variant — `PandocNativeIntermediate::IntermediateAttr(Attr, AttrSourceInfo)`. The cleanest migration is to widen that variant once, at the producer side, so it carries source_info from creation:

```rust
// Before
PandocNativeIntermediate::IntermediateAttr(Attr, AttrSourceInfo)

// After
PandocNativeIntermediate::IntermediateAttr(Attr, AttrSourceInfo, SourceInfo)
```

Then each of the three consumers destructures four fields instead of two and passes the source_info straight through to `InlineAttr::new(attr, attr_source, source_info)`. The producer sites that construct `IntermediateAttr` (search the workspace with `grep -rn 'IntermediateAttr(' crates/`) get a SourceInfo from their local parse context — they have a tree-sitter node in scope, so deriving a `SourceInfo::Original{file_id, start_offset, end_offset}` is local.

Why widen the enum rather than wire through three callers separately: provenance is *carried* with the intermediate, not reconstructed at the consumer. If a future fourth consumer appears, it gets source_info automatically. If the producer's source_info ever drifts (e.g. from a refactor of the parse helper), it's one site to update, not three. And the call-stack chase for the existing three consumers may surface inconsistencies — caption.rs and paragraph.rs in particular destructure from a `child` variant inside a loop, with no easy local handle on the original tree-sitter range.

### Work items

- [x] Add `By::config_default()`, `By::programmatic_config()`, `By::is_programmatic_sentinel()` to `quarto-source-map`. (Earlier drafts also added `By::reconcile_synthesize()`; dropped — no producer uses it.)
- [x] Unit tests in `quarto-source-map`:
  - Assert `By::test_scaffold()`, `By::config_default()`, `By::programmatic_config()` all return `false` from `is_atomic_kind()`. Pins the property explicitly so a future producer-contract change can't accidentally promote one to atomic.
  - Assert `By::unknown()` (from Phase 4) returns `false` from `is_atomic_kind()`.
  - Assert `is_programmatic_sentinel()` returns `true` for `By::config_default()`, `By::programmatic_config()`, `By::unknown()` and `false` for `By::user_edit()`, `By::filter("x.lua", 1)`, `By::shortcode("meta")`.
- [x] Apply `config_value.rs:415` (Default impl) fix → `By::config_default()`.
- [x] Apply `config_value.rs:539` (from_path) fix → `By::programmatic_config()`.
- [x] Apply `config_value.rs:822, 826` (insert_path intermediates) fix → `By::programmatic_config()`. Also forwarded the same kind into `pampa/src/readers/json.rs:2212` (top-level meta) to keep round-trips lossless.
- [x] Apply `project_resources.rs:541` fix → `By::unknown()`. Follow-up beads issue `bd-3az78` filed to refactor `canonicalize_within_project` to take `Option<&SourceInfo>`. Also fixed `project_resources.rs:123` (`Pattern::without_source`) which surfaced during the audit.
- [x] Apply `navigation_href.rs:382` fix → replace `source == &SourceInfo::default()` with the `Generated { by, .. } if by.is_programmatic_sentinel()` pattern.
- [x] Apply newly-discovered production sites (cross-crate audit 2026-06-01):
  - `crates/quarto-citeproc/src/output.rs:1274` — landed as dedicated `By::citeproc()` (atomic).
  - `crates/quarto-config/src/materialize.rs:132, 152, 165` — landed as `By::programmatic_config()` / `By::unknown()` per site.
  - `crates/quarto-core/src/project/listing/feed/stage.rs:596, 602` — landed as `By::unknown()`. Same shape applied to the sibling sites in `feed/complete.rs` and `listing/post_render_upgrade/substitute.rs`.
- [x] Change `SchemaError::InvalidStructure::location` to `Option<SourceInfo>`; update the 4 `None` sentinel sites (`schema/merge.rs:32, 51, 88`; `schema/mod.rs:250`), wrap the ~11 real-source sites in `helpers.rs:20, 40, 56, 70, 86, 95, 114, 125, 151, 158` in `Some(...)`. Actual scope was wider — 33 `Some(...)` wraps across helpers.rs, parser.rs, parsers/{combinators,enum,objects,ref,wrappers}.rs — applied via compile-error-driven sweep. Formatter at `error.rs:33-46` now branches on `Option`. New regression test `test_schema_error_invalid_structure_display_no_location`.
- [x] Refactor `InlineAttr::new` signature (at `crates/quarto-pandoc-types/src/inline.rs:340`); add `new_from_attr_source` convenience.
- [x] Widen `PandocNativeIntermediate::IntermediateAttr` from `(Attr, AttrSourceInfo)` to `(Attr, AttrSourceInfo, SourceInfo)`. Updated every constructor site (5 production sites in `treesitter.rs` + `commonmark_attribute.rs` + `info_string.rs` + `language_specifier.rs`) and every consumer site (the three plan-named sites + 8 destructuring sites in `atx_heading`, `code_span_helpers`, `editorial_marks`, `fenced_code_block` ×2, `fenced_div_block`, `span_link_helpers` ×2).
- [x] Update the **test-code** `InlineAttr::new` call sites (`quarto-pandoc-types/src/inline.rs:1455, 1474, 1491`; `pampa/src/filters.rs:1503, 1513, 2123`; `pampa/src/writers/plaintext.rs:887`; `pampa/src/lua/types.rs:2932`; `pampa/src/lua/filter.rs:2254`) to pass `SourceInfo::for_test()`. Two `inline.rs` tests migrated to `new_from_attr_source` since they specifically exercise the derive-from-AttrSourceInfo path.
- [x] Delete `source_info_attr_empty` test at `inline.rs:1453`. Was already neutralised during Phase 6 sweep (assertion-pin fix), now structurally impossible after the signature change.
- [x] Audit `AttrSourceInfo::empty()` call sites: confirmed they're scaffolding-only — the production-side `InlineAttr::new` no longer accepts empty input as a sentinel-triggering pattern, so `AttrSourceInfo::empty()` is honest about its meaning everywhere it appears. No site renamed.
- [x] Decide whether `AttrSourceInfo::empty()` should be renamed: kept as-is — the name is honest and the rename would touch every Block-with-attr test fixture.
- [x] Clean up the stale doc-comment at `crates/quarto-pandoc-types/src/attr.rs:45-46`: doc now reads "fall back to `None` (or whatever Option<SourceInfo>-aware behavior the consumer prefers)" and cross-references `theorem.rs` / `proof.rs` as canonical patterns.
- [x] Verify: `cargo xtask verify --skip-hub-build` clean after all sites are updated. **Promoted to full `cargo xtask verify` for Phase 8 (see below).** Full verify passed 2026-06-01 (see Phase 8 item).

### Discovered production residue — landed during Phase 6.5

The Phase 6 sweep surfaced ~70 production `SourceInfo::default()`
sites the plan didn't enumerate. Per user direction (2026-06-01),
they were all addressed during Phase 6.5 rather than deferred to
Phase 7's compiler audit. Three new `By::*` kinds were defined to
support them:

- `By::citeproc()` (atomic) — CSL-rendered citation/bibliography
  content.
- `By::jupyter_output()` (atomic) — kernel-execution outputs;
  regenerate on every re-run.
- `By::callout()` (non-atomic) — callout-decoration synthesis
  (default-title injection, screen-reader-only spans). The user's
  callout body stays editable.

Per-site landing summary:

- **pampa/src/** (28 sites): `citeproc_filter.rs` → `By::citeproc()`;
  `pandoc/meta.rs` + `writers/json.rs` yaml-tagged-string spans →
  reuse the YAML value's `source_info` for both wrapper and inner
  scalar; `template/config_merge.rs` → `By::config_default()`;
  `toc.rs` → `By::programmatic_config()`;
  `lua/{types,utils,readwrite}.rs` → `By::unknown()` (Lua-side
  synthesis; `filter_source_info` may overwrite downstream).
  `readers/json.rs` (5 sites) — legitimate per
  `provenance-contract.md` §10; retained.
- **quarto-analysis/src/transforms/shortcode.rs** (7) — reuse the
  shortcode token's source range; same pattern as the canonical
  `shortcode_resolve.rs` enrichment, in the simpler static-analysis
  form.
- **quarto-citeproc/src/output.rs** (1) — `By::citeproc()`.
- **quarto-config/src/materialize.rs** (3) —
  `By::programmatic_config()` / `By::unknown()` per site.
- **quarto-core/src/engine/** (13) — `By::unknown()` for context
  default + `By::jupyter_output()` for cell-output synthesis.
- **quarto-core/src/project/listing/** (8) — `By::unknown()` for
  diagnostic-span fallbacks; `By::programmatic_config()` for
  Listing defaults.
- **quarto-core/src/transforms/** (28) — `By::callout()` for
  callout decorations, `By::programmatic_config()` for navigation/
  render config storage, source_info-reuse for shortcode_resolve
  innermost synthesis sites (the canonical stamper still wraps with
  `Invocation`).
- **quarto-navigation/src/** (16) — `By::programmatic_config()`
  for navigation-item construction without YAML context.
- **quarto-yaml-validation/src/schema/{merge,mod}.rs** — fixed via
  the `Option<SourceInfo>` refactor in the InvalidStructure
  signature change.

Residue that remains after Phase 6.5:

- **`crates/pampa/src/readers/json.rs`** (5 sites) — Pandoc
  legacy-JSON backward-compat per `provenance-contract.md` §10.
  Will need `#[allow(deprecated)]` annotations once Phase 7's
  `#![deny(deprecated)]` lands.
- **`crates/quarto-source-map/src/source_info.rs`** (1 site) —
  the `impl Default for SourceInfo` body itself; Phase 7
  deprecates it.

Phase 7's compiler audit now has a much smaller surface to cover
— the deprecation lights up just these 6 sites plus any
`unwrap_or_default()` / `Default::default()` callers we haven't
spotted yet.

## Phase 7 — Deprecate `SourceInfo::default()`

After Phase 6 brings test usages to the irreducible minimum:

```rust
#[deprecated(
    since = "0.x",
    note = "Use SourceInfo::for_test() in tests, or the appropriate Generated{by: <kind>} in production. See provenance-contract.md."
)]
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

The `#[deprecated]` attribute surfaces remaining call sites at compile time with a clear message. After Phases 6 and 6.5, every known production site has a deliberate replacement.

### The `-D deprecated` strategy (2026-06-01 decision)

The deprecation isn't just informational — it's an **enforcement mechanism**. After Phase 7 lands the deprecation, run a CI build with `RUSTFLAGS="-D deprecated"` (or a workspace-level `#![deny(deprecated)]`) to turn every remaining `SourceInfo::default()` caller into a compile error. The build is green ⇒ every caller is migrated. The build is red ⇒ the failure list IS the residue list; fix or `#[allow(deprecated)]` per-site with a clear comment.

Once `-D deprecated` is green in CI:

- No new `SourceInfo::default()` callers can land.
- The "is this a real source or a sentinel?" question collapses to "what's the `By` kind?" — there's no longer an Original{FileId(0),0,0} sentinel to disambiguate.
- Writer dispatch can assume `Generated` nodes have well-formed `by` kinds and no defaults lurk.

The Phase 6 audit step ("grep for `SourceInfo::default()`") is therefore redundant once the deprecation is in place — the compiler does the audit. Run the deprecation first, fix the failures, ship Phase 7.

### `#[derive(Default)]` exposure (audited 2026-06-01)

Phase 8's `#[derive(Default)]` audit was prompted by a worry that structs with derived `Default` would transitively trigger the deprecation. The audit (2026-06-01) found that the three candidate files (`config_value.rs`, `quarto-lsp-core/src/document.rs`, `quarto-ast-reconcile/src/generators.rs`) contain `#[derive(Default)]` on structs that **do not** contain a `SourceInfo` field — neither directly nor transitively. The deprecation won't fire on them. If `-D deprecated` surfaces unexpected derive-related warnings post-Phase 7, fall back to `#[allow(deprecated)]` with a comment; no audit work is needed up front.

Removing the `Default` impl entirely is a follow-up after the deprecation has had time to surface any forgotten sites.

Work items:

- [x] Add `#[deprecated]` to `impl Default for SourceInfo` in `crates/quarto-source-map/src/source_info.rs`. Rust does not allow `#[deprecated]` on trait method implementations, so the enforcement is via a deprecated *inherent method* `SourceInfo::default()` that shadows the trait impl. The trait `impl Default for SourceInfo` is retained so `unwrap_or_default()` and `#[derive(Default)]` continue to work; callers writing `SourceInfo::default()` (the most common pattern) hit the inherent method and see the deprecation error. The `impl Default` body is a comment-only target for `#[allow(deprecated)]` — it does not itself fire any deprecation.
- [x] Add `#![deny(deprecated)]` at the workspace root (`Cargo.toml` lints table: `deprecated = "deny"`). This turns the deprecation into a compile error for callers of the inherent `SourceInfo::default()` method.
- [x] Run `cargo xtask verify --skip-hub-build` after the deny; iterate on the resulting compile errors until clean. **Residue found and fixed (2026-06-01):** `quarto-core/src/template.rs:2023` (test code, switched to `SourceInfo::for_test`); `quarto-core/src/transforms/proof.rs:189` and `theorem.rs:346` (`unwrap_or_default()` → `unwrap_or_else(|| SourceInfo::generated(By::programmatic_config()))`); `quarto-navigation/src/render_html.rs:828` (`Default::default()` → `SourceInfo::generated(By::programmatic_config())`); `pampa/src/lua/shortcode.rs:416,418,426` (`Default::default()` → `SourceInfo::generated(By::unknown())`). The 5 `readers/json.rs` sites received `#[allow(deprecated)]` with a comment referencing provenance-contract.md §10. All 9743 tests pass.
- [x] CI confirms `-D deprecated` is green (`cargo nextest run --workspace` passes with `deprecated = "deny"` in workspace lints).

## Phase 8 — Verification

- [x] `cargo xtask verify` (full, including hub-build) clean **with `-D deprecated` enabled**. Completed 2026-06-01: full `cargo xtask verify` passed (9745 Rust tests, hub build:all green, 83 WASM tests) — confirmed via exit-code-0 task output. Phase 7's `deprecated = "deny"` is in effect.
- [x] All existing tests pass. Confirmed: 9745 nextest tests pass; 83 WASM tests pass; 19 preview-renderer unit tests pass.
- [x] New tests from Phases 2, 3, 4 pass. All Phase 4 deferred tests committed in `3c3492ac` and Phase 8 deferred tests (pool[0] WASM, MissingSourceInfoRef WASM ×2, atomic-gate sanity ×3, sourceInfo.test.ts) committed in this session — all pass.
- [x] (Audited 2026-06-01 — no work needed.) `#[derive(Default)]` exposure to the deprecation: the three candidate files don't contain a SourceInfo transitively. `-D deprecated` did not surface unexpected derive warnings.
- [ ] Manual smoke test of q2-preview: open a document with shortcodes, edit a paragraph, save, re-open; verify the shortcode tokens are preserved and the framework's `s:` is intact on rebuilt wrappers. *(Requires browser — deferred to user or follow-up session.)*
- [ ] Manual smoke test of q2-debug: open a document; verify the source_info pool display shows `[0] = Generated{by: user_edit, …}` as the reserved slot, and that documents without user edits still display correctly (pool entry 0 is always present even if unreferenced from any node). Also edit a node inside q2-debug; verify the resulting AST round-trips cleanly through `incremental_write_qmd` (no `MissingSourceInfoRef` errors). *(Requires browser — deferred to user or follow-up session.)*
- [x] Plan 7b coordination: 7b ships *after* 7f. Its tests are qmd-focused — they construct ASTs directly in Rust (`SourceInfo::generated(...)`, `BlockAlignment`, etc.) and exercise the qmd writer via `incremental_write`. They don't go through `json::read` or assert on JSON wire-format, so 7f's strict-reader split and `attrS`/`sourceInfoPool` renames are invisible to 7b. The interaction is API-surface-only: 7b's authors write tests against the post-7f APIs from the start — `SourceInfo::for_test()` instead of `SourceInfo::default()`, the three-argument `InlineAttr::new(attr, attr_source, source_info)`, and (if any 7b test constructs `PandocNativeIntermediate::IntermediateAttr`) the three-element tuple. No 7b rebase work; no Phase-8 hand-off action beyond keeping 7f's CI green.

## What 7f does not do

- **No CustomNode serialization.** CustomNode qmd serialization (Callout, Theorem, etc. surviving an edit) is not addressed here.
- **No writer changes.** `coarsen` keeps its flat State-A shape.
- **No removal of `Default` impl.** Deprecation only; removal is a follow-up.

## References

- Producer contract: [`provenance-contract.md`](../designs/provenance-contract.md).
- Playwright fixture convention: `claude-notes/instructions/testing.md` (post-`provenance-reactji-demo` merge).
