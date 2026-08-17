# q2 ignores `aliases:` — no redirect stubs are generated

**Observed with:** q2 0.15.0 (also checked q2 HEAD: no `aliases`
handling in `crates/`, no alias-related commits)
**Repro:** `q2 render` in this directory.

## Expected (Quarto 1)

`current/index.qmd` declares:

```yaml
aliases:
  - /old-name.html
  - ../previous/index.html
```

Quarto 1 writes a small HTML redirect stub at each alias path
(`_site/old-name.html`, `_site/previous/index.html`) that
`window.location.replace`s to the canonical page, preserving hash and
query string.

## Actual (q2 0.15.0)

Only `_site/index.html` and `_site/current/index.html` are written.
The `aliases` key is silently ignored — no redirect stubs, and no
warning that the key was dropped.

## Impact on the Connect docs

69 source files in `docs-quarto-2` declare `aliases:`; the Q1 reference
site contains **99 redirect stub HTML files** that the q2 render omits
entirely (this is the whole 451 vs 352 file-count gap between
`docs-quarto-1/_site` and `docs-quarto-2/_site`). External links and
bookmarks pointing at pre-reorganization URLs would 404.
