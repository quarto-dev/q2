# Investigation artifacts for bd-format-css-not-copied-crn3bjdz

`repro/` is a minimal website project mirroring the external repro at
`~/repos/github/cscheid/q2-connect-docs/llms-info/repros/format-css-not-copied/`
(source of truth for the Q1 comparison table).

Run:

```bash
cargo run --bin q2 -- render claude-notes/plans/format-css-not-copied-investigation/repro
```

Expected (buggy) behavior at `main` @ `10d86829`, confirmed 2026-08-14:

- `_site/styles.css` and the extension css are **not written** anywhere in
  the output tree;
- both `<link>` hrefs are emitted **verbatim at every depth**
  (`styles.css` on `deep/deeper/index.html`, beside a correctly rebased
  `../../site_libs/quarto/quarto-theme-*.css`);
- render is clean: exit 0, no diagnostic.

Note `_extensions/acme/widget/` deliberately contains only `widget.css` —
no `_extension.yml` — so no extension machinery is involved; the path just
happens to point into `_extensions/`.

The rendered `_site/` is not committed; re-render to regenerate.

See `claude-notes/plans/2026-08-14-format-css-not-copied.md` for the full
investigation and design questions.
