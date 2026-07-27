# Repro at HEAD — bd-97yc brand-aware favicon fallback

**Date:** 2026-07-27
**HEAD:** `dd87a8b5` (`main`), `cargo xtask verify --skip-hub-build` green
(all 14 steps) before these runs.

Two sibling website projects, identical except for one `_quarto.yml` line.
`_site/` outputs are not committed — regenerate with the invocations below.

## repro-site/ — brand supplies a small logo, `website.favicon` unset

```yaml
# _quarto.yml
project:
  type: website
website:
  title: Brand Favicon Repro
  site-url: https://example.com
brand: _brand.yml
```

```yaml
# _brand.yml
color:
  primary: "#4b2e83"
logo:
  small: logo.png
```

```console
$ cargo run --bin q2 -- render claude-notes/plans/brand-aware-favicon-fallback-investigation/repro-site
Rendering project: …/repro-site (type: website)
Rendered 1 of 1 files to …/repro-site/_site

$ find _site -maxdepth 2 -type f | sort
_site/index.html
_site/robots.txt
_site/sitemap.xml

$ grep -c 'rel="icon"' _site/index.html
0

$ grep -ro '4b2e83' _site/ | head -3
_site/site_libs/quarto/quarto-theme-c860fa5ab64a67db.css:4b2e83
_site/site_libs/quarto/quarto-theme-c860fa5ab64a67db.css:4b2e83
_site/site_libs/quarto/quarto-theme-c860fa5ab64a67db.css:4b2e83
```

**Observed (output inspected):** no `<link rel="icon">` in `index.html`, and
`logo.png` is not copied into `_site/`. The brand *was* resolved — the primary
colour `#4b2e83` reached the compiled theme CSS — so this is specifically the
favicon consumer that never consults it, not a brand-loading failure.

**Q1 behavior for the same input:** `<link rel="icon" href="logo.png"
type="image/png">` plus the logo copied to the site root
(`external-sources/quarto-cli/src/project/types/website/website.ts:185-205`).

## control-site/ — same project with `website.favicon: logo.png` added

```console
$ cargo run --bin q2 -- render claude-notes/plans/brand-aware-favicon-fallback-investigation/control-site
Rendered 1 of 1 files to …/control-site/_site

$ find _site -maxdepth 1 -type f | sort
_site/index.html
_site/logo.png
_site/robots.txt
_site/sitemap.xml

$ grep -o '<link rel="icon"[^>]*>' _site/index.html
<link rel="icon" href="logo.png" type="image/png">
```

**Observed (output inspected):** link emitted, MIME type derived, file copied.

## Reading

The control proves the whole favicon machinery bd-b9mz built —
`WebsiteFaviconTransform` (`<link>` emission, MIME lookup, page-relative href)
and `copy_favicon` (file copy) — works correctly. The only missing piece is the
*source* of the path: `website_config::website_favicon()` reads exactly one key
and gives up. That is the single reader both call sites go through, which is why
the fallback is a small change once a resolved `Brand` is reachable from
quarto-core (see the plan's Obstacle 1).
