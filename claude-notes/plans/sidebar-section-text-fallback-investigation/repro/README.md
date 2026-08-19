# Sidebar item with `text:` + `file:` + `contents:` renders the page title, not `text:`

**Observed with:** q2 0.19.0; re-verified failing on 0.24.0.
**Repro:** `q2 render` in this directory; compare with
`quarto render --output-dir _site-q1`.

A sidebar item that has both `text:` and `contents:` (a section-style
item with an explicit landing page) should display its configured
`text:`. q2 displays the linked page's title instead.

Root cause: `SidebarEntry::from_config_value`
(`crates/quarto-navigation/src/sidebar.rs`) treats any entry with
`contents:` as a `Section` and reads its display text from the
`section:` key only. When the entry uses `text:` (no `section:` key),
the section text comes out `None` and rendering falls back to the
`file:` page's title. Items with `text:` + `file:` but **no**
`contents:` ("Plain Item" here) render correctly.

## Expected (Quarto 1)

The sidebar entry for `landing.qmd` reads "Short Name" (the configured
`text:`).

## Actual (q2 0.19.0)

It reads "The Much Longer Landing Page Title" (the page's own title).

## Impact in the Connect docs port

The Cookbook sidebar's landing item shows "Posit Connect Cookbook"
instead of the configured "Cookbook" on all ~109 cookbook pages. Found
via br-wu5cbkws.
