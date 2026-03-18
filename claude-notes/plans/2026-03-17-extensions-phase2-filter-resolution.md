# Extensions Phase 2: Extension Filter Resolution

**Created**: 2026-03-17
**Status**: Not Started
**Parent Plan**: `claude-notes/plans/2026-03-16-extensions-master-plan.md`
**Depends on**: Phase 1 (complete), Lua filter support (complete, rebased)

## What Phase 1 Built (already on this branch)

Phase 1 created the extension infrastructure in `crates/quarto-core/src/extension/`:

- **`types.rs`**: `Extension`, `ExtensionId`, `Contributes`, `ExtensionFilter` structs
- **`read.rs`**: `read_extension()` parses `_extension.yml` → `Extension`. Includes
  `mark_path_valued_keys()` which converts known path-valued keys (`template`,
  `template-partials`) from `ConfigValueKind::Scalar` to `ConfigValueKind::Path`
  so they get rebased during metadata merge.
- **`discover.rs`**: `discover_extensions()` walks `_extensions/` directories.
  `find_extension()` looks up an extension by name. `parse_format_descriptor()`
  splits `"acm-html"` → extension `"acm"` + base format `"html"`.
- **`StageContext`** (`crates/quarto-core/src/stage/context.rs`): Has
  `pub extensions: Vec<Extension>`, populated during context creation.
- **`MetadataMergeStage`** (`crates/quarto-core/src/stage/stages/metadata_merge.rs`):
  Inserts extension metadata as a layer between Project and Directory layers.
  Calls `adjust_paths_to_document_dir()` on the extension layer to rebase
  `ConfigValueKind::Path` values from extension dir to document dir.

### The `ConfigValueKind::Path` mechanism

`ConfigValue` is the universal metadata type. Its `value` field is a
`ConfigValueKind` enum with variants including `Scalar(Yaml)`, `Map(...)`,
`Array(...)`, and **`Path(String)`**. The `Path` variant marks a value as a
filesystem path relative to its origin directory.

During metadata merge, `adjust_paths_to_document_dir(config, origin_dir, doc_dir)`
walks a `ConfigValue` tree and rebases every `Path` node:
1. Joins the path with `origin_dir` to get an absolute path
2. Makes it relative to `doc_dir`
3. Result: a document-relative path that `filter_resolve.rs` can later join
   with `document_dir` to get the correct absolute path

Key: `as_str()` and `as_plain_text()` both return `Some` for `Path` values,
so downstream code that reads string values works unchanged after marking.

## Overview

Extensions can contribute Lua/JSON filters in two ways:

1. **Per-format filters**: Declared in `contributes.formats.<format>.filters` within
   `_extension.yml`. These flow through metadata merge as part of format config.
2. **Name-based filter extensions**: Standalone extensions with `contributes.filters`.
   Referenced by name in document metadata (e.g., `filters: [lightbox]`).

This phase wires both mechanisms into the existing filter resolution pipeline.

### Goals

1. Per-format extension filters (in format metadata) have correct absolute paths
   and are applied during rendering
2. Filter extension names in document `filters` metadata are resolved to the
   extension's contributed filter paths
3. Extension filters and user filters are correctly ordered and respect entry
   points (`at` field) and the `quarto` sentinel
4. Extension filter resolution errors produce clear diagnostics

### Non-Goals

- Embedded extension resolution (Phase 9) — filter names that reference embedded
  extensions within other extensions
- Custom Lua writers (Phase 5)
- Filter execution changes — pampa's `apply_filters()` is unchanged

## How TS Quarto Does It

Confirmed via DeepWiki research on `quarto-dev/quarto-cli`:

1. **Per-format filters**: Format extension metadata (including `filters`) flows
   through `resolveFormats()`. Filter paths are resolved to absolute via
   `resolveFilterPath()` which joins relative paths with `extensionDir`.

2. **Name-based resolution**: `resolveFilterExtension()` in `filters.ts` checks
   if a filter string matches an existing file (`existsSync`). If not, it uses
   `options.services.extension?.find` to search for an extension contributing
   filters. If found, the extension's filters are substituted.

3. **Merge behavior**: Extension format-level filters and user filters are
   **concatenated** (extension first, user appended). The `quarto` sentinel
   and `at` field work normally within the concatenated array.

4. **Ordering**: Extension format filters come first (lower priority in merge),
   user filters are appended. Within each group, the `quarto` sentinel splits
   pre/post, and `at` overrides the default.

## What q2 Already Has

### Filter resolution (`crates/quarto-core/src/filter_resolve.rs`)

