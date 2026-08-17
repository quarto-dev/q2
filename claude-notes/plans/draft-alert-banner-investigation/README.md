# Investigation artifacts — draft alert banner (bd-draft-banner-missing-hgx1gkqm)

Plan: `../2026-08-13-draft-alert-banner.md`

## `repro/`

Minimal reproduction, copied from
`/Users/cscheid/repos/github/cscheid/q2-connect-docs/llms-info/repros/draft-banner-missing/`
so it survives independently of that external checkout.

- `drafty.qmd` — `draft: true` front matter.
- `index.qmd` — non-draft control.
- `_quarto.yml` — website project. `draft-mode: unlinked` is set so that
  **Quarto 1** renders the draft page at all; q2 ignores the key (it has no
  `draft-mode` support — see `bd-w0o9`).

Reproduce:

```bash
cd claude-notes/plans/draft-alert-banner-investigation/repro
cargo run --bin q2 -- render
grep quarto-draft-alert _site/drafty.html   # absent at 0dcd7e83
```

The Quarto 1 reference output (`_site-q1/drafty.html`, which *does* contain the
banner) is not copied here — it is a full rendered site. Regenerate it in the
original repro directory with `quarto render --output-dir _site-q1`.

## Findings not obvious from the repro

Recorded in the plan, but the short version:

- The `#quarto-draft-alert` CSS **already ships** — it is in
  `resources/scss/bootstrap/_bootstrap-rules.scss:2604` (not `crates/quarto-sass`,
  where the strand looked) and reaches `_site/site_libs/quarto/quarto-theme-*.css`.
- Bootstrap Icons **already ships** to `_site/site_libs/bootstrap/`.
- `draft: "Draft"` **already exists** in `resources/language/_language.yml:130`,
  fully translated — so the banner text must be localized.
- `$if(draft)$` in `FULL_HTML_TEMPLATE` **already works**; verified with a
  temporary probe div that appeared in `drafty.html` and not `index.html`
  (probe reverted).

Only the HTML emission is missing.
