# Investigation artifacts — bd-q120-masks-config-md-diagnostic-a039r80t

Plan: `../2026-08-19-q120-forward-underlying-diagnostic.md`

## repro/

Minimal in-repo version of the external repro
(`q2-connect-docs/llms-info/repros/q120-masks-config-md-diagnostic/`).
A website project whose `_quarto.yml` `page-footer.center` contains
markdown that fails q2's attribute grammar (key-value pair before
class specifier):

```markdown
![logo](images/logo.svg){width="65px" .light-content}
```

`body-control.qmd` contains the identical text in a document body.

Run:

```bash
cargo run --bin q2 -- render claude-notes/plans/q120-forward-underlying-diagnostic-investigation/repro
```

Observed at main @ 6bee9ebe (captured, ANSI-stripped, in
`repro/observed-output.txt`):

- body-control.qmd → **Q-2-3** error with the precise two-part span
  ("This key-value pair cannot appear before the class specifier" /
  "This class specifier appears after the key-value pair").
- _quarto.yml footer → only the generic **Q-1-20** warning
  ("Could not parse '…' as markdown" + `!str` hint). The underlying
  Q-2-3 is discarded at `crates/pampa/src/pandoc/meta.rs:120`
  (`Err(_parse_errors)`).

`_site/` output is deleted before commit; re-render to regenerate.