- `resolve_filters(meta, document_dir) -> ResolvedFilters` reads `meta["filters"]`
- `ResolvedFilters` has `pre: Vec<FilterSpec>` and `post: Vec<FilterSpec>`
- `FilterSpec` enum (from `pampa::unified_filter`): `Citeproc`, `Lua(PathBuf)`,
  `Json(PathBuf)`. `FilterSpec::parse(s: &str)` → `Citeproc` if `s == "citeproc"`,
  `Lua` if `.lua` extension, `Json` otherwise. Accepts `&str` (and `Cow<str>`
  auto-derefs to `&str`).
- Finds `quarto` sentinel, assigns entry points, splits into pre/post groups
- `parse_filter_item()` handles string form (`"filter.lua"`) and map form
  (`{path: "filter.lua", at: "post-render"}`)
- Entry point mechanism: 8 named entry points map to `Pre` or `Post` position.
  Before the `quarto` sentinel → default `pre-quarto` (Pre). After sentinel →
  default `post-render` (Post). The `at` field overrides the default.
  `entry_point_index(name) -> Option<usize>` returns the sort key.
  Filters are stable-sorted by `(entry_point_index, original_index)`.
- `resolve_filter_path()` joins relative paths with `document_dir`; absolute
  paths pass through unchanged.
- **No awareness of extensions**

### User filters stage (`crates/quarto-core/src/stage/stages/user_filters.rs`)

- Two instances: `UserFiltersStage::pre()` and `UserFiltersStage::post()`
- Calls `resolve_filters(&doc.ast.meta, document_dir)` then
  `pampa::unified_filter::apply_filters()`
- Has access to `ctx.extensions` via `StageContext`
- **Does not pass extensions to `resolve_filters()`**

### Extension filter storage (`crates/quarto-core/src/extension/types.rs`)

- `ExtensionFilter { path: PathBuf, at: Option<String> }` — absolute paths,
  optional entry point
- `Contributes.filters: Vec<ExtensionFilter>` — top-level contributed filters
- `Contributes.formats: HashMap<String, ConfigValue>` — format metadata
  (may contain `filters` key with relative paths as plain strings)

### ConfigValue merge behavior

- `MergeOp::Concat` is the **default** for arrays
- During merge, lower-priority arrays are prepended, higher-priority appended
- This means extension format filters (lower priority) come before user filters
  (higher priority) after merge — exactly what we want
- No special handling needed; the existing merge infrastructure concatenates
  filter arrays correctly

### Per-format filter paths (current problem)

Filter paths in `contributes.formats.<format>.filters` are **plain strings**
relative to the extension directory. After metadata merge, they appear in
`meta["filters"]` as strings. `filter_resolve.rs` resolves them against the
**document directory**, which is wrong — they should resolve against the
extension directory.

Example: extension at `/project/_extensions/acm/` with `filters: [filter.lua]`.
Document at `/project/posts/doc.qmd`. After merge, `meta["filters"]` contains
`"filter.lua"`. `filter_resolve.rs` resolves to `/project/posts/filter.lua`
(wrong). Should be `/project/_extensions/acm/filter.lua`.

## Design Decisions

### IMPORTANT: Two separate filter-path mechanisms for two separate contexts

Phase 2 touches filters in **two completely different contexts**. Despite
syntactic similarity (both deal with `filters` arrays), they operate on
different data at different stages of the pipeline, with different code paths
and different disambiguation strategies. Do NOT confuse them:

| | **Phase 2.1** | **Phase 2.2** |
|---|---|---|
| **What** | Per-format filter paths in extension `_extension.yml` | Bare extension names in user document metadata |
| **Where in code** | `mark_path_valued_keys()` in `extension/read.rs` | `resolve_filters()` in `filter_resolve.rs` |
| **When it runs** | During `read_extension()` (before merge) | During `UserFiltersStage` (after merge) |
| **Input data** | Extension YAML only — values are always file paths or reserved names (`citeproc`, `quarto`) | Merged metadata — values can be file paths, reserved names, OR extension names |
| **Mechanism** | `ConfigValueKind::Path` marking + `adjust_paths_to_document_dir()` rebasing | File-first existence check + `find_extension()` lookup |
| **Shared code** | None — `mark_path_valued_keys` is in `read.rs`, completely separate | None — `resolve_filters` is in `filter_resolve.rs` |

The syntactic similarity (`filters: [something.lua]`) is misleading. In
extension format metadata, every non-reserved string is a file path relative
to the extension directory. In user document metadata, a bare name like
`"lightbox"` could be a file path OR an extension reference — disambiguation
requires a runtime file-existence check.

### Phase 2.1: Per-format filter path resolution via `!path` marking

