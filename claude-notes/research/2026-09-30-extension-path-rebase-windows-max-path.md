# Extension-contributed path rebase: `..` chains and Windows MAX_PATH

Research for **bd-s0jupmmv** (a `question` strand, discovered-from bd-qi11c7fj,
related bd-oejuizi9). Companion to the normative contract
`claude-notes/designs/path-resolution-model.md`.

Question the strand asks: what does Quarto 1 do for extension-contributed
paths, and for every Q2 consumer of a mechanism-2 `Path` value, does an
absolute value work there and does the join normalize?

## TL;DR

Q1 uses **absolute paths** for filters / shortcodes / reveal-plugins / engines,
but **input-relative `..` chains** (via `toInputRelativePaths`) for the format
metadata (`css`, `theme`, `include-*`, `template`, `template-partials`,
`format-resources`). Q1 has the same latent defect we do; it never surfaces
because (a) the load-bearing Lua filter paths are absolute, and (b) Deno's
`path.join` **normalizes** (collapses `..`) before any `Deno.readFile`, while
Rust's `Path::join` does not. Q2 produces `..`-leading strings from
`adjust_paths_to_document_dir` and feeds them to consumers whose joins do not
normalize, so on Windows a deep temp-dir extension blows past MAX_PATH and Lua
`io.open` (`fopen`) fails.

## 1. What Quarto 1 does (confirmed in source)

Source: `src/extension/extension.ts` and `src/project/project-shared.ts`
(fetched from `quarto-dev/quarto-cli@main`).

There are **two different mechanisms**, split by contribution kind.

### Absolute paths (safe) — resolved in `readExtension()`

These are rebased against `extensionDir` at parse time and stored absolute:

- `contributes.filters` → `resolveFilterPath()`:
  `isAbsolute(f) ? f : join(extensionDir, f)`
- `contributes.shortcodes` → `resolveShortcodePath()`: same absolute join
- `contributes.revealjs-plugins` → `resolveRevealPlugin()`: `join(extensionDir, …)`
- `contributes.engines[].path` → `join(extensionDir, engine.path)` (absolute)
- `contributes.formats.<fmt>` custom Lua writer (`key.endsWith(".lua")`) →
  `formatMeta.writer = fullPath` (absolute)

Because the executed Lua filter list comes from `contributes.filters`
(absolute), Q1's `runFilters` / pandoc `--lua-filter` never sees a `..` chain.

### Input-relative paths (the same bug we have) — `resolveExtensionPaths()`

`resolveExtensionPaths()` calls `toInputRelativePaths()`
(`project-shared.ts`) over the contributed **format metadata**. That function
computes `offset = relative(inputDir, extension.path)` and rewrites each string
to `pathWithForwardSlashes(join(offset, value))`. For an extension outside the
project (a temp-extracted built-in), `offset` is a `..` climb, so the stored
string is exactly the long `..` chain we see.

Affected keys: `css`, `theme`, `include-in-header` / `-before-body` /
`-after-body`, `template`, `template-partials`, `format-resources`.

Why it never bites Q1 on Windows:

- Deno `path.join` **normalizes** — `join(inputDir, "../../../../tmp/x/f.lua")`
  yields a clean absolute path before any `Deno.readFile`. The `..` survives
  only as a *string*; the OS open sees the collapsed form.
- The filters Q1 actually runs come from the absolute `contributes.filters`,
  not from the relative-ized `formatMeta.filters`.
- Non-Windows hosts have no MAX_PATH.

Q1 *does* still emit `..`-relative `css`/include strings, but those go to
pandoc argv / `<link href>` relative to the input, under a relocatable `_site`.

## 2. Q2 consumers of mechanism-2 `Path` values

The rebase `adjust_paths_to_document_dir` (`crates/quarto-core/src/project/mod.rs:259`)
produces `..`-leading strings whenever the declaring dir (extension dir) is
outside the consuming document dir. The sibling fragment rebase already guards
this: `rebase_candidate` (`project/mod.rs:764`) refuses a `..`-leading rebase
and keeps the absolute path. But the three per-document merge calls in
`metadata_merge.rs` (lines 163, 267, 299) call `adjust_paths_to_document_dir`
with **no such guard**.

| Consumer (file:line) | Keys | Space | `is_absolute` check? | Join normalizes? | Absolute works? |
|---|---|---|---|---|---|
| `filter_resolve.rs:269-286` (`resolve_filter_path`) | `filters` | FS | **Yes** (`path.is_absolute()`) | **No** (Rust `Path::join` keeps `..`) | **Yes** |
| `apply_template.rs:199` | `template` | FS | No (`document_dir.join`) | No | Yes, but only by accident (Rust `join` returns an absolute RHS) |
| `apply_template.rs:415` | `template-partials` | FS | No | No | same accidental yes |
| `include_resolve.rs:569` (`read_include_file`) | `include-in-header` / `-before-body` / `-after-body` | FS | No (`doc_dir.join`) | No | same accidental yes |
| `transforms/format_css.rs` (`FormatCssTransform`) | `css` | **FS and URL** | — | — | **FS yes; URL NO** — an absolute source leaks into the emitted `<link href>` |
| `quarto-sass` `ThemeContext::resolve_path` | `theme` | FS | — | — | yes |
| `pandoc_filters::format_defaults::build_forwarded_args` | `reference-doc`, `template`, `epub-*` | FS (pandoc argv) | — | — | yes — pandoc takes absolute |

