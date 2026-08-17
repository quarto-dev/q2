# Repro: `_environment` file ignored (bd-environment-files-372u9qbs)

Copied from the origin repro at
`/Users/cscheid/repos/github/cscheid/q2-connect-docs/llms-info/repros/environment-file-ignored/`.

Run (make sure `REPRO_VERSION` is NOT exported):

```bash
env -u REPRO_VERSION cargo run --bin q2 -- render claude-notes/plans/environment-files-loading-investigation/repro
```

## Expected (Quarto 1 behavior)

`_environment` contains `REPRO_VERSION=from-env-file`, so the body renders
"Version is **from-env-file**."

## Actual (q2 @ 8518ac79, confirmed 2026-08-09)

The file is never read; the shortcode resolves against the process env only:

```
Version is <strong>?env:REPRO_VERSION</strong>.
```

plus one `[Q-16-5] Environment variable not set` warning per use.
