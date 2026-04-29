# 06-site-metadata — sitemap, robots, favicon, title prefix, canonical URL

Demonstrates the post-render outputs that turn a directory of HTML
into a deployable website: sitemap, robots, favicon copy, and the
per-page `<head>` additions that make pages SEO-friendly.

## What this demonstrates

- **`<title>` prefix.** Every page's `<title>` becomes
  `<page title> – <website.title>`.
- **`<link rel="canonical">`.** Each page's `<head>` carries a
  canonical URL derived from `website.site-url` plus the page's
  output path.
- **`<link rel="icon">`.** When `website.favicon:` is set, every
  page's `<head>` adds a favicon link, and the favicon file is
  copied to the project's output dir.
- **`_site/sitemap.xml`.** All rendered pages are listed with
  absolute URLs (`<loc>`) and `<lastmod>` timestamps. Generated
  only when `website.site-url` is set.
- **`_site/robots.txt`.** A minimal robots file pointing at the
  sitemap, generated alongside it.

## How to run

```bash
cargo run --bin q2 -- render examples/websites/06-site-metadata
```

## What to look for

After rendering, the output directory should contain:

```
_site/
├── api.html
├── favicon.svg
├── guides.html
├── index.html
├── robots.txt
├── sitemap.xml
└── site_libs/
    └── quarto/
        └── quarto-theme-<hash>.css
```

Inspect each artifact:

```bash
cat _site/sitemap.xml
```

You should see one `<url>` block per rendered page, with
`<loc>https://example.com/my-site/<page>.html</loc>`.

```bash
cat _site/robots.txt
```

Should be a single `Sitemap:` line referencing the absolute URL.

```bash
grep -E '<title>|rel="canonical"|rel="icon"' _site/guides.html
```

Should show:

- `<title>Guides – My Site</title>`
- `<link rel="canonical" href="https://example.com/my-site/guides.html">`
- `<link rel="icon" href="favicon.svg" type="image/svg+xml">`

## Try it

- Remove `website.site-url:` from `_quarto.yml` and re-render. The
  `_site/sitemap.xml` and `_site/robots.txt` should both
  disappear, and per-page `<link rel="canonical">` should be
  absent.
- Remove `website.favicon:` and re-render. The favicon file is no
  longer copied; the `<link rel="icon">` is no longer emitted.
- Override the title prefix on a single page by setting
  `title-prefix: false` in its frontmatter. (Not yet implemented in
  v1; see `bd-97yc` for the home-page carve-out follow-up.)

## Notes

- The favicon path can be relative to the project root
  (`favicon.svg`) or any other path Quarto can resolve. PNG, ICO,
  and SVG all work; the `<link>`'s `type=` attribute is set from
  the file extension.
- The sitemap currently rewrites the whole file on every render. A
  Phase-8 follow-up (`bd-pphv`) makes the sitemap merge in place,
  preserving entries for pages that weren't re-rendered. Exercised
  by example `07-incremental`.
- Per-page favicon override (`meta.favicon`) is tracked as
  `bd-7h6a`.
- Open Graph / Twitter card meta tags are tracked as `bd-tyvt`.
