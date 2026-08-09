# q2 table listings ignore `fields:` / `field-display-names:`

**Observed with:** q2 0.14.0
**Repro:** `q2 render` in this directory.

## Expected (Quarto 1)

A `type: table` listing with `fields: [title]` renders a one-column
table; `field-display-names` renames the header (see
`docs-quarto-1/_site/how-to/index.html`: single "How To" column).

## Actual (q2)

The built-in table item template is hard-coded to three columns:

```
| [$title$]($path$){.no-external} | $date$ | $author$ |
```

(`crates/quarto-core/src/project/listing/templates/item-table.template`)

- `fields:` and `field-display-names:` are ignored; output always has
  Title | Date | Author columns (empty when items lack those fields).
- Each item missing `date`/`author` produces "Undefined variable"
  doctemplate diagnostics, surfaced as one `Q-12-10` warning
  ("Listing `guides` doctemplate produced N diagnostic(s)").

In the Connect docs this hits `how-to/index.md` (the only `type: table`
listing): renders three columns instead of one, 8 diagnostics.

## Incidental finding while building this repro

A standalone project of `.md` files renders **0 files** unless
`project.render` includes a `"**/*.md"` pattern — q2's Q-PROJECT-EMPTY
diagnostic explains this well, but beware when building repros: a
"clean" run may have rendered nothing.