**Context**: This runs inside `mark_path_valued_keys()` during
`read_extension()`. It operates on extension `_extension.yml` format metadata
only. User document metadata never passes through this function.

**Decision**: Mark per-format filter path values as `ConfigValueKind::Path` in
`mark_path_valued_keys()`, so that `adjust_paths_to_document_dir()` rebases
them from extension dir to document dir during metadata merge. This is the same
approach used for `template` and `template-partials` (Phase 4), extended to
handle the two filter entry forms.

**How it works**:
1. In `mark_path_valued_keys()`, add handling for the `filters` key
2. For string-form entries: convert to `ConfigValueKind::Path` **unless** the
   string is a reserved filter name (`"citeproc"` or `"quarto"`). Extension
   format metadata never contains bare extension names (that's Phase 9), so
   all non-reserved strings are file paths.
3. For map-form entries (`{path: filter.lua, at: post-render}`): always convert
   the `path` sub-key's value to `ConfigValueKind::Path` (map-form `path`
   values in extension format metadata are always file paths)
4. `adjust_paths_to_document_dir()` (already called on the extension layer in
   `MetadataMergeStage`) walks recursively and rebases all `Path` nodes
5. After merge, `filter_resolve.rs` sees document-relative paths via
   `as_plain_text()` (which works for both `Path` and `Scalar` variants) and
   resolves them against `document_dir` as usual

**Example flow**: Extension at `/project/_extensions/acm/`, document at
`/project/posts/doc.qmd`:
- Parse: `filters: [filter.lua]` → `ConfigValueKind::Path("filter.lua")`
- `adjust_paths_to_document_dir(ext_dir=/project/_extensions/acm/, doc_dir=/project/posts/)`:
  - abs = `/project/_extensions/acm/filter.lua`
  - relative to doc_dir = `../_extensions/acm/filter.lua`
  - Result: `ConfigValueKind::Path("../_extensions/acm/filter.lua")`
- After merge, `meta["filters"]` contains `"../_extensions/acm/filter.lua"`
- `filter_resolve.rs` resolves: `/project/posts/../_extensions/acm/filter.lua`
  → `/project/_extensions/acm/filter.lua` ✓

**Reserved name handling**: Only `"citeproc"` and `"quarto"` are excluded from
`Path` marking. These are the only non-path strings that can appear in
extension format filter arrays. `"citeproc"` must remain as `Scalar` because
rebasing it would produce a nonsense path like `../_extensions/ext/citeproc`.
`"quarto"` is the sentinel and must also not be rebased. Bare extension names
(like `"lightbox"`) never appear in extension format metadata — those only
appear in user document metadata and are handled by Phase 2.2's completely
separate code path.

### Phase 2.2: Name-based resolution in `resolve_filters()`

**Context**: This runs inside `resolve_filters()` during `UserFiltersStage`,
AFTER metadata merge. It operates on the fully merged `meta["filters"]` array,
which may contain a mix of: file paths (from user or rebased from extensions),
reserved names, and bare extension names (from user document metadata).

**Decision**: Modify `resolve_filters()` to accept `&[Extension]` and
`&dyn SystemRuntime`. During filter item parsing, use **file-first resolution**
matching TS Quarto's behavior: check if the name resolves to an existing file,
and only try extension lookup if it doesn't.

**Resolution order** (for both string and map forms):
1. `"citeproc"` → built-in (existing `FilterSpec::parse()` handles this)
2. `"quarto"` → sentinel (already skipped in the loop)
3. File existence check: `runtime.path_exists(&document_dir.join(name), None)`
   → if the path points to an existing file, treat as file path
   (fall through to `parse_filter_item()`)
4. Extension lookup via `find_extension(name, extensions)` → if matched
   and the extension contributes filters, expand to those filters
5. Fall through → treat as file path via `FilterSpec::parse()` (existing
   behavior — handles the case where the file doesn't exist yet or is
   referenced by relative/absolute path)

This matches TS Quarto's `resolveFilterExtension()` at `filters.ts:874-880`:
`existsSync(pathToResolve)` first, extension lookup second. Local files
always take priority over extensions of the same name.

**`runtime.path_exists()` error handling**: If `path_exists()` returns an
error (e.g., VFS error in WASM), we treat it as "file doesn't exist"
(`.unwrap_or(false)`). This is intentional: a transient IO error during
existence checking should not prevent extension lookup from working, and
the worst case is that an extension filter is resolved by name instead of
by file path — which would fail later at execution time with a clear error.

