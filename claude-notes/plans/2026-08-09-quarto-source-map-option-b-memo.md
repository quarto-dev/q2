# Memo: quarto-source-map binding-API redesign (Option B), with Option C as the declared end-state

**Audience:** the agent/session working in `posit-dev/quarto-source-map`
(and, for the migration leg, `posit-dev/quarto-yaml`).
**Origin:** q2's FileId/span integrity audit, bd-nv4p0eb1 —
`claude-notes/research/2026-08-09-fileid-span-integrity-audit.md` (in the
quarto-dev/q2 repo; read it first, especially §1, §4, §5).
**Public context:** posit-dev/quarto-yaml#17 records the quarto-yaml-side
findings; q2 PR #482 and the follow-up phase-A branch contain the q2-side
mitigations that this redesign makes structurally unnecessary.

## Why (one paragraph)

A source span is semantically a triple *(file identity, byte range, file
content)*. The current API lets each leg travel and be re-bound
independently: `FileId(pub usize)` is freely constructible,
`SourceContext::add_file_with_id(id, path, content)` accepts any pairing,
`get_file` silently aliases unregistered small ids onto dense slots, and
content is re-read from disk at render time. q2 has now fixed ten+ concrete
bugs of this class (wrong-file ariadne spans, mis-sliced writer output,
misattributed provenance) and installed guards/lints — but the bad state
remains representable, and new call sites in new shapes will keep
appearing. Option B makes the *pairing* unrepresentable at the API seam;
Option C (the declared end-state) makes the whole rebinding step
nonexistent.

## Constraints (already decided in q2 review — do not relitigate)

