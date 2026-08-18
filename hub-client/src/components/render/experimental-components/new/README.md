These can be used in hub client by creating .tsx files for them and putting
them in your frontmatter like so:

```
---
format: q2-debug
render-components:
  - "simple\\_strings.tsx"
  - html.tsx
  - comments.tsx
  - "html\\_slide.tsx"
  - "drag\\_div.tsx"
source-location: full
---
```
## What a component receives

A component is handed the AST **as the pipeline leaves it**, not as the
author wrote it. Normalization transforms run first, and some of them
restructure the very subtree a component is inspecting.

The one that bites most often is `SectionizeTransform`. It wraps every
heading, together with the content that follows, in a section `Div` —
including headings nested inside a component's own `Div`:

```
::: {.kanban}
## backlog
* item one
:::
```

reaches the component as

```
Div(.kanban)
  Div(#backlog .section .level2)[ Header(2) "backlog", BulletList ]
```

not as a flat `[Header, BulletList]` run. `kanban_rc.jsx` shows the shape
to expect; scanning a Div's direct children for `Header` nodes will find
nothing.

**Reading vs writing.** This applies to the node you *render*. Editing
components resolve back to the *source* node (`edit.resolveSource`), which
is pre-transform — flat headings, no sections — and edits written there
must keep that shape, or sections end up in the user's `.qmd`. The two
paths are deliberately asymmetric; see `kanban.tsx` in
`crates/quarto/tests/playwright-fixtures/q2-preview/render-components-kanban/`.

This coupling between components and the pipeline is real and, for now,
unversioned: a pipeline change can require a component change. Expect some
churn while the API settles.
