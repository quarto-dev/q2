# Extension subtrees: adding a real vendored subtree

Q2's extension-subtree infrastructure (design + build notes:
`claude-notes/plans/2026-09-23-extension-subtree-infrastructure.md`) lets a
whole extension repo be vendored as a `git subtree` under
`resources/extension-subtrees/<name>/`, kept in sync via `cargo xtask
pull-extension-subtree`, and discovered as a builtin root alongside the
regular `resources/extensions/` bundle — mirroring Quarto 1's
`src/resources/extension-subtrees/` + hidden `pull-git-subtree` dev command.

Two real subtrees are registered: **orange-book** (book-projects P2 item
80, the default Typst book extension, subtreed from `quarto-ext/orange-book`)
and **julia-engine**, subtreed from the `q2-static-declarations` branch of
`gordonwoodhull/quarto-julia-engine` (the parent epic's Step 4,
`claude-notes/plans/2026-09-03-julia-engine-static-declarations-epic.md`).
The `synth-echo` fake (`crates/quarto-core/tests/fixtures/extension-subtrees/synth-echo/`,
`crates/quarto-core/tests/integration/synth_extension_subtree_e2e.rs`)
remains the hermetic test fixture.

## Adding a subtree: the three pieces

1. **One `subtrees()` row.** Add a `SubtreeConfig` entry to `subtrees()` in
   `crates/xtask/src/pull_extension_subtree.rs` (a fn, not a const —
   `SubtreeConfig` owns `String`s, which can't be built non-empty in a
   const context):

   ```rust
   pub fn subtrees() -> Vec<SubtreeConfig> {
       vec![
           SubtreeConfig {
               name: "orange-book".to_string(),
               prefix: "resources/extension-subtrees/orange-book".to_string(),
               remote_url: "https://github.com/quarto-ext/orange-book.git".to_string(),
               remote_branch: "main".to_string(),
           },
           SubtreeConfig {
               name: "julia-engine".to_string(),
               prefix: "resources/extension-subtrees/julia-engine".to_string(),
               remote_url: "https://github.com/gordonwoodhull/quarto-julia-engine.git".to_string(),
               remote_branch: "q2-static-declarations".to_string(),
           },
           // ... new rows here
       ]
   }
   ```

2. **Pull it in.** Run `cargo xtask pull-extension-subtree julia-engine`
   (or omit the name / pass `all` to pull every configured row). First run
   performs `git subtree add --squash`; subsequent runs no-op or
   `git subtree pull --squash` depending on whether upstream has new
   commits — see the command's `--help` and
   `crates/xtask/src/pull_extension_subtree.rs`\'s module doc for the exact
   `git-subtree-dir`/`git-subtree-split` trailer mechanics.

   **Running from a linked worktree (`.worktrees/<name>/`): set
   `QUARTO_SUBTREE_ROOT`.** `subtree_root()`'s default falls back to
   `create_worktree::repo_root()`, which resolves to the *main* checkout
   (`git rev-parse --git-common-dir`'s parent) — not the worktree the
   command is invoked from. Without the override, `git subtree add`
   silently targets the main checkout's working tree and fails with
   `fatal: working tree has modifications.  Cannot add.` whenever the main
   checkout happens to be dirty (unrelated to the worktree you're actually
   working in — found vendoring `orange-book` from a worktree, item 80 of
   `claude-notes/plans/2026-09-21-book-projects-P2-single-file-merge.md`).
   Run instead:

   ```bash
   QUARTO_SUBTREE_ROOT="$(pwd)" cargo xtask pull-extension-subtree <name>
   ```

   from inside the worktree.

3. **One `include_dir!` payload static + registration.** The vendored repo
   at `resources/extension-subtrees/<name>/` typically contains far more
   than the extension itself (source, tests, CI config — a real repo
   checkout). **Only embed that subtree's own `_extensions/` payload**, not
   the whole vendored tree (F8 / design decision D1) — embedding whole
   repos would put unrelated bytes (tests, CI config, potentially many MB)
   into the shipped binary. In `crates/quarto-core/src/extension/mod.rs`\'s
   native `builtin` module:

   ```rust
   static JULIA_ENGINE_SUBTREE_DIR: Dir = include_dir!(
       "$CARGO_MANIFEST_DIR/../../resources/extension-subtrees/julia-engine/_extensions"
   );
   pub static JULIA_ENGINE_SUBTREE: ResourceBundle =
       ResourceBundle::new("julia-engine-subtree", &JULIA_ENGINE_SUBTREE_DIR);
   ```

   then add it to `EXTENSION_SUBTREE_PAYLOADS`:

   ```rust
   pub static EXTENSION_SUBTREE_PAYLOADS: &[&ResourceBundle] =
       &[&ORANGE_BOOK_SUBTREE, &JULIA_ENGINE_SUBTREE];
   ```

   and add the subtree's name to `EXTENSION_SUBTREE_NAMES` in the same file
   (the shared list the WASM side keys off).

   `builtin_extension_subtree_roots` picks up every registered payload
   automatically — no other native code changes needed. **On WASM you must
   also add a matching per-subtree embed** in `populate_extension_subtrees`
   in `crates/wasm-quarto-hub-client/src/lib.rs`:

   ```rust
   static JULIA_ENGINE_SUBTREE_DIR: Dir = include_dir!(
       "$CARGO_MANIFEST_DIR/../../resources/extension-subtrees/julia-engine/_extensions"
   );
   // prefix: {RESOURCE_PATH_PREFIX}/extension-subtrees/julia-engine/_extensions
   ```

   The WASM side embeds **per-subtree `_extensions/` dirs**, exactly like
   native — never the whole `resources/extension-subtrees/` tree (that would
   put the vendored repos\' tests/CI config into the WASM blob, and the root
   shape would be wrong: `discover_extensions` scans each root's *children*
   as extensions, so a root must be a `_extensions/` dir, not the parent
   `extension-subtrees/` dir).

## Dev/test seam: `QUARTO_EXTENSION_SUBTREES_DIR`

Set this env var to a single directory shaped like a builtin root (i.e.
directly containing extension directories, the same shape as
`resources/extensions/` or a project's `_extensions/`) to override
`builtin_extension_subtree_roots` entirely — used by
`synth_extension_subtree_e2e.rs` to point at the `synth-echo` fixture
without installing anything under a project's `_extensions/`. Native-only;
checked before the embedded `ResourceBundle` list, never read on WASM.
Not for production use.

## Why the fake extension lives outside `resources/`

`crates/quarto-core/tests/fixtures/extension-subtrees/synth-echo/` — never
`resources/extension-subtrees/` — so it never ships in a release binary (D3).
The `README.md` in `resources/extension-subtrees/` keeps that directory in
git (git does not track empty directories) for the `include_dir!` call
sites to compile against even before any real subtree lands.
