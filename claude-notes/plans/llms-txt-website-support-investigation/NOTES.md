# Investigation notes — bd-llms-txt-unimplemented-oih6z6j7

**2026-08-14, main @ 3ac596e0.**

Repro in `repro/` (copied from the connect-docs skein repro at
`~/repos/github/cscheid/q2-connect-docs/llms-info/repros/llms-txt-unimplemented/`).

Observed at HEAD:

```
$ cargo run --bin q2 -- render claude-notes/plans/llms-txt-website-support-investigation/repro
Rendering project: .../repro (type: website)
Rendered 2 of 2 files to .../repro/_site
$ find repro/_site -name 'llms.txt' -o -name '*.llms.md'
(no output — neither file exists; _site holds only about.html, index.html, site_libs)
```

`website.llms-txt: true` accepted, no warning, no artifacts — confirms the
strand. `_site/` deliberately not committed.

Key code-reading findings are inlined in the plan
(`claude-notes/plans/2026-08-14-llms-txt-website-support.md`, §"What the
code looks like today").
