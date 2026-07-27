# Brand-aware favicon fallback (bd-97yc) — before / after

Three website projects driven through the real binary. `_site/` and
`.quarto/` are not committed — regenerate with the invocations below.

| Fixture | `website.favicon` | brand `logo.small` | Exercises |
| --- | --- | --- | --- |
| `repro-site/` | unset | `logo.png` at project root | the fallback |
| `subdir-brand-site/` | unset | `logo.png` under `_brand/` | brand-relative → project-relative rebasing |
| `control-site/` | `favicon.ico` | `logo.png` | precedence: explicit wins |

## Before — HEAD `dd87a8b5`, no fallback

`cargo xtask verify --skip-hub-build` green (all 14 steps), so this was a
missing feature rather than a broken tree.

```console
$ cargo run --bin q2 -- render claude-notes/plans/brand-aware-favicon-fallback-investigation/repro-site
Rendered 1 of 1 files to …/repro-site/_site

$ find _site -maxdepth 2 -type f | sort
_site/index.html
_site/robots.txt
_site/sitemap.xml

$ grep -c 'rel="icon"' _site/index.html
0

$ grep -ro '4b2e83' _site/ | head -1
_site/site_libs/quarto/quarto-theme-c860fa5ab64a67db.css:4b2e83
```

No `<link rel="icon">`, and `logo.png` not copied — while the brand *was*
resolved (its primary colour reached the compiled theme CSS). So the gap was
specifically the favicon consumer never consulting the brand.

## After — `6882d57c`

### `repro-site/` — brand at the project root

```console
$ cargo run --bin q2 -- render claude-notes/plans/brand-aware-favicon-fallback-investigation/repro-site
Rendered 1 of 1 files to …/repro-site/_site

$ find _site -maxdepth 2 -type f | sort
_site/index.html
_site/logo.png          # ← now copied
_site/robots.txt
_site/sitemap.xml

$ grep -o '<link rel="icon"[^>]*>' _site/index.html
<link rel="icon" href="logo.png" type="image/png">

$ xxd _site/logo.png | head -1
00000000: 8950 4e47 0d0a 1a0a                      .PNG....
$ xxd logo.png | head -1
00000000: 8950 4e47 0d0a 1a0a                      .PNG....
```

**Observed (output inspected):** link emitted with the MIME type derived from
the extension, and the file copied byte-identically.

### `subdir-brand-site/` — the load-bearing case

`brand: _brand/_brand.yml`, whose `logo.small: logo.png` means
`_brand/logo.png` in project terms. Two pages, one nested.

```console
$ cargo run --bin q2 -- render claude-notes/plans/brand-aware-favicon-fallback-investigation/subdir-brand-site
Rendered 2 of 2 files to …/subdir-brand-site/_site

$ find _site -name '*.png'
_site/_brand/logo.png

$ grep -o '<link rel="icon"[^>]*>' _site/index.html
<link rel="icon" href="_brand/logo.png" type="image/png">

$ grep -o '<link rel="icon"[^>]*>' _site/docs/api.html
<link rel="icon" href="../_brand/logo.png" type="image/png">

# Resolve the nested page's href against the filesystem the way a
# browser would — it must land on the copied file.
$ ls -l _site/docs/../_brand/logo.png
-rw-r--r--  1 cscheid  staff  8 Jul 27 16:20 _site/docs/../_brand/logo.png
```

**Observed (output inspected):** the brand-relative `logo.png` became the
project-relative `_brand/logo.png`, the nested page got the correct `../`
prefix, and both hrefs resolve to a file that exists. The last command is the
point of the fixture: an implementation that forwarded the raw YAML path would
emit `href="logo.png"` and copy nothing, and would still pass the
project-root case above.

### `control-site/` — explicit favicon wins

```console
$ cargo run --bin q2 -- render claude-notes/plans/brand-aware-favicon-fallback-investigation/control-site
Rendered 1 of 1 files to …/control-site/_site

$ grep -o '<link rel="icon"[^>]*>' _site/index.html
<link rel="icon" href="favicon.ico" type="image/x-icon">

$ find _site -maxdepth 1 -type f | sort
_site/favicon.ico
_site/index.html
_site/robots.txt
_site/sitemap.xml
```

**Observed (output inspected):** the explicit `favicon.ico` wins, and
`logo.png` is neither linked nor copied. The fixture originally used
`favicon: logo.png`, which after the change proved nothing — both the explicit
key and the fallback would have produced `logo.png`. It now names a distinct
file so precedence is actually observable.

## Not verified

`q2 preview` / hub-client. The fallback follows wherever `parse_config` runs;
whether the hub-client's project render path reaches it is untested here. See
bd-wjg4h (browser-verify brand under preview) and bd-k5rxujiy (preview asset
walker misses meta-driven images).
