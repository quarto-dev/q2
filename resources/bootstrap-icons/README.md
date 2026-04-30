# Bootstrap Icons

Vendored copy of [Bootstrap Icons](https://icons.getbootstrap.com/),
used by Q2's website rendering for icons in the prev/next
page-navigation strip and any other website chrome that uses `bi-*`
classes.

## Source

Files copied from
`external-sources/quarto-cli/src/resources/formats/html/bootstrap/dist/`
(Quarto 1.x, Bootstrap Icons v1.13.1).

## Licensing

Bootstrap Icons is MIT-licensed. See
<https://github.com/twbs/icons/blob/main/LICENSE>.

## Updating

When Quarto 1 (or Bootstrap Icons upstream) updates the version,
re-copy `bootstrap-icons.css` and `bootstrap-icons.woff` from the
quarto-cli source tree above as a pair — the CSS references the
woff by relative path with a content-hash query string, so the two
files must travel together.

## Bundling

Embedded by `crates/quarto-core/src/transforms/website_bootstrap_icons.rs`
via `include_bytes!` and shipped as Project-scope artifacts to
`_site/site_libs/bootstrap/{bootstrap-icons.css, bootstrap-icons.woff}`
on every website render. Tracking issue: bd-bsut.