**Both string and map forms**: In TS Quarto (`resolveFilterExtension()`,
`filters.ts:868-872`), name resolution applies to both:
- String form: `"lightbox"` → resolve `lightbox` as extension name
- Map form: `{path: lightbox, at: post-render}` → resolve `lightbox` as
  extension name, propagate `at: post-render` to ALL contributed filters
  (overriding their individual `at` values)

This means the map form serves dual purpose: it's a file path if the value
points to a file, or an extension reference if it matches an extension name.
TS Quarto disambiguates via `existsSync()` (file existence check first, then
extension lookup). We match this: file-first, extension-second.

**`at` propagation from map form**: When `{path: ext-name, at: post-render}`
matches an extension contributing 3 filters with various `at` values, the
map's `at` overrides ALL of them. This lets users control where an extension's
filters run. Confirmed in TS Quarto at `filters.ts:904-919`.

### Expansion of multi-filter extensions and `at` entry point priority

When an extension contributes multiple filters, expanding its name produces
multiple `FilterSpec` entries. Each expanded filter's entry point is resolved
with the following priority order (highest wins):

1. **Map-form `at` override** from the user's filter reference (e.g.,
   `{path: acm, at: post-render}` forces ALL expanded filters to `post-render`)
2. **Extension filter's own `at`** from `ExtensionFilter.at` (e.g., the
   extension declared `{path: b.lua, at: post-render}` in its `_extension.yml`)
3. **Position-relative default** based on sentinel position (`pre-quarto` if
   before sentinel, `post-render` if after)

This matches TS Quarto (confirmed via DeepWiki): an extension filter's explicit
`at` overrides the position-relative default, regardless of where the extension
name appears relative to the `quarto` sentinel. The user can further override
all of them via the map-form `at`.

Example: Extension `acm` contributes `[{path: a.lua}, {path: b.lua, at: post-render}]`.
User writes `filters: [acm, quarto, user.lua]`. After expansion:
```
[a.lua(pre-quarto), b.lua(post-render), quarto, user.lua(post-render)]
```
- `a.lua`: no own `at`, no map override → position default (before sentinel → `pre-quarto`)
- `b.lua`: own `at: post-render` overrides position default → `post-render`
- `user.lua`: no own `at` → position default (after sentinel → `post-render`)

---

## Work Items

### Phase 2.1: Mark per-format filter paths as `!path`

Extend `mark_path_valued_keys()` in `extension/read.rs` to handle the `filters`
key. Filter entries have two forms (string and map). All entries are marked as
`Path` except reserved filter names (`"citeproc"`, `"quarto"`).

- [x] **2.1.1** Add a `FILTER_RESERVED_NAMES` constant: `&["citeproc", "quarto"]`.
  These are the only non-path strings that can appear in a filter array.

- [x] **2.1.2** Extend `mark_path_valued_keys()` to handle `"filters"` key.
  Handle it separately from `PATH_VALUED_KEYS` since its logic differs (array
  of strings and maps, with reserved name exclusion). When the key is
  `"filters"` and the value is an array:
  - **String entries** (`ConfigValueKind::Scalar(Yaml::String(s))`): if
    `s` is NOT in `FILTER_RESERVED_NAMES`, convert to
    `ConfigValueKind::Path(s)`
  - **Map entries** (`ConfigValueKind::Map`): find the `path` sub-key. If its
    value is `Scalar(Yaml::String(s))`, convert to `ConfigValueKind::Path(s)`
    (map-form `path` values in extension metadata are always file paths)

- [x] **2.1.3** Tests in `read.rs`:
  - `test_format_filter_string_marked_as_path`: `filters: [filter.lua]` →
    `ConfigValueKind::Path("filter.lua")`
  - `test_format_filter_map_path_marked`: `filters: [{path: f.lua, at: post-render}]`
    → the `path` value is `ConfigValueKind::Path("f.lua")`
  - `test_format_filter_citeproc_not_marked`: `filters: [citeproc]` → remains
    `ConfigValueKind::Scalar`
  - `test_format_filter_quarto_not_marked`: `filters: [quarto]` → remains
    `ConfigValueKind::Scalar`
  - `test_format_filter_mixed_entries`: array with file paths and reserved
    names — only file paths marked

### Phase 2.2: Name-based extension filter resolution

Modify `resolve_filters()` to accept extensions and resolve extension names.

- [x] **2.2.1** Change `resolve_filters()` signature:
  ```rust
  pub fn resolve_filters(
      meta: &ConfigValue,
      document_dir: &Path,
      extensions: &[Extension],
      runtime: &dyn SystemRuntime,
  ) -> ResolvedFilters
  ```

