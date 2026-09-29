# Extension subtrees: adding a real vendored subtree

Q2's extension-subtree infrastructure (design + build notes:
`claude-notes/plans/2026-09-23-extension-subtree-infrastructure.md`) lets a
whole extension repo be vendored as a `git subtree` under
`resources/extension-subtrees/<name>/`, kept in sync via `cargo xtask
pull-extension-subtree`, and discovered as a builtin root alongside the
regular `resources/extensions/` bundle — mirroring Quarto 1's
`src/resources/extension-subtrees/` + hidden `pull-git-subtree` dev command.

As of this writing **no real subtree is registered** — the infrastructure
was built and proven against a fake extension (`synth-echo`, see
`crates/quarto-core/tests/fixtures/extension-subtrees/synth-echo/` and
`crates/quarto-core/tests/integration/synth_extension_subtree_e2e.rs`). The
first real row (the julia engine) is the parent epic's Step 4
(`claude-notes/plans/2026-09-03-julia-engine-static-declarations-epic.md`).

## Adding a subtree: the three pieces

1. **One `SUBTREES` row.** Add a `SubtreeConfig` entry to `SUBTREES` in
   `crates/xtask/src/pull_extension_subtree.rs`:

   ```rust
   pub const SUBTREES: &[SubtreeConfig] = &[
       SubtreeConfig {
           name: "julia-engine",
           prefix: "resources/extension-subtrees/julia-engine",
           remote_url: "https://github.com/PumasAI/quarto-julia-engine.git",
           remote_branch: "main",
       },
   ];
   ```

2. **Pull it in.** Run `cargo xtask pull-extension-subtree julia-engine`
   (or omit the name / pass `all` to pull every configured row). First run
   performs `git subtree add --squash`; subsequent runs no-op or
   `git subtree pull --squash` depending on whether upstream has new
   commits — see the command's `--help` and
   `crates/xtask/src/pull_extension_subtree.rs`'s module doc for the exact
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
   into the shipped binary. In `crates/quarto-core/src/extension/mod.rs`'s
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
       &[&JULIA_ENGINE_SUBTREE];
   ```

   `builtin_extension_subtree_roots` picks up every registered payload
   automatically — no other code changes needed. On WASM,
   `populate_extension_subtrees` in `crates/wasm-quarto-hub-client/src/lib.rs`
   already embeds the **whole** `resources/extension-subtrees/` tree (see
   that function's doc comment for why the WASM side doesn't need the same
   per-subtree splitting the native side does — mirroring
   `populate_builtin_extensions`'s existing whole-dir embed for
   `resources/extensions/`); no change needed there either unless the
   binary-size tradeoff is revisited (tracked by the epic's Step 4, not
   this doc).

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
`resources/extension-subtrees/README.md` is the only thing committed there
until a real subtree lands; it exists purely so the directory exists in git
(git does not track empty directories) for the two `include_dir!` call
sites (native `builtin` module here, and
`populate_extension_subtrees`/`populate_builtin_extensions` in
`wasm-quarto-hub-client`) to compile against.
