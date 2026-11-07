# Triage: issue #1110 — citeproc: document `lang` is not used for citation locale

- **Issue:** https://github.com/quarto-dev/q2/issues/1110 (reported by EwoutH, 2026-11-07, q2 0.42.0)
- **Branch:** `issue-1110`
- **Strand:** bd-kn01ctxg (related: bd-apudk = #255; incidental: bd-mn9jphx3)
- **Verdict:** confirmed bug — real work to do.
- **Target behavior (per user):** match Quarto 1, which delegates citeproc to
  Pandoc. So the reference is Pandoc's citeproc (`citeproc-hs`).

## Report

With `lang: nl` (or `nl-NL`) in front matter, and either the default CSL or
Zotero's `apa.csl` (no `default-locale`), q2 renders

```text
Smith, Jane. 2020. "A Page". July 12.
```

Dates and terms stay English. Expected Dutch (`juli 12` / `12 juli`).

## Reproduction

`repro.qmd` in this directory (verbatim from the issue).
`style-de.csl` is a tiny style with `default-locale="de-DE"` that renders one
term (`and`) and a `form="text"` date, used to probe precedence.

## Reference behavior (Pandoc 3.12.1, = Quarto 1)

`pandoc tmp.md --citeproc -t plain`, varying `lang` and `--csl`
(script: `pandoc-probe.sh`; output: `pandoc-probe.txt`):

| `lang`  | CSL                         | Pandoc output                                     |
| ------- | --------------------------- | ------------------------------------------------- |
| `nl`    | default (chicago)           | `Smith, Jane. 2020. ‘A page’. juli 12.`           |
| `nl`    | `style-de.csl` (de-DE)      | `A page en 12 juli 2020`                          |
| `nl-NL` | default                     | `Smith, Jane. 2020. ‘A page’. juli 12.`           |
| `de`    | default                     | `Smith, Jane. 2020. „A page“. Juli 12.`           |
| `en`    | `style-de.csl`              | `A page and July 12, 2020`                        |
| (unset) | default                     | `Smith, Jane. 2020. “A Page.” July 12.`           |
| (unset) | `style-de.csl`              | `A page und 12. Juli 2020`                        |

Conclusions:

1. **Precedence:** document `lang` > CSL `default-locale` > `en-US`. `lang`
   *overrides* the style's `default-locale` (not the other way around).
2. A bare language (`nl`, `de`) resolves to its primary dialect locale.
3. The effective locale drives three things, not just terms/dates:
   - terms and date formats (`juli`, `en`);
   - **quote characters** (`‘…’` for nl, `„…“` for de) — from the locale's
     `quotes`/`open-quote` terms;
   - **title-casing**: CSL `text-case="title"` applies only to English. With
     `lang: nl`, Pandoc prints `A page`, not `A Page`. q2's issue output
     shows `"A Page"`, so this is wrong in q2 too.
4. Quote placement relative to punctuation also follows the locale
   (`punctuation-in-quote`): en-US `“A Page.”` vs nl `‘A page’.`.

## Current q2 behavior (main @ 9c7a43bbe)

`target/debug/q2 render <file> --to html`, body text of `<main>`:

| `lang`  | CSL                    | q2 output                                        |
| ------- | ---------------------- | ------------------------------------------------ |
| `nl`    | default                | `Smith, Jane. 2020. “A Page”. July 12.`          |
| `de`    | default                | `Smith, Jane. 2020. “A Page”. July 12.`          |
| `en`    | default                | `Smith, Jane. 2020. “A Page”. July 12.`          |
| `nl`    | `style-de.csl`         | `A page und 12th Juli 2020`                      |
| (unset) | `style-de.csl`         | `A page und 12th Juli 2020`                      |

`lang` has no effect at all: the CSL `default-locale` (or en-US) always wins.

## Root cause in q2

`lang` is extracted but never reaches the processor:

- `crates/pampa/src/citeproc_filter.rs:1413` — `config.lang = get_meta_string(meta, "lang")`.
- `crates/pampa/src/citeproc_filter.rs:137` — `Processor::new(style)`;
  `config.lang` is unused.
- `crates/quarto-citeproc/src/types.rs:294` — `Processor::new` builds
  `LocaleManager::new(style.default_locale.clone())`; there is no way to pass
  a document language.

Other places that would still ignore `lang` once the constructor is fixed
(each reads the style's locale, or no locale, directly):

- `crates/quarto-citeproc/src/types.rs:830` — `get_date_format` computes
  `effective_lang` from `self.style.default_locale`, for matching in-style
  `<locale xml:lang=…>` date formats.
- `crates/quarto-citeproc/src/types.rs:~797` — `get_term` applies **every**
  in-style `<locale>` term override regardless of its `xml:lang`. Once `lang`
  works, an `apa.csl`-style `<locale xml:lang="en">` override would leak into
  a Dutch document. Should match the effective language the way
  `get_date_format` does (exact → base → no-lang).
- `crates/quarto-citeproc/src/eval.rs:289` — `is_english_reference` treats a
  reference with no `language` field as English. Per CSL 1.0.2 §text-case
  and citeproc-hs, a missing reference language defaults to the *effective
  locale*, so title-case must be skipped for `lang: nl`.
- `crates/quarto-citeproc/src/locale.rs:241` — `load_embedded_locale` maps a
  bare language to the **first** file with that prefix in embed iteration
  order. That's alphabetical, so `pt` → `pt-BR` (CSL primary dialect is
  `pt-PT`), `en` → `en-GB` (primary `en-US`), `zh` → `zh-CN` (ok by luck).
  It is also a raw `starts_with`, so a two-letter code can match a longer
  code that happens to share a prefix. Should use the CSL primary-dialect
  table (`locales.json` `primary-dialects` in citation-style-language/locales,
  which citeproc-hs also embeds).
- `crates/quarto-core/src/project/book/bibliography.rs:88` — the book-wide
  bibliography merge also calls `Processor::new(style)` and needs the same
  language (project/book `lang`).

## Incidental findings (independent of `lang`)

Both show up even with `lang` unset, so they are not part of this fix:

- **Ordinal day uses English suffix under de-DE**: `12th Juli` vs Pandoc's
  `12. Juli`. de-DE defines only the generic `ordinal` term; q2 seems to fall
  back to en-US `ordinal-NN` first. Filed as **bd-mn9jphx3**.
- **en-US punctuation-in-quote**: q2 `“A Page”.` vs Pandoc `“A Page.”`.
  Already tracked by **bd-citeproc-cluster-collapse-kns8uggw**.

## Pre-flight verify

`cargo xtask verify --skip-hub-build` on main @ 9c7a43bbe: all 18,741 Rust
tests pass. 9 hub-client vitest tests fail (`recordPayload.wasm.test.ts`,
`smokeAll*.wasm.test.ts`, …: "Failed to parse capture … expected a
sequence"), because the local WASM bundle in
`crates/wasm-quarto-hub-client/pkg/` predates the engine-record commits
(bundle from Nov 4; `108681f12` landed Nov 6), and `--skip-hub-build`
doesn't rebuild it. That's an environment issue and doesn't involve citeproc.
The worktree is at the same commit, so verify wasn't run again there.

## Related

- #255 — locale load failures fall back silently to English. Orthogonal
  (here the locale is never requested), but the fix should not regress it
  and could share a diagnostic: an unknown `lang` (e.g. `xx`) should fall
  back to en-US the way Pandoc does; whether to warn is a separate decision.

## Proposed fix scope

1. `quarto-citeproc`: add a way to pass the document language
   (e.g. `Processor::with_lang(style, Option<String>)` or
   `Processor::new_with_locale`), computing the effective locale as
   `lang.or(style.default_locale).unwrap_or("en-US")`, stored on the
   processor and used by `LocaleManager`, `get_date_format`, `get_term`
   style-locale matching, and the title-case English check.
2. Primary-dialect resolution for bare language codes.
3. `pampa` citeproc filter + `quarto-core` book bibliography: pass
   `config.lang` / project lang.
4. Tests: unit tests in quarto-citeproc for precedence (lang > default-locale
   > en-US), bare-code → primary dialect, in-style locale term matching, and
   non-English title-case suppression; a pampa integration test using
   `repro.qmd` that expects Pandoc's output
   (`Smith, Jane. 2020. ‘A page’. juli 12.`).

Out of scope here: `lang` affecting non-citation strings (crossref prefixes,
callout titles) — that is Quarto's own translation layer, not citeproc.
