# FileId / span integrity audit — assessment and design proposals

**Strand:** bd-nv4p0eb1 (discovered-from bd-m6wmztln / PR #478)
**Date:** 2026-08-09
**Scope:** q2 workspace (`main` + PR #478 branch `a967da81`), quarto-yaml 0.1.2,
quarto-source-map 0.1.0/0.1.1 (checkouts under `external-sources/`).
**Method:** four parallel code sweeps (config-diagnostic binding sites;
`resolve_byte_range` consumers; FileId-namespace mapping; quarto-yaml internals),
with every high-severity finding re-verified by direct reading of the cited
lines. Findings marked **[verified]** were confirmed first-hand in this session;
**[agent]** findings were reported by a sweep with line citations but not
independently re-read.

---

## 1. The design flaw, stated precisely

A source span in this system is semantically a triple **(file identity, byte
range, file content)**. The current APIs let each leg of the triple be
constructed, transported, and re-bound independently:

1. **`FileId(pub usize)` is freely constructible** and carries no notion of
   which registry minted it (`quarto-source-map/src/types.rs:7`).
2. **`SourceContext::add_file_with_id(id, path, content)` accepts any triple**
   (`context.rs:112-144`). Nothing ties the id to the content that will later
   be rendered under it. This is the exact API that made bd-m6wmztln
   representable.
3. **At least five id "namespaces" share the one `usize` space**:
   - **N1** dense sequential ids from `add_file` (index into the files vec);
   - **N2** quarto-yaml's `hash(filename-spelling)` ids
     (`quarto-yaml/src/parser.rs:108-115`);
   - **N3** quarto-yaml's `FileId(0)` dummy, minted by `parse()` with neither
     filename nor parent (`parser.rs:357,373,483`);
   - **N3′** quarto-xml's hardcoded `FileId(0)` in *production*
     (`crates/quarto-xml/src/parser.rs:27,43,83`) — no filename anywhere in
     its API;
   - **N4** `SourceInfo::default()` → `Original{FileId(0), 0, 0}` (deprecated
     but reachable via `unwrap_or_default()`; live at
     `crates/pampa/src/readers/json.rs:2656-2824` and
     `crates/pampa/src/writers/json.rs:945,3118`).
4. **`SourceContext::get_file` falls back to positional indexing**
   (`context.rs:147-155`): an id absent from the sparse map resolves to
   `files[id.0]`. Every `FileId(0)` dummy therefore aliases to whatever file
   was registered first — in q2, always the primary document. A dummy span
   *renders successfully against the wrong file*; nothing fails.
5. **`resolve_byte_range()` returns `(usize, usize, usize)`**
   (`source_info.rs:388-408`) — it unwraps the newtype, so every consumer
   juggles a raw usize and re-wraps it (or forgets to, as in
   `attribution_render.rs`'s `file_id != 0` gate).
6. **Content is (re)read from disk at render time.** `add_file(path, None)`
   reads for `FileInformation` at registration; the ariadne renderer
   (`quarto-error-reporting/src/diagnostic.rs:794-800`) and `map_offset`
   (`quarto-source-map/src/mapping.rs:31-35`) read *again* at render. Under
   watch/preview, offsets computed against parse-time content can be rendered
   against newer disk content — the same wrong-text failure with no wrong
   binding anywhere. **[verified]**

Consequences observed: confidently-wrong ariadne spans, silently dropped
snippets, and (outside diagnostics) mis-sliced bytes in the incremental writer
and misattributed provenance in the engine pipeline.

One more span-level representable bad state: `map_offset` on a `Concat`
resolves each piece independently, so a single diagnostic's start and end can
land in **different files** (`mapping.rs:54-73`); the renderer draws the
excerpt from `root_file_id()` regardless. **[verified]**

---

## 2. Findings — q2

### 2.1 Wrong-file binding at diagnostic sites (the PR #478 class)

| # | Site | Status |
|---|------|--------|
| D1 | `render_scripts.rs` `script_error` | **fixed** on PR branch via `bind_config_source` (candidates: `config_path` + `extension_manifest_paths`; list proven complete for scripts) |
| D2 | `project_resources.rs` project-level (Q-5-1) | **fixed** on PR branch (`resource_error_to_config_parse_error`) |
| D3 | `project_resources.rs:864` doc-level `resource_error_to_parse_error`, callers at `:1035`/`:1060` (PR branch) | **SUSPECT — real, reachable.** Doc-level `resources:` patterns come from *merged* metadata (`document_profile.rs:117-124` ← `doc.ast.meta` post-`MetadataMergeStage`), so a pattern written in `blog/_metadata.yml` (or `_quarto.yml`, or an extension's `contributes.metadata`) carries that file's hash FileId while the call binds `doc_source_abs` (the `.qmd`). Same two symptoms as bd-m6wmztln. The PR's own docstring names this caveat and defers it here. **[agent, matches PR docstring]** |
| D4 | `project/mod.rs:903` `project_type_error` (Q-5-17) | **Correct today only by call ordering** — both error returns provably happen before any extension-fragment merge, so the fid always equals `hash(config_path)`. Unchecked, undocumented at the site, no guarding test; breaks silently if the merge moves or the helper is reused post-merge. **[agent, data-flow traced]** |
| D5 | `commands/render.rs:1078` (`config_source_context`) and `:1086-1119` (`attach_config_source`) | **Correct** — `attach_config_source` re-derives the hash and refuses to bind on mismatch (`:1113`). Candidate list is `{config_path}` only → extension/`_metadata.yml` spans degrade span-less (coverage gap, not corruption). |
| D6 | `theme_diagnostic.rs:69` + `compile_theme_css.rs:633-643` | **Correct** (the original precedent). Candidates: `{config_path (N2), document input (N1 FileId(0))}`. A theme set in `_metadata.yml` or an extension format degrades span-less. |
| D7 | `metadata_merge.rs:299-313` `register` | **Correct pattern** — id, path, and content all derived from one path (this is the shape the API *should* force). Gap: extension manifests are never registered, so doc-scoped diagnostics anchored in `_extension.yml` are span-less. |
| D8 | `glob_resolve.rs:92`, `glob/provenance.rs:154`, `span_assert.rs:142` | Test-only, self-consistent. |
| D9 | `pipeline.rs:800-802` (also `:834`, `:1006`) | **Coverage gap:** the `StageError` fallback arm builds a fresh single-file context, discarding the real multi-file context `MetadataMergeStage` populated — any stage diagnostic anchored in a config file or include loses its snippet. Not a mis-bind. **[agent]** |

### 2.2 Wrong-file offsets outside the diagnostics path

| # | Site | Finding |
|---|------|---------|
| P1 | `engine_execution.rs:498,518,528-530` | **Live structural bug in every project render with an executable engine. [verified]** Main pipeline order is ParseDocument → **MetadataMerge** → … → IncludeExpansion → … → **EngineExecution** (`pipeline.rs:279-323`). `MetadataMergeStage` registers `_quarto.yml`/`_metadata.yml` into the *same* `source_context` via `add_file_with_id`, growing `files` but not `ast_context.filenames`. `engine_execution.rs:498` then computes the intermediate's slot as `FileId(filenames.len())` — while `add_file` actually assigns `FileId(files.len())`, larger by the number of config registrations — and remaps every executed-AST id by `id.0 + new_slot.0`. Engine-produced blocks end up pointing at the dense slot occupied by `_quarto.yml` (or an include). The comment at `:333-334` ("files are added in lock-step with filenames") is false after MetadataMerge. Downstream: trace attribution wrong; any diagnostic anchored in an engine-produced block renders against config-file text; JSON writer's positional pairing (P4) compounds it. Also the additive remap applied to a tree that *could* contain hash ids is only safe because the executed AST is a fresh parse — an unwritten invariant with no assertion. |
| P2 | `writers/incremental.rs:704-708`, `:747-766` | **[verified at 704-708]** `preimage_in(target_file_id).unwrap_or_else(|| inline_source_span(..))` then `result.push_str(&original_qmd[range])`. `preimage_in` returns `None` for an Original inline **in a different file**, at which point the fallback supplies that foreign file's offsets and slices them out of `original_qmd` — wrong bytes copied into the rewritten qmd, or a panic on out-of-range/char-boundary. The comment justifies the fallback for Concat/Generated sentinels but it also catches the foreign-file case. `target_file_id` itself is guessed from the first block that reports one (`:238-243`). `assemble_recursed_container` (`:747-766`) slices with no file check at all. |
| P3 | `treesitter_utils/section.rs:126-139`, `pipe_table.rs:253-261` | **[verified at section.rs]** Caption-hull construction stamps `SourceInfo::original(table.root_file_id().unwrap_or(FileId(0)), table.start, caption.end)` — no check that the caption resolves to the same file, raw `end_offset()` (sentinel 0 for Concat/Generated), and `unwrap_or(FileId(0))` mints the dummy id in-tree. `hull_source_infos` (`postprocess.rs:301-315`) is the same computation done correctly. |
| P4 | `writers/json.rs:1825-1845` (dup at `:4260`), `readers/json.rs:1503-1562` | JSON writer emits the file table by pairing `filenames[idx]` with `files[idx]` **positionally**; reader re-densifies with `add_file`/`add_file_with_info` in array order. Any N2 hash id surviving into the serialized AST (post-MetadataMerge, or the desynced state from P1) is silently lost or mis-paired on round-trip. WASM entry points (`wasm-quarto-hub-client/src/lib.rs:903-908`) construct `filenames: vec!["/input.qmd"]` against a multi-file context. **[agent]** |
| P5 | `transforms/attribution_render.rs:176-181` | Gates blame attribution on the raw literal `file_id != 0` — a cross-namespace comparison relying on "slot 0 = the blamed doc" with no assertion; `AttributionMap` carries no file identity to catch mismatch. Degrades to silent misattribution if slot 0 changes meaning. **[agent]** |
| P6 | `pampa/src/lua/types.rs:1042-1053` + `lua/quarto_api.rs:387-392` | `si:byte_range()` hands Lua `{start, end}` with the fid discarded; `quarto.attribution.lookup_range(start, end)` is a public primitive, so the natural filter idiom silently misattributes include-spliced nodes. The bundled `lookup` thunk guards; user filters must each rediscover the rule. **[agent]** |

### 2.3 Namespace-collision exposure (latent)

- Any of the diagnostic sites that do `SourceContext::new()` +
  `add_file_with_id(FileId(fid), …)` will, for an N3/N3′/N4 dummy span
  (`fid == 0`), register the config file at *dense index 0* — making a
  genuinely-dummy span render as a real location. `render.rs:1104-1115` is the
  only site whose hash-equality guard also rejects dummies.
- `quarto_xml::parse`'s `FileId(0)` spans (N3′) are in production; if an XML
  diagnostic ever reaches a renderer with a populated context, it renders
  against the primary document.
- Bare `quarto_yaml::parse()` (N3) is test-only in q2 today — one production
  call away from live.
- `span_assert.rs:154-161` recognizes only the exactly-`{0,0,0}` default;
  `FileId(0), 12, 25` sails through.

---

## 3. Findings — quarto-yaml (upstream repo)

| # | Finding |
|---|---------|
| Y1 | **`parse_with_parent`'s contract is unchecked** (`parser.rs:92-94`): the parent must describe exactly `content` (origin at byte 0 of `content`, length = `content.len()`), but nothing validates it, `SourceInfo::substring` stores offsets verbatim, and `resolve_byte_range` composes with **no clamp to the parent's end** (`source-map/source_info.rs:400-403`). A misaligned parent yields plausible in-file offsets at the wrong place. **The crate's own doc example violates the contract** (`parser.rs:69-87`: narrates "extracted at offset 10-50" while passing a `0..1000` parent; `rust,no_run` so never executed). q2's two call sites are correct by carefully-maintained convention: `cell_options/mod.rs:203-222` (parallel push loops, no length assertion), `jupyter/text_execute.rs:272-274` (sound only while engine input is byte-identical through the frontmatter). |
| Y2 | **The `FileId(0)` dummy** (`parser.rs:357,373,483`) — see N3 above. Combined with `get_file`'s positional fallback this is the crate's contribution to the aliasing hazard. |
| Y3 | `error.rs`: all variants carry `location: Option<SourceInfo>` but `Display` drops it and the only live error path (`From<ScanError>`, `:67-74`) sets `location: None` — so no wrong-file risk *and* no location at all. Trap for a future fix: `ScanError`'s index is a **character** index; the parser's own `byte_offset_of_char` (`parser.rs:264-298`) exists for this conversion. |
| Y4 | `hasher.finish() as usize` truncates to 32 bits on wasm32. `add_file_with_id` **panics** on duplicate ids, so a birthday collision (~2^16 files) becomes a crash in the hub client. q2 guards at exactly one site (`metadata_merge.rs:305-312`). |
| Y5 | Public constructors accept arbitrary span combinations with zero consistency checks: `YamlHashEntry::new` takes five independent spans (`yaml_with_source_info.rs:254-268`); `new_hash`/`new_array` accept children from other files; `with_tag` attaches any span to any node. In-parser use is safe; the API invites hand-assembled inconsistent trees. |
| Y6 | `create_contiguous_span` (`parser.rs:159-201`): the same-file assert in the Original arm is unreachable today (single builder, fixed parent), but the Substring arm silently drops `end_info`'s parent — the arm that *would* mint a hybrid span if per-node parents ever appear. |

### quarto-yaml-validation (latent for q2 — q2 does not depend on it; verified)

| # | Finding |
|---|---------|
| V1 | **Context/value pairing is unenforced end-to-end**: `validate(value, schema, registry, source_ctx)` accepts any ctx for any value; `ValidationDiagnostic::from_validation_error(err, ctx_A)` computes ranges with one context and `to_text(&self, ctx_B)` renders with another (`diagnostic.rs:128`, `:201-203`). Constructing with one context and rendering with another is the API working as designed — and is exactly the wrong-file render. |
| V2 | **`SourceRange` mixes coordinate spaces** (`diagnostic.rs:326-334`): `filename` from the fully-resolved root file, `start_offset`/`end_offset` from `start_offset()`/`end_offset()` — parent-relative for every `Substring` (i.e., every parsed node). Coincidentally correct for `parse_file`, wrong for `parse_with_parent` (frontmatter): JSON consumers slicing the named file at those offsets get the wrong text while line/column in the same struct are right. Also `end - start` underflow-panics for unordered hand-built spans (`:320`). |
| V3 | `diagnostic.rs:543-553` re-implements the filename hash inline instead of calling `file_id_for_filename` — a hash-recipe change would silently desynchronize it. |
| V4 | Good news: `Schema` discards spans after parsing and all 24 `add_error` sites pass instance nodes, so schema-file spans cannot leak into instance diagnostics through the normal path. |

---

## 4. Findings — quarto-source-map (upstream repo)

These are the API decisions that make everything above representable:

1. `FileId(pub usize)` — public constructor, no provenance, one flat namespace.
2. `add_file_with_id` — arbitrary (id, path, content) pairing; panics on
   duplicates (the failure mode Y4 turns into a crash).
3. `get_file` positional fallback — dense and sparse namespaces silently alias.
4. `resolve_byte_range` — returns raw `usize` fid.
5. No clamping/validation anywhere: `SourceInfo::original` accepts `end < start`;
   `substring` accepts child ranges exceeding the parent; `resolve_byte_range`
   composes without bounds.
6. Content re-read from disk at render time (drift under watch/preview);
   `add_file(path, None)` even *silently* tolerates a missing file at
   registration and fails only at render.
7. `map_offset` on `Concat` can resolve endpoints of one span into different
   files.

---

## 5. Design proposals

The unifying principle: **the (identity, content) binding exists exactly once —
at parse time. Every bug in §2.1 comes from discarding that binding and
reconstructing it at diagnostic time.** The API should make reconstruction
unnecessary (and ideally impossible).

### Option A — shallow: guard every site, lint the pattern (q2-only, days)

1. Migrate D3, D4 (and D5/D6's candidate lists) to `bind_config_source`;
   extend candidates with `extension_manifest_paths` + the doc's directory
   `_metadata.yml` layers (already enumerable at the call sites via
   `directory_metadata_for_document`).
2. Fix P1 by using the id actually returned by `add_file` (and
   `debug_assert!` the lock-step claim); fix P2 by returning `None` (re-serialize)
   instead of the foreign-offset fallback; fix P3 by routing through
   `hull_source_infos`; fix P5 by threading the blamed `FileId` onto
   `AttributionData`; fix P6 by returning the fid from `byte_range()`.
3. Add an xtask lint: `add_file_with_id` is allowed only in blessed modules
   (`config_sources.rs`, `metadata_merge.rs`, test helpers) — everything else
   must go through `bind_config_source`.

Honest assessment: this fixes every *known* site and prevents the known
pattern, but the bad state stays representable; new call sites in new shapes
(P1 was not a diagnostic site at all) will keep appearing.

### Option B — mid: kill the split at the API seam (quarto-source-map + quarto-yaml, ~1-2 weeks)

1. **Move the path→id derivation into quarto-source-map.** The filename-hash
   scheme is not YAML-specific. Add `FileId::for_path(&str)` (same recipe,
   width-stable `u64`-based to fix Y4's wasm32 truncation — a breaking hash
   change, coordinated with quarto-yaml's re-export) and deprecate
   `quarto_yaml::file_id_for_filename` in favor of re-exporting it.
2. **Replace `add_file_with_id` with `bind_path(path, content) -> FileId`**:
   the context derives the id internally; a caller can no longer pair an
   arbitrary id with arbitrary content. Duplicate binding with identical
   content is a no-op; with different content, first-wins + a debug warning
   (not a panic). `add_file_with_id` gets deprecated and removed after q2
   migrates.
3. **Remove the positional fallback in `get_file`** (or equivalently: make
   `add_file` also record its id in the sparse map, so lookup is uniform and
   never aliases).
4. **Reserve an explicit unknown id** (`FileId::UNKNOWN = usize::MAX`);
   quarto-yaml's and quarto-xml's dummies mint it instead of 0; `get_file`
   and `map_offset` refuse it. (Alternatively quarto-yaml's `parse()` uses
   `FileId::for_path("<anonymous>")`.)
5. **`resolve_byte_range` returns `(FileId, Range<usize>)`**, not raw usizes.
6. Validate at construction: `debug_assert!(start <= end)` in
   `original`/`substring`; `parse_with_parent` debug-asserts
   `parent.length() == content.len()`; fix the doc example (Y1).

This makes the *pairing* unrepresentable while keeping FileId serializable and
the overall architecture intact. It does not solve content drift (§1.6) or the
dense-table positional serialization (P4), and the dense namespace still
exists for documents.

**VFS constraint (review decision, 2026-08-09).** Everything must work under
the hub-client's Automerge VFS (`/project/...` paths, no host filesystem).
This constrains B in three ways, none of which raise its risk:

- **Paths stay opaque string keys — no canonicalization in the library.**
  Today's `file_id_for_filename` hashes the given spelling and interprets
  nothing, which is why it already works under the VFS; `bind_path` /
  `FileId::for_path` keep exactly that semantics. Canonicalization is where
  OS/VFS divergence lives (`canonicalize` doesn't exist in the VFS; macOS
  `/tmp` symlinks; Windows case folding) and is explicitly out of scope for
  quarto-source-map. If path-spelling instability needs fixing (§7.1), fix it
  at q2's minting boundary — project-relative spelling is both the stable and
  the VFS-native representation.
- **`bind_path` takes content mandatorily** (no `Option` + disk-read
  fallback). The caller reads via its runtime (host FS or VFS) and hands over
  the string. This *removes* filesystem awareness from quarto-source-map
  relative to today — the library's current disk touchpoints (`add_file(path,
  None)`, `map_offset`'s and the renderer's render-time re-reads) are exactly
  what already degrades in WASM.
- **Prefer the uniform-lookup variant of B.3** (have `add_file` also record
  its id in the sparse map) over deleting the positional fallback outright:
  it closes the dummy-aliasing hole without touching any dense-id caller
  (pampa's `FileId(0)` convention survives unchanged).

Net risk assessment: B minus canonicalization is a relocation of existing
semantics plus deletion of footguns, not a re-conceptualization of paths —
low engineering risk, no path-representation refactoring needed ahead of it.
Option C is the most VFS-friendly end-state of all: a handle pinning
parse-time content means rendering needs no filesystem anywhere.

### Option C — deep: identity carries its binding (quarto-source-map redesign, larger)

Replace `Original { file_id, start, end }` with
`Original { file: Arc<SourceFileHandle>, start, end }` where the handle owns
path + content (+ lazily-built `FileInformation`). `SourceContext` becomes an
interner producing handles; diagnostic sites stop building contexts entirely —
the span *is* renderable by itself. FileId survives only as a serialization
artifact (the JSON writer already emits an id table; the reader reconstructs
handles). Consequences:

- The entire §2.1 class is unrepresentable — there is no rebinding step.
- Content drift is gone: the handle pins the parsed content.
- `remap_file_ids` disappears (merging ASTs needs no id shifting), which
  also deletes P1's arithmetic and P4's positional pairing.
- Costs: one pointer per Original (vs usize today — `Substring` already pays
  an Arc); serde needs interning on write (already true); every crate that
  touches `SourceInfo::Original` changes; memory pinned for content of every
  file with live spans (in practice these are files we render diagnostics
  about — we want the parse-time content anyway).

My recommendation: **B now, C as the stated end-state**, with A's items 2
(the P-series fixes) and 3 (the lint) done immediately in q2 regardless —
they're bugs and a guardrail independent of the upstream API question. The
B→C split keeps the external-crate release small and unblocks q2 fixes while
the C design gets its own review in the quarto-source-map repo.

### Renderer defense-in-depth (quarto-error-reporting, independent)

- Refuse to render an `Original` span whose offsets exceed the bound file's
  length — degrade span-less instead of clamping (today's clamp at
  `diagnostic.rs:808-818` turns detectable mis-binds into plausible spans).
- When start/end of a `Concat` map into different files, drop the span.

### q2-internal structural cleanups (independent of upstream)

- **Retire `ASTContext.filenames`** (or make it derived): the parallel table
  is the enabling condition for P1/P4. Single source of truth =
  `SourceContext`.
- Thread the document's *real* SourceContext into the `StageError` fallback
  arm (D9) instead of rebuilding a fresh one.
- Register extension manifests in `MetadataMergeStage` (D7 gap) so doc-scoped
  diagnostics anchored in `_extension.yml` get spans.

---

## 6. Proposed work breakdown

**Agreed sequencing (review, 2026-08-09):**

1. **Option A in q2, now** — P-series bug fixes, `bind_config_source`
   migrations + candidate-list completion, xtask lint on `add_file_with_id`.
2. **Then a memo for a separate agent on posit-dev/quarto-source-map** —
   Option B API (per the VFS-constrained shape in §5), deprecation plan for
   `add_file_with_id` / positional fallback / raw `resolve_byte_range`
   (published crates: deprecate one release, remove later), migration plan
   for quarto-yaml. The memo declares Option C as the stated end-state so
   B's API is designed forward-compatibly (e.g. `bind_path` returning
   something that can later become a handle) — migration churn paid once.
   Note: **quarto-xml is an in-tree q2 crate**; its move off the hardcoded
   `FileId(0)` is a q2 strand riding the version bump, not memo scope.
3. **Fold B's migration lessons into the concrete Option C plan**
   (`Original` carries its source binding).

Filed now (discovered-from bd-nv4p0eb1):

- **bug (p1):** P1 engine-slot desync (`engine_execution.rs:498`) — filed as
  its own strand; needs a fixture: project + `_quarto.yml` + jupyter/knitr
  cell, assert engine-block FileIds resolve to the intermediate, not the
  config.
- **bug (p2):** D3 doc-level `resource_error_to_parse_error` mis-bind — the
  direct sibling of bd-p86nlm92.

Proposed after plan review (not yet filed):

- q2: P2 incremental-writer foreign-offset fallback; P3 hull construction;
  P5 attribution FileId threading; P6 Lua `byte_range` fid; D4 hardening;
  D5/D6 candidate-list completion; D9 context threading; xtask lint;
  `filenames` retirement.
- quarto-source-map (GH issues): Option B items 1-6; renderer defense items;
  Option C design RFC.
- quarto-yaml (GH issues): Y1 (assert + doc example), Y2/N3 dummy → UNKNOWN,
  Y4 hash width, V1-V3 validation-crate fixes, Y5 constructor validation.
- quarto-xml: replace hardcoded `FileId(0)` with the same scheme (in-tree,
  can ride any q2 strand).

## 7. Open questions for review

1. **Hash-of-path-spelling as identity** is itself fragile: relative vs
   absolute spelling changes the id (PR #478's correctness argument required a
   paragraph tracing `display()` vs `to_string_lossy()` and
   `parent().join(...)` reconstruction). Do we canonicalize before hashing
   (Option B.1 could), or is path-spelling identity a deliberate choice
   (same file via two spellings = two ids)?
   *Direction after VFS-constraint review: no canonicalization in the
   library (see §5, Option B's VFS note); normalize spelling at q2's minting
   boundary instead. The per-setting canonicalizer already exists:
   `SystemRuntime::canonicalize` (`quarto-system-runtime/src/traits.rs:297-302`;
   native = `std::fs::canonicalize`, WASM = VFS `normalize_path`, sandbox
   delegates). What's needed is policy, not infrastructure: identity-minting
   sites canonicalize before hashing. Caveat: canonical spelling is for the
   hash only — native canonicalize resolves symlinks (macOS `/tmp` →
   `/private/tmp`), so keep the original spelling in `SourceFile.path` for
   display. Residual risk accepted for now: `foo/../X` vs `X` = two FileIds,
   whose failure mode is degradation (missed candidate match → span-less, or
   double registration with both resolving correctly), never wrong-file
   rendering.*
2. Same path, different content over time (watch/preview): under B, first-wins
   binding means stale content can pin; under C the span pins parse-time
   content by construction. Is pinning parse-time content the desired
   semantics for preview diagnostics? (I believe yes — offsets are meaningless
   against newer content.)
   *Resolved (review, 2026-08-09): yes — pin parse-time content.*
3. Is Option C's per-node `Arc` + content pinning acceptable for the WASM/hub
   memory budget? (Substring already pays Arc; the delta is Original leaves.)
   *Resolved (review, 2026-08-09): acceptable — err on the side of
   correctness over lean-but-wrong; no robust real-life hub-client perf
   measurements exist to argue otherwise. Constraint: nothing worse than the
   current complexity class (O(n) stays O(n)). (`Arc` has no wasm32
   compilation constraints; wasm32-unknown-unknown is single-threaded but
   `Arc` compiles and works normally.)*
4. Priority of the validation-crate fixes (V1-V3) given q2 doesn't consume it
   but external Posit consumers do.
   *Resolved (review, 2026-08-09): file a GitHub issue on
   posit-dev/quarto-yaml recording the concern and context (braid skeins are
   not publicly readable), noting the fixes ride the planned API redesign.*
