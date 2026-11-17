# Default CSL style

`chicago-author-date.csl` is pampa's `DEFAULT_CSL_STYLE`
(`crates/pampa/src/citeproc_filter.rs`): the style used when a document
asks for citations without naming a `csl:`. Registered as
`chicago-author-date-csl` in `resources/vendored.toml`.

## Source

Byte-identical to **Pandoc's `data/default.csl`** at pandoc `3.13` (and
unchanged since `3.11`, verified 2026-11-09) — so q2 and Pandoc format
citations with the same default style. It is *not* a copy of the CSL styles
repository's current `chicago-author-date.csl`: Pandoc ships its own revision
(its `<updated>` stamp, 2025-08-07, is newer than the styles repo's master,
and the two differ substantially).

## Updating

Follow Pandoc, not the styles repo:

```bash
git -C <pandoc clone> show <tag>:data/default.csl > crates/pampa/resources/csl/chicago-author-date.csl
```

then update the tag above and `version` in the registry entry, and run
`cargo nextest run -p quarto-citeproc` and `-p pampa citeproc`.
