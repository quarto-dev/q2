---
title: 'Highlight grammar coverage survey: Pandoc parity and candidate crates'
date: 2026-11-17
description: 'Which of the syntaxes Pandoc highlights q2 is missing, which tree-sitter grammar crates could fill the gaps, and what each costs in the WASM bundle. Decision: add only ini, toml (plus conf), xml and dockerfile now, with powershell borderline; defer the rest until the WASM build can load grammars on demand.'
---

## Summary

The Posit Connect docs build reported code blocks that Quarto 1 highlights
and q2 does not. Connect works around part of it by shipping tree-sitter
grammars for `ini`, `toml`, `dockerfile` and `xml` in its own
`_quarto/grammars/` directory, so those four highlight in the Connect build
only. The languages it still lacks are `apache`, `conf` and `powershell`.

This note records a survey of every syntax Pandoc 3.10 knows (the table in
`crates/quarto-highlight/src/pandoc_languages_table.rs`) against grammar
crates on crates.io, run on 2026-11-17.

**Decision (2026-11-17).** Bundle as built-ins only:

- `ini`
- `toml`, with `conf` as an alias
- `xml`
- `dockerfile`, using the `tree-sitter-containerfile` crate

`powershell` is the borderline case (about 1.1 MB). It is acceptable but
not required.

All other missing languages wait until the WASM distribution can fetch a
grammar from a separate location on demand. A built-in grammar adds its
weight to the hub-client WASM, so every user downloads it, and most of these
languages are rare in Quarto documents. A broad set would double or triple
the bundle, which is about 42 MB today. The prior art for runtime loading
is in `claude-notes/research/syntax-highlighting-dynamic-grammars.md` and
`claude-notes/plans/2026-05-10-thread-user-grammars-renderer.md`.

## Method

1. **Metadata.** For each candidate, the crates.io API gave the newest
   version, recent downloads, license, repository, and whether the crate
   depends on `tree-sitter-language` (current) or on a pinned `tree-sitter`
   (legacy, unusable with our 0.26).
2. **Native probe.** A throwaway binary depended on every candidate along
   with `tree-sitter` and `tree-sitter-highlight` at `=0.26.13`, the
   versions in our lockfile. For each grammar it built a
   `HighlightConfiguration` from the highlight query in the published crate
   and highlighted a short sample. It recorded the grammar ABI, the number
   of captures, the number of spans, and the capture names that fired.
3. **wasm32 C compile.** The same dependency set was compiled for
   `wasm32-unknown-unknown`. It used Homebrew clang,
   `-isystem crates/wasm-quarto-hub-client/wasm-sysroot` (as
   `hub-client/scripts/build-wasm.js` does), and our
   `tree-sitter-language-wasm-shim` patch. This shows only that the C
   compiles against our stub headers. **The final wasm link was not
   tested**, so a scanner that calls a libc function the shim does not
   export would still fail at link time.
4. **Size.** The size of each wasm32 static library (`.a`) was used as a
   proxy. It overstates the bytes that end up in the final optimized WASM,
   but it ranks the grammars reliably.

## Coverage today

q2 highlights 14 of Pandoc's syntaxes: bash, css, html, javascript (plus
jsx), json, julia, lua, markdown (through the qmd composite), python, r,
rust, sql, typescript and yaml. tsx and lmd are q2 additions. Some of
Pandoc's entries are internal helpers in skylighting rather than languages:
`alert`, `comments`, `modelines`, `doxygen`, `doxygenlua`, `spdxcomments`
and `default`.

## The adopted set

| Class | Crate | Probe | wasm32 C | Size | Notes |
|---|---|---|---|---|---|
| `ini` | `tree-sitter-ini` 1.4.0 (Apache-2.0) | OK, ABI 15 | OK | 8 KB | No external scanner. Captures: `type`, `property`, `operator`, `punctuation.bracket`. |
| `toml` | `tree-sitter-toml-ng` 0.7.0 (MIT) | OK, ABI 14 | OK | 36 KB | The older `tree-sitter-toml` 0.20.0 depends on tree-sitter 0.20 and cannot be used. |
| `xml` | `tree-sitter-xml` 0.7.0 (MIT) | OK, ABI 14 | OK | 108 KB | The language constant is `LANGUAGE_XML`, and the query is in `queries/xml/highlights.scm`. The crate also exposes `LANGUAGE_DTD`. Check the `markup` capture against the theme mapping. |
| `dockerfile` | `tree-sitter-containerfile` 0.9.2 (MIT) | OK, ABI 15 | **fails** | small | Needs `memchr` declared in the sysroot `string.h` and `iswxdigit` in `wctype.h`. `tree-sitter-dockerfile` 0.2.0 depends on tree-sitter 0.20. containerfile is a maintained fork with about 320k recent downloads. |
| `powershell` | `tree-sitter-powershell` 0.26.6 (MIT) | OK, ABI 15 | OK | 1.1 MB | Borderline on size. Its query uses some nonstandard capture names (for example `assignvalue`) that need auditing against `capture_token`. |

### `conf`

