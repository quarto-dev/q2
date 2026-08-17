# `add_html_dependency`'s `version` field is unsupported, and warns once per call

**Observed with:** q2 0.20.0.
**Repro:** `q2 render` in this directory; compare with
`quarto render --output-dir _site-q1`.

A Lua filter attaches an HTML dependency that declares a version, the
way real extensions do:

```lua
quarto.doc.add_html_dependency({
  name = 'versioned-dep',
  version = '1.0.0',
  scripts = { 'versioned-dep.js' }
})
```

## Expected (Quarto 1)

No diagnostic. The version becomes part of the asset directory name, so
a site can carry two versions of the same dependency without collision:

```
_site/site_libs/quarto-contrib/versioned-dep-1.0.0/versioned-dep.js
```

## Actual (q2 0.20.0)

```
Warning [Q-11-1]: add_html_dependency: field 'version' is not yet supported and will be ignored
```

…and the assets land in an unversioned directory:

```
_site/site_libs/libs/versioned-dep/versioned-dep.js
```

The warning is truthful and self-documenting. Two things about it are
worth separating:

1. **The field is unimplemented.** For most extensions this is
   cosmetic — one dependency, one version, nothing to collide with.
2. **The warning is emitted per call, not per distinct dependency.**
   `add_html_dependency` de-duplicates by name, so extensions are
   written to call it unconditionally for every matching element; the
   documentation of the API encourages exactly that. This repro's
   filter runs on two paragraphs and produces two identical warnings.

Point 2 is what makes it noisy in practice rather than cosmetic. See
the Connect docs impact below.

## Connect docs impact

The `mermaid-zoom` extension calls `add_html_dependency` once per
mermaid diagram, with a `version`. Across the 33 diagrams on 14 pages
that is **33 warnings per full render** for one field on one line —
enough to take the site's render from one distinct diagnostic to two.

Worked around in `docs-quarto-2/_quarto.yml` with the `diagnostics:`
suppression added in q2 0.20.0, so the extension source can stay
byte-identical to the upstream Quarto 1 copy:

```yaml
diagnostics:
  Q-11-1:
    level: off
    reason: "add_html_dependency version: field is not yet supported by q2"
```

That is the suppression mechanism working as designed, and it is worth
recording as the first real use of it in this project.
