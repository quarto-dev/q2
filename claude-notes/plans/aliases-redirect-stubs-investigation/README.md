# Investigation artifacts — `aliases:` redirect stubs

Supporting material for
`claude-notes/plans/2026-08-12-aliases-redirect-stubs.md`
(braid `bd-aliases-redirects-missing-sch7cd1g`).

| File | What it is |
| --- | --- |
| `repro/` | Minimal two-page website reproducing the bug. `cargo run --bin q2 -- render repro` writes 2 HTML files; Quarto 1 writes 4. Copied from `q2-connect-docs/llms-info/repros/aliases-redirects-missing/`, minus its `_site/`. Phase 0 should promote this into `crates/quarto-core/tests/integration/`. |
| `q1-website-aliases.ts` | Verbatim copy of Quarto 1's `src/project/types/website/website-aliases.ts` — the reference implementation, so the plan is readable without an `external-sources/` checkout. |
| `q1-redirect-map.ejs` | Verbatim copy of Quarto 1's stub template (`resources/projects/website/templates/redirect-map.ejs`). |
| `connect-docs-alias-corpus.txt` | The 106 unique front-matter alias entries across the 69 declaring files in `q2-connect-docs/docs-quarto-2`. This is the corpus the shape breakdown in the plan is measured from. |

## Reproducing the corpus extraction

From a `docs-quarto-2` checkout — note the `awk` guard, which reads *only* the
front-matter `aliases:` block. A naive `grep -A20 '^aliases:'` picks up unrelated
body list items and inflates the count.

```bash
for f in $(grep -rl '^aliases:' --include='*.qmd' --include='*.md' .); do
  awk '/^aliases:[[:space:]]*$/{inb=1;next}
       inb && /^[[:space:]]*-[[:space:]]/{sub(/^[[:space:]]*-[[:space:]]*/,"");print;next}
       inb{inb=0}' "$f"
done | sed 's/^["'"'"']//;s/["'"'"']$//' | sort -u
```
