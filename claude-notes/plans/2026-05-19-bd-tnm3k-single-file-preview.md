# bd-tnm3k — Fix `q2 preview` for single-file mode without `_quarto.yml`

## Problem (verbatim from the beads issue)

When `q2 preview` is invoked on a single `.qmd` file with no
`_quarto.yml` ancestor, `crates/quarto/src/commands/preview.rs:270-276`
sets `project_root` to the file path itself (a file, not a directory).
Downstream:

1. `ProjectFiles::discover()` walks via `WalkDir` and yields exactly one
   entry — the file itself.
2. `path.strip_prefix(project_root)` returns an empty `PathBuf` (same
   path against itself), which is pushed into `qmd_files`.
3. `reconcile_files_with_index` does `project_root.join(file_path)`
   with an empty `file_path`, which produces a *trailing-slash variant
   of the file path*.
4. `std::fs::read_to_string` of that returns ENOTDIR (os error 20).

Symptom: the warning `Failed to read text file, skipping path= error=Not
a directory (os error 20)` (empty `path=`) fires every iteration. The
SPA stays stuck at "Initializing q2-preview…" forever.

## Chosen direction (option b — confirmed with user)

`project_root` becomes the parent directory of the file. **Discovery
and the file watcher are constrained to that one file** — so we do not
accidentally start indexing or watching the entire parent directory
(e.g. all of `~/Downloads`).

Option (a) — naive use of the parent directory — was rejected for the
`~/Downloads` footgun. Option (c) — tempdir + symlink — was rejected
because edits would not flow back to the original file.

## TDD plan

### Phase 1 — failing tests

- [x] `crates/quarto-hub/src/discovery.rs`:
      `single_file_constructor_yields_one_qmd_with_nonempty_path`.
- [x] `crates/quarto-hub/src/watch.rs`:
      `test_watcher_single_file_ignores_sibling_qmd`.
- [x] `crates/quarto/src/commands/preview.rs`: rewrote
      `resolve_file_without_quarto_yml_keeps_single_file_mode` as
      `resolve_file_without_quarto_yml_resolves_parent_as_root`
      (asserts new behaviour); other resolver tests updated to the
      `ResolvedProject` struct return shape.

Pre-implementation `cargo nextest` confirmed they failed to even
compile (E0599 / E0560 on the missing constructor + field) — that
is the red phase.

### Phase 2 — implementation

- [x] `ProjectFiles::single_file(rel)` — no walk, just stuffs `rel`
      into `qmd_files`.
- [x] `HubConfig.single_file: Option<PathBuf>`; `HubContext::new`
      branches on it.
- [x] `WatchConfig.single_file: Option<PathBuf>`; `FileWatcher::new`
      subscribes to the file in `NonRecursive` mode *and* filters
      events to that exact path (belt + suspenders for cross-platform
      `notify` quirks).
- [x] `PreviewConfig.single_file: Option<PathBuf>`; `build_hub_config`
      forwards it. Test fixtures (`boot.rs`, `staleness.rs`,
      `eager_capture.rs`) updated with `single_file: None`.
- [x] `resolve_project_and_initial_page` returns a `ResolvedProject`
      struct (root, initial_page, single_file); the no-ancestor
      branch sets `root = parent`, `initial_page = Some(filename)`,
      `single_file = Some(filename)`.

### Phase 3 — verification

- [x] All new tests pass.
- [x] `cargo nextest run --workspace`: 9134 passed, 196 skipped.
- [x] `cargo xtask verify --skip-hub-build`: clean.
- [x] Manual e2e against `cargo run --bin q2 -- preview ...` on a
      `.qmd` with no `_quarto.yml` ancestor — see § End-to-end
      verification below.

## End-to-end verification

Invocation (run from a `.tmp-bd-tnm3k/` directory placed *inside* the
repo so I could clean up easily; the path has no `_quarto.yml`
ancestor under it):

```
cargo run --bin q2 -- preview --no-browser --port 0 ./.tmp-bd-tnm3k/file.qmd
```

Observed boot log (key lines):

```
  q2 preview
  → http://127.0.0.1:62266/?page=file.qmd

INFO quarto_hub::storage:  Storage manager initialized (project mode)
     project_root=.../q2/.tmp-bd-tnm3k hub_dir=...
INFO quarto_hub::context:  Discovered project files
     qmd_count=1 config_count=0 binary_count=0 ...
INFO quarto_hub::context:  Reconciled new files with index count=1
INFO quarto_hub::context:  Initial filesystem sync complete synced=1 errors=0
INFO quarto_hub::watch:    Started filesystem watcher
     path=.../q2/.tmp-bd-tnm3k debounce_ms=500 filter=PreviewBroad
     single_file=Some(".../q2/.tmp-bd-tnm3k/file.qmd")
```

`GET /health` on a fresh instance:

```
{"status":"ok",
 "project_root":".../q2/.tmp-bd-tnm3k",
 "qmd_file_count":1,
 "index_document_id":"pUS7U8aqVzAdk8163vTeLWiaYVe"}
```

Searched the boot log for the pre-fix symptom:

```
grep -cE 'ENOTDIR|Not a directory|Failed to read text file|skipping path=' ...
0
```

— zero matches. The bug is fixed: project_root is the file's parent
directory, discovery indexes exactly one `.qmd`, and the watcher is
constrained to that one file rather than scanning the parent dir.

## Out of scope

- Loading multiple files when the user gives a *directory* without
  `_quarto.yml` — already works via the `metadata.is_dir()` branch.
- Special-casing single-file mode for the SPA's UI (sidebar etc.).
  bd-1tl09 epic features may want different behaviour but that's a
  separate decision.
