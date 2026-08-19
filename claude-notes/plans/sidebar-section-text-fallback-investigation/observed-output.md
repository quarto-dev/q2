# End-to-end confirmation at HEAD (`e6ac236d`, 2026-08-19)

Invocation (repro sources copied to a scratch dir so `_site/` stays out of the repo):

```
cargo run --bin q2 -- render <scratch>/repro-run
# Rendered 4 of 4 files to <scratch>/repro-run/_site
```

Observed sidebar markup in `_site/index.html`:

```
sidebar-item-text sidebar-link">The Much Longer Landing Page Title
...
<span class="menu-text">Inner</span>
<span class="menu-text">Plain Item</span>
```

- The `text: "Short Name"` + `file: landing.qmd` + `contents:` entry renders as
  **"The Much Longer Landing Page Title"** (the landing page's title) — the bug.
- The `text: "Plain Item"` + `file: plain.qmd` entry (no `contents:`) renders its
  configured text correctly — matching the Link-branch analysis.

Output was inspected directly (grep of the rendered HTML above), not inferred
from exit status.

# End-to-end verification after the fix (Phase 3, 2026-08-19)

All runs: `cargo run --bin q2 -- render <scratch>/repro-run`, output inspected
by grepping `_site/index.html` / stderr directly.

**1. Original repro (`text: "Short Name"` + `file:` + `contents:`):**

```
sidebar-item-text sidebar-link">Short Name</a>
```

The configured label renders; the landing page's title no longer leaks in.

**2. Formatted label (`text: "*Short* Name"`):**

```
sidebar-link"><em>Short</em> Name</a>
sidebar-link"><span class="menu-text"><em>Plain</em> Item</span></a>
```

PandocInlines survive the fallback in both section headers and leaf items.

**3. Both-keys conflict (`section: "Winning Label"` + `text: "Losing Label"`):**

```
Warning: [Q-13-10] Sidebar entry has both `section:` and `text:`
  ╭─[ …/_quarto.yml:9:15 ]
9 │         text: "Losing Label"
  │               ───────┬─────
Affected files: index.qmd, inner.qmd, landing.qmd (and 1 other)
...
sidebar-link">Winning Label</a>
```

`section:` wins in the rendered sidebar; the warning prints once for the whole
4-page render (not once per page) because it carries a source location and
`coalesce_by_source` groups identical located diagnostics — see bd-drdx1pew
for why the location-less Q-13-5/Q-13-6 still repeat.