- [x] **2.2.2** Add `try_resolve_extension_filter()` helper:
  ```rust
  fn try_resolve_extension_filter(
      name: &str,
      extensions: &[Extension],
  ) -> Option<Vec<ExtensionFilter>>
  ```
  Uses `find_extension(name, extensions)` to look up extension by name.
  Returns `Some(ext.contributes.filters.clone())` if the extension exists
  and contributes at least one filter. Returns `None` otherwise.

- [x] **2.2.3** Add extension lookup logic in the calling loop of
  `resolve_filters()`, **before** calling `parse_filter_item()`. Keep
  `parse_filter_item()` unchanged (single-item-in, single-item-out) since
  extension lookup is logically a pre-processing step.

  For **string-form** items (where `item.as_plain_text()` returns `Some(s)`):
  1. If `s == "citeproc"`: fall through to `parse_filter_item()` (existing)
  2. File existence check: `runtime.path_exists(&document_dir.join(&s), None)`.
     If the file exists, fall through to `parse_filter_item()` (file wins).
  3. Extension lookup: call `try_resolve_extension_filter(&s, extensions)`. If
     matched, expand to one `AnnotatedFilter` per extension filter (path
     already absolute, `at` from `ExtensionFilter.at` or inherited default).
     Push all and `continue`.
  4. If not matched: fall through to `parse_filter_item()` (treats as file
     path — existing behavior).

  For **map-form** items (where `item.get("path")` returns `Some`):
  1. Extract `path` value as string.
  2. File existence check: if `document_dir.join(&path_str)` exists, fall
     through to `parse_filter_item()` (file wins).
  3. Extension lookup on the path string. If matched: expand to extension's
     filters. If the map has an `at` field, **propagate** it to ALL expanded
     filters (overriding their individual `at` values). This matches TS
     Quarto's `filters.ts:904-919`.
  4. If not matched: fall through to `parse_filter_item()` (existing).

- [x] **2.2.4** Update `UserFiltersStage::run()` to pass `&ctx.extensions`
  and `ctx.runtime.as_ref()` to `resolve_filters()`

- [x] **2.2.5** Tests in `filter_resolve.rs`:

  **Test mock strategy**: Create a `TestRuntime` struct that holds a
  `HashSet<PathBuf>` of "existing" paths. Its `path_exists()` returns
  `true` only for paths in that set. All other `SystemRuntime` methods
  can use stub implementations (return `Ok(default)`). Most tests use
  an empty set (no files exist → extension lookup always triggers).
  `test_existing_file_shadows_extension` adds a specific path to the set.

  Test cases:
  - `test_extension_name_resolves_to_filter`: `filters: [lightbox]` with
    matching extension → resolves to extension's filter path
  - `test_extension_name_multiple_filters`: extension contributing 2 filters →
    both appear in resolved output
  - `test_extension_name_with_at`: extension filter has `at: post-render` →
    entry point respected
  - `test_extension_name_with_sentinel`: extension name before/after sentinel →
    correct default entry points
  - `test_map_form_extension_reference`: `{path: lightbox}` resolves to
    extension's filters
  - `test_map_form_at_propagation`: `{path: lightbox, at: post-render}`
    overrides all extension filter entry points
  - `test_unresolved_name_falls_through`: name with no matching extension →
    falls through to file path treatment (existing behavior)
  - `test_existing_file_shadows_extension`: `"filter.lua"` exists on disk
    AND a matching extension named `filter.lua` exists → file wins
    (file-first, matching TS Quarto `filters.ts:874-880`). Uses
    `TestRuntime` with the file path in its existing-paths set.
  - `test_extension_filter_paths_absolute`: resolved extension filters have
    absolute paths (no document_dir joining)

- [x] **2.2.6** Update existing `resolve_filters` tests to pass empty
  `extensions` slice (`&[]`) and a `TestRuntime` with empty existing-paths
  set for backwards compatibility

### Phase 2.3: Integration tests

- [x] **2.3.1** Test in `user_filters.rs`: `test_pre_stage_no_extensions` —
  verify existing behavior unchanged when no extensions

- [x] **2.3.2** Test in `metadata_merge.rs`:
  `test_extension_format_filter_paths_rebased_through_merge`:
  - Setup: Extension at `/project/_extensions/acm/` contributes
    `formats.html.filters: [filter.lua]` (marked as `Path` by
    `mark_path_valued_keys`)
  - Document at `/project/posts/doc.qmd`
  - After metadata merge, `meta["filters"]` should contain a path that
    resolves to `/project/_extensions/acm/filter.lua`
  - Verify the path survives merge by checking that
    `document_dir.join(rebased_path)` canonicalizes to the extension filter