1. **Paths are opaque string keys.** No canonicalization, no path
   interpretation, no filesystem probing inside quarto-source-map. This is
   what makes the scheme work identically under q2's hub-client VFS
   (`/project/...` paths) and native. If spelling instability needs
   fixing, it happens at the consumer's minting boundary
   (q2's `SystemRuntime::canonicalize` is the per-setting normalizer).
2. **Binding takes content, mandatorily.** `bind_path(path, content)` —
   the caller reads via its own runtime and hands over the string. This
   *removes* filesystem awareness from the library (today's
   `add_file(path, None)` disk fallback, and the render-time re-reads in
   `map_offset`, are exactly what degrades in WASM and what drifts under
   watch/preview). Pin-parse-time-content semantics are the decided
   contract.
3. **Uniform lookup, not fallback removal.** Close the dense/sparse
   aliasing hole by having `add_file` also record its id in the sparse
   map, so lookup is uniform — do NOT delete positional resolution
   outright (pampa's `FileId(0)` convention must keep working unchanged).
4. **Complexity class stays O(n).** No per-node memory blowups.

## Option B work items (quarto-source-map)

1. **`FileId::for_path(&str) -> FileId`** — move the path→id derivation
   into quarto-source-map. Same recipe as
   `quarto_yaml::file_id_for_filename` *for now* (DefaultHasher over the
   string), BUT: make the hash width stable across targets (u64-based,
   not `as usize` — the usize truncation to 32 bits on wasm32 plus
   `add_file_with_id`'s panic-on-duplicate is a latent crash,
   posit-dev/quarto-yaml#17 item Y4). This is a hash-recipe change on
   wasm32 only; coordinate the release with quarto-yaml re-exporting the
   new function (see migration below). Note `FileId(pub usize)` likely
   needs to become u64-backed or the hash needs masking — design point
   for the implementing agent; the constraint is: identical ids for
   identical spellings on every target.
2. **`SourceContext::bind_path(path: &str, content: String) -> FileId`**
   — derives the id internally via `FileId::for_path`; a caller can no
   longer pair an arbitrary id with arbitrary content. Duplicate binding
   with identical content: no-op returning the id. With different
   content: first-wins + a debug-log (never a panic).
3. **Reserve `FileId::UNKNOWN` (`usize::MAX`)** — the sentinel for
   anonymous/synthetic provenance. `get_file` and `map_offset` return
   `None` for it unconditionally. (q2's quarto-xml already mints
   `FileId(usize::MAX)` as `ANONYMOUS_FILE_ID` in anticipation; quarto-yaml's
   `parse()` dummy `FileId(0)` migrates to it — Y2.)
4. **Uniform lookup** (constraint 3): `add_file` registers in the sparse
   map too; `get_file` consults the map only. Dense ids keep resolving;
   `FileId(0)` dummies stop aliasing.
5. **Typed `resolve_byte_range`** — return `(FileId, Range<usize>)`, not
   `(usize, usize, usize)`. Deprecate the old shape for one release.
6. **Construction validation** — `debug_assert!(start <= end)` in
   `original`/`substring`; `substring` debug-asserts child range within
   parent length where knowable.
7. **Renderer defense** (quarto-error-reporting, separate crate, same
   family): refuse to render an `Original` span whose offsets exceed the
   bound file's length (degrade span-less; today's clamp turns detectable
   mis-binds into plausible spans), and drop the span when a `Concat`'s
   endpoints map into different files.
8. **Deprecations:** `add_file_with_id` (replaced by `bind_path`; one
   release deprecated, then removed), raw-tuple `resolve_byte_range`
   (same cycle). These crates have external Posit consumers — follow
   quarto-yaml's release runbook (version-bump PR; CI publishes).

## quarto-yaml migration (posit-dev/quarto-yaml)

- Re-export `FileId::for_path` as `file_id_for_filename` (deprecated
  alias) so the documented stability contract holds through the
  transition; parse-side id minting switches to the source-map function.
- `parse()`'s `FileId(0)` dummy → `FileId::UNKNOWN` (issue #17, Y2).
- `parse_with_parent`: `debug_assert!(parent.length() == content.len())`
  in `parse_impl` + fix the wrong doc example (Y1 — the example passes a
  `0..1000` parent while narrating "extracted at offset 10-50").
- Validation-crate items V1-V3 from issue #17 ride along where cheap
  (single owning context handle; `SourceRange` offsets from
  `resolve_byte_range` so they share a base with `filename`; delete the
  inline hash re-implementation in `diagnostic.rs` test helpers).

## q2 follow-up (after the release lands; file as q2 strands then)

- Bump pins; migrate `bind_config_source` / `bind_source_candidates` /
  `register_config_source` internals to `bind_path`; extend the
  `add-file-with-id` xtask lint to flag the deprecated API tree-wide.
- quarto-xml: replace `ANONYMOUS_FILE_ID` with the upstream
  `FileId::UNKNOWN`.
- Re-audit `span_assert.rs`'s `SuspiciousDefault` heuristic (the
  `{0,0,0}` special case) against the new sentinel.

## Option C — the declared end-state (design RFC, not this release)

`Original { file: Arc<SourceFileHandle>, start, end }` where the handle
owns path + pinned parse-time content (+ lazy `FileInformation`).
`SourceContext` becomes an interner producing handles; diagnostic sites
stop building contexts entirely; `remap_file_ids` and the positional
file-table serialization (q2's P4) dissolve. FileId survives only as a
serialization artifact. **Design B's API so C is a body-swap, not a
signature churn** — e.g. `bind_path` returning a `FileId` today can
return a handle-convertible newtype tomorrow; don't add new API that
leaks the usize-ness of ids. Memory cost was reviewed and accepted in q2
(Substring already pays an Arc per node; content pinning is bounded by
files with live spans). Write the C RFC in the quarto-source-map repo
after B ships, folding in what the B migration teaches.

## Acceptance criteria for B

- A test in quarto-source-map proving the old bad state is
  unrepresentable through the new API (no way to bind content under a
  non-derived id).
- A test proving `FileId(0)`-dummy spans resolve to `None` in a context
  with dense files (the aliasing hole).
- wasm32 + native id-equality test for `FileId::for_path`.
- quarto-yaml's suite green against the new source-map release;
  q2's workspace green after the pin bump (q2 runs its own e2e span
  tests: `extension_config_spans.rs`, `render_scripts_cli.rs`,
  `unknown_project_type.rs`, `table_caption_provenance.rs`).
