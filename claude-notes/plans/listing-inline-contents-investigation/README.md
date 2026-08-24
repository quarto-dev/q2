# Investigation fixtures — bd-listing-inline-contents-tyy446ze

Copied from `/Users/gordon/src/q2-positron-docs/llms-info/repros/listing-inline-contents/`
(local-only repo) so the record is durable. All use the built-in
`type: default` listing so custom templates are not a variable.

| Fixture | `contents:` shape | Expected (Q1) | q2 at `596ceb572` |
| --- | --- | --- | --- |
| `control/` | two globs | 2 items | 2 items, 0 warnings |
| `repro/` | two inline records, each with `path:` naming a sibling `.qmd` | 2 items titled from the YAML | 0 items, 2 × Q-12-2 |
| `mixed/` | one record with `path:` + one glob | 2 items | 1 item (glob only), 1 × Q-12-2 |
| `linkonly/` | two records with `link:`/`icon:`, **no `path:`** (the shape every Positron card grid uses) | 2 items, no backing file | 0 items, 2 × Q-12-2 |

Run any of them with `cargo run --bin q2 -- render <fixture>` from the repo root.
Compare with `quarto render` (Q1) for the expected output.
