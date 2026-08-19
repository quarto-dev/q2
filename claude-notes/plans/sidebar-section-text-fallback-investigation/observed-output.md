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
