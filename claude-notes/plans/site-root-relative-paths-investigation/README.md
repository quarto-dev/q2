# Investigation artifacts — bd-root-relative-paths-design-fc5pvkcv

See `../2026-08-13-site-root-relative-paths.md` for the plan skeleton.

`repro/` is a minimal website project (trimmed from the strand's
out-of-repo repro at
`q2-connect-docs/llms-info/repros/root-absolute-paths/`). To reproduce:

```bash
cargo run --bin q2 -- render claude-notes/plans/site-root-relative-paths-investigation/repro
grep -o 'src="[^"]*"' .../repro/_site/deep/deeper/index.html
```

Expected (buggy) result at main @ 81d31cbc: the markdown link on the
deep page rebases to `../../index.html`, while both the markdown image
and the raw-HTML image keep `/images/x.svg` verbatim. Delete `_site/`
after inspecting; it is not committed.
