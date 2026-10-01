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

- **Absolute-if-outside at the rebase** (mirror `rebase_candidate`'s
  `..`-refusal inside `adjust_paths_to_document_dir`): fixes
  filters/template/include FS reads with no consumer change. **Breaks
  `css`/URL space** if applied uniformly — an absolute path is not a
  page-relative href. Must be space-aware, or applied only to pure-filesystem
  keys (filters, template, template-partials, include-*, format-resources,
  reference-doc), leaving css/theme on the mechanism-3 marking.
- **Normalized joins at consumers**: collapses `..` before open, but does not
  shorten the *string*, so it does **not** help Windows MAX_PATH on the stored
  value — only on what the OS sees. Insufficient alone for the reported bug.
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
`adjust_paths_to_document_dir` with a `metadata_dir` outside `document_dir`'s
tree stores a `..`-leading value.
