# Repro fixture for bd-jsvetdea

`theme.scss` sets `$grid-body-width: 52rem`. The Bootstrap grid mixins do
`calc(... - 3em)`-style arithmetic that grass rejects for `rem`, so the theme
compile fails. At HEAD the render reports success and ships the ~7KB
`DEFAULT_CSS` instead of the compiled Bootstrap bundle.

Run from the repo root:

    cargo run --bin q2 -- render claude-notes/plans/theme-scss-compile-error-swallowed-investigation/repro
    ls -l claude-notes/plans/theme-scss-compile-error-swallowed-investigation/repro/_site/site_libs/quarto/

`_site/` is gitignored via the `.gitignore` next to this file.