The decisive split: **filesystem-space consumers tolerate absolute** (Rust
`Path::join` with an absolute argument returns it, and `filter_resolve` already
special-cases it). **URL-space consumers do not** — `css` is both copied
(filesystem) and emitted as an href (URL), and an absolute filesystem path is
not a valid page-relative href. This is the seam the design doc's #524/#455
lesson warns about: a fix scoped to one space must record the other space's
work before shipping.

## Candidate fixes (assessed against the two spaces)

- **Absolute-if-outside at the rebase** (mirror `rebase_candidate`\'s
  `..`-refusal inside `adjust_paths_to_document_dir`): fixes
  filters/template/include FS reads with no consumer change. **Breaks
  `css`/URL space** if applied uniformly — an absolute path is not a
  page-relative href. Must be space-aware, or applied only to pure-filesystem
  keys (filters, template, template-partials, include-\*, format-resources,
  reference-doc), leaving css/theme on the mechanism-3 marking.
- **Normalized joins at consumers**: collapses `..` before open. MAX_PATH
  applies to the path handed to the open call, not to the stored metadata
  string, so a consumer that passes the normalized path to Lua `io.open`
  does avoid the failure (this is why Q1 is unaffected). The drawback is
  coverage, not correctness: every filesystem consumer (filters, template,
  partials, include-\*, format-resources, reference-doc, shortcodes) would
  need it, and a new consumer that forgets regresses silently. Fixing the
  value once at the merge-time rebase covers them all.
- **Both, space-aware** (the direction consistent with the contract): FS-space
  keys get absolute-if-outside at merge time (mirroring `rebase_candidate`);
  URL-space keys (css, theme) get copy-to-`_site` + page-relative href and must
  never be made filesystem-absolute.

The natural seam is `adjust_paths_to_document_dir` in `project/mod.rs` — add
the same `..`-leading refusal as `rebase_candidate`, keyed to a per-key space
table so css/theme are excluded. That is the convergence point the design doc
already names (unified path-shaped-key registry, bd-oejuizi9 / bd-hjv5o).

## Reproducer / liveness (from the strand)

On the bd-1klbq2zd stack tip:
`SMOKE_FILTER=orange-book-margin cargo nextest run -p quarto -E 'test(smoke_all)'`
fails with `cannot open ...\../../...orange-book.lua`. Portable probe:
`adjust_paths_to_document_dir` with a `metadata_dir` outside `document_dir`\'s
tree stores a `..`-leading value.

## 3. Follow-up (2026-10-01, bd-gh3qdq7d): `theme` and `css` space at the walk

73d1fbf excluded `css`/`theme` from absolute-if-outside (`URL_SPACE_REBASE_KEYS`) on the
premise that their values reach an emitted href. Checked against code, the premise does
not hold at the point where `adjust_paths_to_document_dir` runs. Section 2's table was
right for `theme`. The `css` row conflated the stored value with the href derived later.
73d1fbf followed the plan's Phase 1 reasoning ("an absolute filesystem value is not a
valid page-relative href") without checking that the href is *derived from* the stored
value rather than *copied from* it.

### Verdict

- **`theme` is filesystem-space, end to end.** No consumer emits a theme value into
  HTML, JSON wire output, or a template variable.
- **`css` is filesystem-space at the walk.** It enters URL space only when
  `FormatCssTransform` rewrites the entry to a page-relative href computed from the
  *resolved filesystem source*. Before `0e4c834c8`, the outside-project branch left
  the stored value unchanged: the `../shared.css` href worked when served from a
  common parent. The absolute-if-outside rebase introduced a regression by making
  that branch emit an absolute filesystem path as the href. The branch now rewrites
  the entry to `diff_paths(source, page_dir)` before returning, preserving a
  page-relative href for either stored form (regression test:
  `extension_css_outside_project_links_page_relative`).
- **Outside-project CSS is still not copied into the output.** The relative href
  depends on the source file remaining accessible at that location. Shipping the
  stylesheet into `quarto-contrib/` remains bd-f0h4ahai's separate asset-copy gap.
- **Neither key hits MAX_PATH today.** The orange-book failure is Lua `io.open` (C
  `fopen`). Theme and css reads go through Rust `std::fs` or pandoc, and both handle long
  and unnormalized paths on Windows (probes below).

### Consumers of a merged `theme` value (reads and joins)