- [x] **2.3.3** Test in `metadata_merge.rs`:
  `test_mixed_extension_format_filters_and_user_filters`:
  - Setup: Extension contributes `formats.html.filters: [ext-filter.lua]`.
    Document frontmatter has `filters: [user-filter.lua]`.
  - After merge, `meta["filters"]` array should contain **both**:
    the rebased extension filter path AND the user filter path.
  - Extension filter comes first (lower priority → prepended by Concat merge).
  - User filter comes second.

- [x] **2.3.4** Test in `filter_resolve.rs`:
  `test_mixed_rebased_path_and_extension_name`:
  - Setup: `filters` array contains both a rebased extension path
    (e.g., `../_extensions/acm/filter.lua` as a Path-kind ConfigValue)
    and a bare extension name (e.g., `"lightbox"` as a Scalar).
  - The rebased path should pass the file-first check and go through
    `parse_filter_item()` (resolved against document_dir).
  - The bare name should fail the file-first check and resolve via
    extension lookup.
  - Both should appear in the output with correct absolute paths.

### Phase 2.4: Smoke tests

Lua filter execution works end-to-end: existing smoke tests at
`crates/quarto/tests/smoke-all/filters/` (`pre-filter.qmd`, `post-filter.qmd`)
exercise `pampa::unified_filter::apply_filters()` with Lua filters. The
extension filter smoke tests below follow the same pattern.

- [x] **2.4.1** Create `crates/quarto/tests/smoke-all/extensions/filter-extension/`
  smoke test: standalone filter extension with `contributes.filters: [filter.lua]`.
  Document references extension by name. Verify the filter's effect on output
  (e.g., filter adds a custom div or modifies content).

- [x] **2.4.2** Create `crates/quarto/tests/smoke-all/extensions/format-with-filters/`
  smoke test: format extension with `contributes.formats.html.filters: [filter.lua]`.
  Document uses the format (`format: myext-html`). Verify the filter runs.

- [x] **2.4.3** Create smoke test for user + extension filter concatenation:
  format extension contributes a filter, user also specifies a filter. Both
  should run.

### Phase 2.5: Workspace verification

- [x] **2.5.1** `cargo build --workspace` — clean build
- [x] **2.5.2** `cargo nextest run --workspace` — all tests pass
- [x] **2.5.3** Update master plan to mark Phase 2 complete

---

## Files to Create/Modify

| File | Action | Description |
|------|--------|-------------|
| `crates/quarto-core/src/extension/read.rs` | Modify | (2.1) Extend `mark_path_valued_keys()` to handle `filters` key (string + map forms), add `FILTER_RESERVED_NAMES` |
| `crates/quarto-core/src/filter_resolve.rs` | Modify | (2.2) Add `extensions` + `runtime` parameters to `resolve_filters()`, add `try_resolve_extension_filter()`, file-first then extension lookup in calling loop |
| `crates/quarto-core/src/stage/stages/user_filters.rs` | Modify | (2.2.4) Pass `ctx.extensions` and `ctx.runtime` to `resolve_filters()` |
| `crates/quarto/tests/smoke-all/extensions/filter-extension/` | Create | (2.4) Smoke test fixtures |
| `crates/quarto/tests/smoke-all/extensions/format-with-filters/` | Create | (2.4) Smoke test fixtures |

---

## Key APIs

**Marking filter paths as `!path`** (in `mark_path_valued_keys`):
```rust
/// Reserved filter names that should NOT be marked as Path.
const FILTER_RESERVED_NAMES: &[&str] = &["citeproc", "quarto"];

// In mark_path_valued_keys(), add handling for "filters":
// (existing template/template-partials handling unchanged)
if entry.key == "filters" {
    if let ConfigValueKind::Array(items) = &mut entry.value.value {
        for item in items.iter_mut() {
            match &mut item.value {
                // String form: mark as Path unless reserved
                ConfigValueKind::Scalar(yaml) => {
                    if let Some(s) = yaml.as_str() {
                        if !FILTER_RESERVED_NAMES.contains(&s) {
                            item.value = ConfigValueKind::Path(s.to_string());
                        }
                    }
                }
                // Map form: {path: "filter.lua", at: "post-render"}
                // Always mark the path sub-key (map-form path values
                // in extension metadata are always file paths)
                ConfigValueKind::Map(entries) => {
                    if let Some(path_entry) = entries.iter_mut()
                        .find(|e| e.key == "path")
                    {
                        if let ConfigValueKind::Scalar(yaml) = &path_entry.value.value {
                            if let Some(s) = yaml.as_str() {
                                path_entry.value.value =
                                    ConfigValueKind::Path(s.to_string());
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
}
```