Pandoc has no `conf` syntax. skylighting resolves the class through file
globs, and TOML lists `*.conf`, so Quarto 1 highlights a `conf` block as
TOML. Aliasing `conf` to `toml` reproduces that. A real-world `.conf` file
often isn't valid TOML, so tree-sitter will produce error nodes and
highlight only part of the file. Kate's regex highlighter degrades more
gracefully. If the toml grammar looks bad on typical `conf` content, `ini`
is the more forgiving alternative.

### `apache`

No tree-sitter grammar for Apache configuration exists on crates.io. The
options are to write one, to alias it to `ini` as an approximation, or to
leave it unhighlighted (it still gets the `div.sourceCode` wrapper through
`is_pandoc_known_language`).

## Constraints found along the way

1. **Gaps in the wasm32 sysroot and shim.** Grammars that need the
   following fail to compile (or will fail to link) for the hub-client
   build:
   - declarations missing from the stub headers: `iswxdigit`, `iswblank`,
     `iswupper`, `iswlower` and `towupper` (`wctype.h`), `memchr` and
     `strncat` (`string.h`);
   - a missing `wchar.h` (php);
   - `crates/tree-sitter-language-wasm-shim/wasm/include/wctype.h` uses
     `bool`, `true` and `false` without including `stdbool.h`, which breaks
     any scanner that includes it before `stdbool.h` (cmake).

   Every new declaration also needs a matching implementation in
   `crates/wasm-c-shim/src/shim.rs`.
2. **The shim pins `tree-sitter-language` at 0.1.7.** Crates that require
   `^0.1.8` or newer (graphql 0.3.0, kdl 2.0.0, mermaid 0.3.4) silently pull
   in the real crate and bypass our patch.
3. **Some crates publish no highlight query.** Those queries would have to
   be vendored under `resources/` the way Julia's is: kotlin-ng, perl,
   graphql, groovy, hcl (terraform), commonlisp, verilog, rst, gdscript.
   crystal 0.1.0 and nim 0.1.0 look like placeholder crates.
4. **Capture names vary.** Some upstream queries use Neovim- or Helix-style
   names (`constant.numeric.integer`, `function.elm`, `_function`, `spell`,
   `delimiter`). The longest-dotted-prefix fallback handles many of them,
   but each new language needs a capture audit.
5. **Queries that inherit from another language.** cpp, objc and glsl build
   on the C query; the probe used the C query concatenated with the
   language's own. `tree-sitter-highlight` does not resolve
   `; inherits:` comments.

## Deferred: the rest of Pandoc's list

These are recorded so that the on-demand loading work has a ready
inventory. Sizes are wasm32 static-library sizes.

**Probe OK, wasm32 C compile OK:** c (648 KB), cpp (3.4 MB, using the C
query plus its own), csharp (5.3 MB), go (236 KB), java (436 KB), swift
(4.2 MB), scala (4.5 MB), haskell (3.8 MB), elixir (1.4 MB), erlang
(460 KB), dart (1.5 MB), zig (696 KB), nix (92 KB), makefile (176 KB), diff
(32 KB), protobuf (104 KB), matlab (472 KB), elm (204 KB), fsharp (14 MB),
gleam (356 KB), glsl (884 KB), racket (132 KB), scheme (120 KB), vhdl
(1.7 MB), objectivec (5.3 MB), ada (348 KB), pascal (732 KB), odin
(2.2 MB), agda (2.2 MB), llvm (1.7 MB), gnuassembler via
`tree-sitter-asm` (36 KB), dosbat via `tree-sitter-batch` (224 KB), zsh
(6.3 MB), qml via `tree-sitter-qmljs` (1.6 MB), prolog (60 KB).

**Probe OK, wasm32 needs a sysroot fix:** ruby (`iswupper`), php
(`wchar.h`), ocaml (`towupper`), fortran (`iswxdigit`, `iswblank`), d
(`iswxdigit`), cmake (the `wctype.h` include bug).

**No highlight query in the crate:** kotlin (`tree-sitter-kotlin-ng`),
perl (wasm32 also needs `strncat` and `iswupper`), graphql (also needs the
shim bump), groovy, terraform (`tree-sitter-hcl`), commonlisp, verilog
(about 18 MB), rest (`tree-sitter-rst`), gdscript.

**Aliases that need no grammar:** `sqlmysql` and `sqlpostgresql` map to
`sql`, and `javascriptreact` maps to `jsx`. These cost nothing and could be
added at any time.

**Crate stuck on tree-sitter 0.19 to 0.22:** scss, dot, fish, bibtex,
org.

**Broken on crates.io:** `tree-sitter-latex` 0.1.0 does not link; its
build script never compiles the external scanner.

**No crate found:** apache, tcl, typst, awk, purescript, idris, stan,
stata, haxe, raku, sml, octave, mathematica, coffeescript, sass, tcsh,
nasm.

## Next steps

- [x] File a strand for the adopted set: bd-z6xw3psd (ini, toml with the conf alias,
      xml, dockerfile via containerfile; powershell optional). Follow the
      add-a-language procedure in
      `claude-notes/plans/2026-09-08-rust-highlight-grammar.md`, and include
      the sysroot and shim fixes containerfile needs.
- [ ] Check the `conf` to `toml` alias against realistic `conf` content
      (for example the Connect blocks) before committing to it.
- [x] File a strand for loading grammars on demand in the WASM
      distribution: bd-iow5o4x4. It gates everything in the deferred list.
