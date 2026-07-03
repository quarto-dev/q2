# Embedded-resource virtual-path contract

**Status:** Design (2026-07-03). Fix tracked by strand `bd-nxxgg0pp`
(discovered from `bd-3fgnmlco`, blocks the `bd-22rtwdur` smoke-all umbrella).

Quarto compiles Bootstrap/theme SCSS from resources embedded at build time
via `include_dir!`. The `@import`/`@use` statements *inside* that embedded
SCSS (e.g. Bootstrap's own `@import "vendor/rfs"`) are resolved at compile
time against a **virtual** load path rooted at `RESOURCE_PATH_PREFIX =
"/__quarto_resources__"`. This document defines the contract that virtual
namespace must obey so resolution behaves identically on every OS and on
both compile backends (native grass, WASM dart-sass).

## Two path domains

Two distinct kinds of path flow through the SASS layer, both currently
typed as `std::path::PathBuf`/`&Path`:

- **(a) Real OS paths** — user project documents, custom theme directories
  (`ThemeContext` load paths). `Path::join` and native separators are
  *correct* here. This domain is not changed by this work.
- **(b) Virtual embedded-resource namespace** — `/__quarto_resources__/…`.
  Must be **forward-slash on every platform**. This is a URL-like opaque
  string domain, not a filesystem path.

The bug (below) is that domain (b) is forced through `std::path` machinery
that is correct only for domain (a).

## Root cause (Windows) — source-anchored

On a Windows build, every embedded `@import` fails to resolve, theme/
bootstrap/highlight compilation errors, and the render silently falls back
to base CSS (`compile_theme_css.rs:547`). Chain:

1. `default_load_paths()` (`quarto-sass/src/resources.rs:305-310`) returns
   virtual paths built with `format!` — forward-slash, e.g.
   `/__quarto_resources__/bootstrap/scss`.
2. grass resolves `@import "vendor/rfs"` in `Visitor::find_import`
   (`grass_compiler 0.13.4`, `crates/compiler/src/evaluate/visitor.rs:858-859`):
   `let path_buf = load_path.join(path);`. `PathBuf::join` is target-OS
   aware and inserts `\` on Windows → `/__quarto_resources__/bootstrap/scss\vendor/rfs`.
   grass's own `options.rs` documents this limitation ("still using
   std::path… constrains to the target platform").
3. grass calls `self.options.fs.is_file(&path_buf)` on that raw `\`-joined
   path (via the `try_path!` macro, visitor.rs:819/823). `Fs::canonicalize`
   is **not** a pre-lookup hook — `import_like_node` (visitor.rs:891) calls
   it only *after* `find_import` already matched, for the `import_cache`/
   `files_seen` key and `fs.read`. So the `Fs` impl must normalize itself.
4. Our `RuntimeFs` (`quarto-system-runtime/src/sass_native.rs:84-119`)
   checks embedded first → `EmbeddedResources::is_file` →
   `strip_prefix` (`quarto-sass/src/resources.rs:167-184`), which is pure
   `&str` surgery that only ever trims `/`. It yields relative key
   `\vendor/rfs.scss`.
5. Embedded file keys are forward-slash — `include_dir_macros 0.7.4`
   normalizes `\`→`/` at compile time (`src/lib.rs:134-136`). So
   `\vendor/rfs.scss` never matches `vendor/rfs.scss` → miss.

Empirically verified: `PathBuf::from("/__quarto_resources__/bootstrap/scss")
.join("vendor/_rfs.scss").to_string_lossy()` = `…/scss\vendor/_rfs.scss` on
Windows; `str::lines`-style stripping leaves the leading `\`. Real OS load
paths (custom themes, reveal SCSS) are immune — `RuntimeFs`'s `std::fs`
fallback is separator-tolerant on Windows, and reveal SCSS is fully inlined
(no `find_import`).

## Native vs WASM contract

| Aspect | Native (grass) | WASM (dart-sass via JS) |
|---|---|---|
| Prefix | `RESOURCE_PATH_PREFIX` | same constant, re-used |
| Separator | forward-slash by construction; **breaks** when grass's `Path::join` injects `\` | pure forward-slash JS strings; `vfs:` URL importer never touches `path` |
| Lookup | `EmbeddedResources::strip_prefix` accepts 3 path shapes, normalizes to a relative key | JS importer builds full keys itself (`loadPath + "/" + url`), flat exact-match VFS |
| Correctness on Windows | **broken** (this bug) | correct (no `\` ever introduced) |

WASM already matches the dart-sass best-practice importer contract
(custom `vfs:` scheme, `canonicalize`→`load`, pure string) in
`ts-packages/wasm-js-bridge/src/sass.js`. The native side is the one that
diverges from the shared forward-slash contract.

**Known WASM divergences (deferred — not this bug):**
- WASM `populate_vfs_with_embedded_resources`
  (`wasm-quarto-hub-client/src/lib.rs:64-76`) materializes **only the
  Bootstrap pool** into the VFS; native exposes all five pools via
  `CombinedResources`. A future cross-pool `@import`-by-path would resolve
  native but fail WASM. Currently masked (no such import shipped).
- The `sass-utils` load path is a dead no-op on WASM (nothing stored there).

Tracked separately (see Deferred work).

## Fix

**Reuse the existing shared helper — do not introduce a new function.**
`quarto-util` already owns the canonicalization boundary:

```rust
// crates/quarto-util/src/path.rs:23
pub fn to_forward_slashes(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
```

This is byte-identical to what an earlier draft proposed as a new
`to_virtual_key`, and it is already unit-tested (unix no-op + Windows
conversion). It is leading-slash-preserving and idempotent — exactly what
`strip_prefix` needs, since the very next step, `.strip_prefix(RESOURCE_PATH_PREFIX)`,
depends on the leading `/`. (An earlier draft's extra
`split('/').filter(!empty).join("/")` would have *dropped* that leading
slash and broken the prefix strip; grass's `load_path.join(path)` never
produces `//` anyway, so only the `\`→`/` replacement is needed.)

There are already two private copies of this logic
(`quarto-core/src/stage/stages/document_profile.rs:132` `to_forward_slash`,
`quarto-core/src/project/discovery.rs:181` `to_forward_slashes`), which is
exactly the proliferation to avoid — consolidate on the `quarto-util` one.

- **Add `quarto-util` as a dependency of `quarto-sass`** (it is not one
  today). `quarto-util::path` is pure `std` and already reasons about
  `wasm32` behavior (`is_rooted`), so it is safe in `quarto-sass`'s WASM
  build. Verify the WASM build during implementation.
- **Placement — `EmbeddedResources` only.** Call `to_forward_slashes` at
  the top of `strip_prefix` (which every `is_file`/`is_dir`/`read`/`read_str`
  funnels through) and in `collect_files`/`collect_directories` for key
  symmetry. This is the embedded-namespace boundary — the same "normalize
  once at the boundary" idiom `include_dir` and `rust-embed` use.
- **`RuntimeFs` stays untouched.** Its `read`/`is_file` fall back to real
  `std::fs` for user files; normalizing `\`→`/` there would mangle genuine
  Windows paths (`C:\Users\…`). Normalization is strictly embedded-only.
  (This corrects a research suggestion to also normalize in `RuntimeFs`.)
- **Shared helper, not a newtype (for now).** grass's `Fs` and
  `SystemRuntime` are hard-typed to `std::path`; a `VirtualPath` newtype
  would ripple through both for a currently-single namespace. Promote to a
  newtype (rust-analyzer `VfsPath` style) only if the virtual surface grows.
- **Document the two domains** at the top of `resources.rs` so future
  embedded providers route through `to_forward_slashes` instead of growing
  their own string-trimming.

## Test plan (TDD)

1. **RED (in-source unit test, runs on Linux CI):** feed a backslash-joined
   virtual path (as grass produces on Windows) to `EmbeddedResources::is_file`
   / `read` and assert it resolves. Build the `\` path inline so the test
   exercises the corruption on any host. Confirm it fails before the fix.
2. **GREEN:** add `to_virtual_key`, wire into `strip_prefix` + collectors.
3. **Regression:** full `cargo nextest run -p quarto-sass` — the 30
   `@import "vendor/rfs"` failures (uncovered once the parse_layer CRLF bug
   `bd-3fgnmlco` was fixed) must go to zero.

## Deferred work (own strands)

- **Unify embedded pools into the WASM VFS** / drop the dead `sass-utils`
  load path, so native and WASM resolve the same cross-pool imports.
- **`windows-latest` CI job** running `compile_scss_with_embedded` against
  Bootstrap — CI is Unix-only today, so this bug cannot regress-guard on CI.
  The in-source backslash test covers the logic; real-Windows E2E coverage
  needs the job.

## Decisions (2026-07-03, with Chris)

- Abstraction: reuse the existing `quarto_util::to_forward_slashes` (add
  `quarto-util` dep to `quarto-sass`); no new function; newtype deferred.
  Consolidate the two private dupes onto it opportunistically.
- Placement: `EmbeddedResources` boundary only; `RuntimeFs` untouched.
- WASM pool divergence: documented + deferred to a strand.
- Windows CI job: deferred to a strand.