**New imports needed in `filter_resolve.rs`**:
```rust
use crate::extension::Extension;
use crate::extension::discover::find_extension;
use crate::extension::types::ExtensionFilter;
use quarto_system_runtime::SystemRuntime;
```

**Extension filter name resolution** (in `filter_resolve.rs`):
```rust
fn try_resolve_extension_filter(
    name: &str,
    extensions: &[Extension],
) -> Option<Vec<ExtensionFilter>> {
    let ext = find_extension(name, extensions)?;
    if ext.contributes.filters.is_empty() {
        return None;
    }
    Some(ext.contributes.filters.clone())
}
```

**Expanding extension filters in the calling loop** (before `parse_filter_item`):
```rust
// In resolve_filters(), for each item in the loop:
// 1. Try file-first, then extension lookup for string-form items
if let Some(s) = item.as_plain_text() {
    if s != "citeproc" {
        // File-first: if the path exists on disk, treat as file (skip extension lookup).
        // Errors from path_exists are treated as "not found" — see Resolved #8.
        let file_exists = runtime
            .path_exists(&document_dir.join(&s), None)
            .unwrap_or(false);
        if !file_exists {
            if let Some(ext_filters) = try_resolve_extension_filter(&s, extensions) {
                for ef in ext_filters {
                    // to_string_lossy() returns Cow<str> which auto-derefs to &str
                    let spec = FilterSpec::parse(&ef.path.to_string_lossy());
                    // Entry point priority: extension's own `at` > position default.
                    // (No map-form `at` override in string form.)
                    let ep_idx = ef.at.as_deref()
                        .and_then(entry_point_index)
                        .unwrap_or(default_idx);
                    annotated.push(AnnotatedFilter { spec, entry_point_index: ep_idx, original_index: i });
                }
                continue;
            }
        }
    }
}
// 2. Try file-first, then extension lookup for map-form items
else if let Some(path_val) = item.get("path") {
    if let Some(path_str) = path_val.as_plain_text() {
        let file_exists = runtime
            .path_exists(&document_dir.join(&path_str), None)
            .unwrap_or(false);
        if !file_exists {
            if let Some(ext_filters) = try_resolve_extension_filter(&path_str, extensions) {
                let at_override = item.get("at").and_then(|v| v.as_plain_text());
                for ef in ext_filters {
                    let spec = FilterSpec::parse(&ef.path.to_string_lossy());
                    // Entry point priority: map `at` > extension's own `at` > position default
                    let ep_idx = at_override.as_deref()
                        .or(ef.at.as_deref())
                        .and_then(entry_point_index)
                        .unwrap_or(default_idx);
                    annotated.push(AnnotatedFilter { spec, entry_point_index: ep_idx, original_index: i });
                }
                continue;
            }
        }
    }
}
// 3. Fall through to parse_filter_item() (existing behavior)
let (spec, ep_idx) = parse_filter_item(item, default_idx);
```

---

## Risks and Open Questions

### Resolved

1. **Per-format filter merge semantics**: `MergeOp::Concat` (default) concatenates
   arrays. Extension filters come first (lower priority), user filters appended.
   No special handling needed.

2. **Path resolution approach**: Use `!path` marking in `mark_path_valued_keys()`,
   consistent with the `template`/`template-partials` approach (Phase 4).
   `adjust_paths_to_document_dir()` rebases paths from extension dir to document
   dir during metadata merge. `filter_resolve.rs` then resolves document-relative
   paths to absolute as usual.

3. **Two separate disambiguation strategies for two separate contexts**: See the
   detailed table and explanation in the "IMPORTANT: Two separate filter-path
   mechanisms" section above. Summary: `mark_path_valued_keys()` (Phase 2.1)
   handles extension `_extension.yml` format metadata — all non-reserved strings
   are file paths, no runtime checks needed. `resolve_filters()` (Phase 2.2)
   handles merged user metadata — bare names require file-existence checks and
   extension lookup at runtime. These are completely independent code paths in
   different files that never interact.

4. **Map-form filter path marking** (Phase 2.1 only, in `mark_path_valued_keys`):
   For `{path: filter.lua, at: post-render}`, only the `path` sub-key's value
   is marked as `ConfigValueKind::Path`. The `at` sub-key remains unchanged.
   `adjust_paths_to_document_dir()` walks recursively and finds the nested
   `Path` node.