| Site | What it does with the value |
|---|---|
| `quarto-sass/src/config.rs:303` `ThemeConfig::from_config_value` | parses string / list / `{light, dark}` into `ThemeSpec::Custom(path)` |
| `quarto-sass/src/themes.rs:463` `ThemeContext::resolve_path` | `document_dir.join` then `normalize_lexically`. Every read below goes through it |
| `quarto-sass/src/themes.rs:573` `load_custom_theme` | `path_exists` + `file_read_string` on the resolved path (NativeRuntime = `std::fs`) |
| `compile_theme_css.rs:464` `cache_key` | hashes the *resolved, normalized* path + file bytes |
| `compile_theme_css.rs:1001` existence pre-check (Q-14-4) | resolved path |
| `compile_theme_css.rs:1129` `attach_entry_location` | compares resolved paths |
| `revealjs/theme.rs:45,84` | `as_plain_text` entries → `load_custom_theme` |
| `format.rs:1238` `is_minimal_html` | compares to `none` / `pandoc` only |
| `template.rs:886`, `navbar_generate.rs:97` | `ThemeConfig::from_config_value(...).dark` presence only |

Compiled CSS is stored as an artifact (`store_css` / `store_variant_pair`). Its href comes
from the artifact, not from the theme value. Templates have no `$theme$` variable
(grep of `resources/`).

### Which theme forms reach the walk as `Path`

`FORMAT_ASSET_PATTERNS` (`extension/paths.rs:59`) has `["theme"]`. Once that pattern is
exhausted, `walk_pattern_leaves` applies `apply_to_string_leaves` (`paths.rs:118-140`),
which marks **every** string leaf under `theme`: a plain string, list items, and the
`light` / `dark` children (string or list). `ThemeConfig` accepts nothing deeper. The pair
form is top-level only (`config.rs:310`). `adjust_paths_recursive` passes the
*immediate* map key down (`Some(&entry.key)`), so under the pair form the key is
`light` / `dark` and the classification is lost (roborev 2987 finding 2). The doc
comment says "top-level key". The implementation does not match it.

### Probes (Windows 11, 2026-10-01)

1. `project::tests::theme_rebase_outside_project_loads_from_deep_document_dir`. A real
   temp-dir theme consumed from a 40-level document dir. Top-level `theme` →
   `../../…(48×)/AppData/Local/Temp/quarto-theme-probe-*/probe.scss`. `{light:}` →
   `C:/Users/chris/AppData/Local/Temp/…/probe.scss`. Unnormalized `document_dir.join` =
   **424 chars**. `load_custom_theme` succeeds for both, and `resolve_path` gives the
   same path for both (same cache key and load path). **No RED for a theme MAX_PATH
   failure exists**: the defect is the inconsistency, not a failure.
2. pandoc 3.12 `--css=<428-char unnormalized '..' path>` (the epub consumer,
   `pandoc_write.rs:210`, `resolve_doc_relative` = unnormalized join): exit 0, and the
   epub contains the stylesheet. pandoc handles long paths.

### Reachability

No built-in extension contributes `css`, `theme` or `include-*`. orange-book only has
`template-partials`, and julia-engine has none (grep of `resources/extension*/**/_extension.yml`).
User extensions are discovered only between the input dir and the project root
(`extension/discover.rs:50-66`), so the extension *dir* is always inside the tree. The
*asset* need not be: `bundled_file_exists` accepts any existing file, so
`<project>/_extensions/acme` declaring `css: ../../../shared.css` reaches this walk with
a value outside the project. That case is reachable, and it used to emit a working
`../shared.css` href (see the plan's Migration section for how it is kept working).

### Precedent already contradicts the exclusion

`rebase_candidate` (`project/mod.rs:~890-904`) stores **absolute** paths for
`contributes.project` fragment values outside the project, *including*
`format.*.theme` and `format.*.css` (`FRAGMENT_PATH_PATTERNS`, `mod.rs:826-828`). Those
values are rooted, so `adjust_paths_recursive` skips them (`mod.rs:320`). Absolute
css/theme from a temp-extracted extension therefore already flow to `FormatCssTransform`
and `ThemeContext` today. 73d1fbf's two mechanisms disagree on the same keys.

### Roborev 2987 finding 1 (lexical `starts_with`)

Confirmed real and **pre-existing**. The `else` branch is the pre-73d1fbf code
unchanged. Example: `metadata_dir = /project/sub`, value `../../tmp/x.lua` →
`abs_path = /project/sub/../../tmp/x.lua`, which lexically `starts_with("/project")`,
so it takes the `diff_paths` branch and yields `../sub/../../tmp/x.lua` (probe run
2026-10-01, Windows). The
chain is bounded by what the author wrote plus the document depth. It never climbs to an
unrelated temp tree, so it is not the bd-qi11c7fj MAX_PATH class. Built-in extension
dirs are clean absolute temp paths, so the orange-book case never takes this route.
Harmless for the stack. It costs one `normalize_lexically` call to make the boundary
check honest.
