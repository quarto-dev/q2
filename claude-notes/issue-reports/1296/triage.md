# Issue #1296 — Mermaid diagrams ignore mermaid SCSS theme variables (unreadable in dark mode)

- **GitHub**: https://github.com/quarto-dev/q2/issues/1296
- **Reporter**: @cscheid (Carlos Scheidegger), 2026-11-24
- **Triage date**: 2026-11-24
- **Branch**: topic branch `issue-1296` in the main checkout (no worktree, at the user's request), based on `main` @ `ecf1bec29`
- **Braid strand**: none new. This duplicates **bd-sehm2rha** (primary) and **bd-nj25kgbu**. See Outcome.
- **Scope**: the whole issue covers SCSS variables, the `themeCSS` injection, `mermaid: theme:`, specificity, tests and a manual check. This triage does not cover #1095 (subgraph title sizing), which only shares the topic.

## Summary

The report says q2 ships no Mermaid theming. It emits no `$mermaid-*` defaults, no `:root --mermaid-*` custom properties, no `themeCSS`, and no `mermaid: theme:` pass-through, so every diagram uses Mermaid's `default` theme. That theme is unreadable on dark backgrounds. I reproduced all three claims on `main` @ `ecf1bec29`. **This is a real gap, but it is already tracked.** bd-sehm2rha (P2, under the `_quarto-rules.scss` parity epic bd-4doe9lvt) covers the SCSS half and notes that the JS `themeCSS` half is needed too. bd-nj25kgbu (P3) covers the same CSS-variable model plus the `mermaid: theme:` pass-through and parity with the preview. The issue adds useful evidence: measured contrast ratios, a dark-mode framing, a specificity check and a test plan. I added that evidence to bd-sehm2rha as a comment instead of filing a third strand.

## Reproduction

Fixtures in this directory:

- `repro.qmd` + `custom.scss`: a light/dark theme pair (`[cosmo, custom.scss]` / `[darkly, custom.scss]`). `custom.scss` sets `$mermaid-fg-color: #ff00ff`. The document has a sequence diagram and a flowchart with a `subgraph`.
- `repro-theme-option.qmd`: `mermaid: theme: forest` with a plain `format: html`.

The `claude-notes` project excludes `issue-reports/` from its render list, so render a copy outside the project:

```bash
S=$(mktemp -d); cp claude-notes/issue-reports/1296/* $S/; cd $S
q2 render repro.qmd
q2 render repro-theme-option.qmd
grep -l 'ff00ff\|mermaid-fg' repro_files/*.css repro-theme-option_files/*.css   # → no matches
grep -o 'mermaid.initialize([^)]*)' repro.html repro-theme-option.html
grep -c 'mermaid-theme' repro-theme-option.html                                  # → 0
```

Observed:

| Check | Expected (Quarto 1) | Observed (q2) |
|---|---|---|
| `--mermaid-fg-color` in `styles.css` / `styles-dark.css` | present (`#ff00ff` from `custom.scss`) | absent from both, and so is the literal `ff00ff` |
| `mermaid.initialize` arguments | `{ startOnLoad: false, themeCSS: "…var(--mermaid-*)…" }` | `{ startOnLoad: false }` |
| `mermaid: theme: forest` | `<meta name="mermaid-theme" content="forest">` → `theme: "forest"` | no meta tag, no `theme`; the key is silently dropped |

I did not re-measure the contrast ratios. They follow from Mermaid's `default` theme colors on a dark background, as the issue states.

## Localization

- `crates/quarto-core/src/transforms/mermaid.rs:333` `mermaid_script_block`: hard-codes `mermaid.initialize({ startOnLoad: false })`. The unit test at `:463` and the expected-output literal at `:767` pin that exact string, so the fix must update them.
- `crates/quarto-core/src/transforms/mermaid.rs:214` `register_mermaid_assets`: the hook for any Mermaid CSS artifact. Read the `css:` key trap in comment c-3bt2tj26 on bd-sehm2rha before keying a stylesheet. The reveal scaffold only collects `css:revealjs:*`.
- `resources/scss/bootstrap/_bootstrap-rules.scss`: the destination for the `$mermaid-*` defaults and the `:root` block. Today its only Mermaid content is diagram alignment (`pre.mermaid > svg`, around line 2055). `resources/scss/revealjs/quarto-revealjs.scss` mirrors that, and the reveal theme likely needs the variables too.
- Model to copy, in Quarto 1:
  - `external-sources/quarto-cli/src/resources/formats/html/_quarto-rules.scss` (the "Mermaid Theming" block, about lines 700–742)
  - `external-sources/quarto-cli/src/resources/formats/html/mermaid/mermaid-init.js` (`defaultCSS` and the `meta[name="mermaid-theme"]` switch, about lines 26–43)
  - schema: `external-sources/quarto-cli/src/resources/schema/document-mermaid.yml`
  - user docs: `external-sources/quarto-web/docs/authoring/_mermaid-theming.qmd`

## Open questions — resolved during triage

1. **Is this already tracked?** Yes. `braid search mermaid` found bd-sehm2rha (open, P2, labels `css,parity`, parent bd-4doe9lvt) and bd-nj25kgbu (open, P3). bd-93ensbpn was an earlier duplicate, already closed against both. Their scopes match the issue's checklist item for item. The specificity check and the dark-mode framing are the only new content.
2. **Does Quarto 1 emit `--mermaid-font-weight`, as the issue says?** No. Q1's `themeCSS` references `var(--mermaid-font-weight)` 11 times, but the `:root` block in `_quarto-rules.scss` never defines it. Only the SCSS variable `$mermaid-font-weight` exists. In Q1 those declarations fall back to inheritance. The port should define `--mermaid-font-weight` and fix the gap, not copy it.
3. **Does Q1 have other quirks the port should not copy?** Yes. `$mermaid-node-fg-color` is declared twice with `!default`, first as `$body-color` and then as `$primary`. The first declaration wins, because `!default` skips a variable that is already set. That gives `$body-color`, even though the second line suggests `$primary` was intended. Decide which one is right before porting. Also, `$mermaid-node-bg-color: rgba($primary, 0.1)` is translucent. That is acceptable here because it paints SVG nodes, not hub-client surfaces (`.claude/rules/hub-client-theme.md` applies only to hub-client).
4. **Is #1095's `[id^=mermaid-] foreignObject` rule in q2's stylesheets?** `grep -rn 'id^=mermaid' resources/ crates/*/src` and `grep -rn foreignObject resources/scss` both return nothing. It probably comes from a downstream theme, not from q2. That is out of scope here, but whoever ports `themeCSS` should re-check #1095's subgraph sizing against the Q1 rules, since `themeCSS` also styles clusters.

## Outcome / recommended next step

**Duplicate of bd-sehm2rha** (with bd-nj25kgbu overlapping).

- I added a comment on bd-sehm2rha that links GH #1296 and records the new evidence: the contrast measurements, the specificity check, the `--mermaid-font-weight` gap and the doubled `$mermaid-node-fg-color` default.
- Suggested follow-up for the user: either merge bd-nj25kgbu into bd-sehm2rha, or reparent them so one strand owns "SCSS + `themeCSS`" and the other owns "`mermaid: theme:` + preview parity". The issue's checklist is a ready-made acceptance list for whichever strand owns the work.
- Reply on GH #1296 with a pointer to bd-sehm2rha, and close the issue when that strand lands.

## Verification commands used

```bash
gh issue view 1296 --repo quarto-dev/q2 --json title,body,author,createdAt,labels,comments
gh issue view 1095 --repo quarto-dev/q2 --json title,state,body
braid search mermaid
braid show bd-sehm2rha; braid show bd-nj25kgbu; braid show bd-93ensbpn
cargo xtask verify --skip-hub-build    # first run failed: quarto2-vscode tsc could not find @types/vscode
                                       # (stale node_modules after #1255); fixed by `fnm exec --using=24 npm install`
q2 render repro.qmd; q2 render repro-theme-option.qmd   # in a scratch copy, see Reproduction
grep -o 'mermaid-font-weight\|mermaid-font-family' external-sources/quarto-cli/src/resources/formats/html/mermaid/mermaid-init.js | sort | uniq -c
```

## Cross-references

- bd-sehm2rha: Mermaid theming CSS, `$mermaid-*` SCSS vars + `:root --mermaid-*` bridge (primary)
- bd-nj25kgbu: Mermaid theming, `--mermaid-*` CSS variables + Mermaid theme metadata
- bd-93ensbpn: closed duplicate of both
- bd-4doe9lvt: epic, port `_quarto-rules.scss` for HTML DOM parity
- bd-c3dtpe36: Mermaid render component for q2-preview (the preview side must read the same variables)
- GH #1095: Mermaid subgraph title sizing