5. **Map-form extension references** (Phase 2.2 only, in `resolve_filters`):
   `{path: lightbox, at: post-render}` resolves `lightbox` as an extension
   name. Confirmed in TS Quarto (`filters.ts:868-872`): both string and map
   forms resolve extension names. The map form's `at` overrides all expanded
   filters' entry points (`filters.ts:904-919`). Note: this is a completely
   separate code path from Phase 2.1's `mark_path_valued_keys()`. The
   `mark_path_valued_keys()` function always marks map `path` values as `!path`
   because extension format metadata never contains bare extension names (those
   only appear in user document metadata).

6. **Top-level vs per-format filters are independent mechanisms**: Per-format
   filters (`contributes.formats.html.filters`) flow through metadata merge
   and are automatically active when the format is used. Top-level filters
   (`contributes.filters`) are only activated when explicitly referenced by
   name (e.g., `filters: [lightbox]`). Confirmed in TS Quarto:
   `readExtensionFormat()` (`render-contexts.ts:703`) reads only
   `contributes.formats`; `resolveFilterExtension()` (`filters.ts:1099`)
   reads `contributes.filters` during name resolution. If an extension has
   both, they don't interact — per-format runs automatically, top-level only
   by name.

7. **Extension filter `at` overrides position-relative default**: Confirmed
   via TS Quarto DeepWiki. When an extension filter has an explicit `at` entry
   point (e.g., `at: post-render`), it overrides the position-relative default
   regardless of where the extension name appears relative to the `quarto`
   sentinel. Priority: map-form `at` > extension filter's own `at` > position
   default. See "Expansion of multi-filter extensions and `at` entry point
   priority" section above.

8. **`runtime.path_exists()` error handling**: Errors from `path_exists()` are
   treated as "file doesn't exist" (`.unwrap_or(false)`). This is intentional:
   a transient IO error should not block extension lookup, and the worst case
   is that extension resolution is tried unnecessarily — if the extension
   doesn't match either, the name falls through to `parse_filter_item()` which
   produces the same result as if the file existed.

---

## Codebase Reference

### `ConfigValue.get_mut()` — verified available

`ConfigValue::get_mut(&mut self, key: &str) -> Option<&mut ConfigValue>` exists
at `config_value.rs:762`. Used by `mark_path_valued_keys()` for in-place
modification. Not needed for Phase 2.1 since we already iterate `entries` via
mutable reference in the existing function.

### `as_plain_text()` and `as_str()` both work for `Path` variants

`filter_resolve.rs` uses `as_plain_text()` throughout (not `as_str()`).
Both return `Some` for `ConfigValueKind::Path` values:
- `as_str()` → `Option<&str>` (borrows); works for `Scalar(String)`, `Path`, `Glob`, `Expr`
- `as_plain_text()` → `Option<String>` (clones); works for all of the above plus `PandocInlines`

After marking filter paths as `ConfigValueKind::Path` in Phase 2.1,
`filter_resolve.rs` reads them via `as_plain_text()` without any changes.

### `FilterSpec::parse()` and path resolution

`FilterSpec::parse(s: &str)` takes a `&str` and creates `FilterSpec::Lua`
for `.lua` extension or `FilterSpec::Json` for everything else.

For extension filters, paths are stored as `PathBuf` in `ExtensionFilter.path`.
Convert via `ef.path.to_string_lossy()` which returns `Cow<'_, str>`. This
auto-derefs to `&str` for `FilterSpec::parse()`.

`resolve_filter_path()` in `filter_resolve.rs` checks `path.is_absolute()`
and skips `document_dir` joining. After `!path` adjustment, filter paths are
relative to document dir, so `resolve_filter_path()` joins them correctly.

### Path-to-string conversion convention

The q2 codebase convention is `to_string_lossy()` for converting `PathBuf`
to strings (replaces invalid Unicode with U+FFFD). `to_str().unwrap()` is
only used in contexts where paths are guaranteed UTF-8 (e.g., snapshot names).
For `FilterSpec::parse()`, use `ef.path.to_string_lossy()` rather than
`ef.path.to_str().unwrap_or("")`.

### Smoke test filter

The smoke test Lua filter needs to produce a visible effect. Simplest approach:
a filter that adds a custom class to the document body or inserts a marker div.

Example `filter.lua`:
```lua
function Pandoc(doc)
  table.insert(doc.blocks, 1, pandoc.Div(
    pandoc.Blocks{pandoc.Para{pandoc.Str "EXTENSION-FILTER-ACTIVE"}},
    pandoc.Attr("", {"ext-filter-marker"})
  ))
  return doc
end
```

The smoke test asserts `ensureHtmlElements: [["div.ext-filter-marker"]]` and
`ensureFileRegexMatches: [["EXTENSION-FILTER-ACTIVE"]]`.
