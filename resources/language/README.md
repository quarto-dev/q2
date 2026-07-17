# Language term files (localization)

Per-language translations of Quarto's document-visible terms (callout titles,
crossref titles/prefixes, TOC title, title-block labels, etc.).

- `_language.yml` — English defaults; the authoritative catalog of term keys.
- `_language-<tag>.yml` — overrides for one BCP 47 tag. Region/script variants
  (`_language-pt-BR.yml`, `_language-de-CH.yml`, `_language-sr-Latn.yml`) are
  merged on top of their parent (`_language-pt.yml`, …) via the subtag walk in
  `crates/quarto-core/src/language.rs`.

These files are embedded into the `quarto-core` binary via `include_dir!`
(see `crates/quarto-core/src/language.rs`) and resolved at render time based
on the document's `lang` option and `language:` metadata overrides.

## Provenance

Copied from `quarto-cli` (`src/resources/language/`) at commit
`45caede32a0f987c4a377a952120cac6d624cb31` (v1.10.3-116-g45caede32) on
2026-07-17, per the external-sources policy (compile-time resources must live
in-repo, never referenced from `external-sources/`).

To update: re-copy the files from a current `quarto-cli` checkout, update the
commit hash above, and run the catalog integrity test
(`cargo nextest run -p quarto-core -E 'test(language_catalog)'`). Key-level
compatibility with Quarto 1 is deliberate — do not rename keys.

Design: `claude-notes/plans/2026-07-17-localization-i18n-design.md`
(braid strand bd-llhlzd7p).
