# Table listings ignore fields/field-display-names (bd-listing-table-fields-peg1w3b3)

**Date:** 2026-08-09
**Braid:** bd-listing-table-fields-peg1w3b3 (bug, P1, label `listings`)
**Branch:** `main` (investigated in place; no worktree created)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design.** The symptom reproduces at HEAD, the root cause is unambiguous (static built-in table templates cannot express a dynamic column set), the config and binding layers already carry everything the fix needs, and Q1's reference implementation pins the target semantics. The remaining work is choosing the mechanism (pre-rendered binding keys vs. dynamically generated template source) and pinning per-field cell rendering rules.

## Issue context

Filed 2026-08-09 by Carlos (same day as this investigation — no staleness risk). A `type: table` listing with `fields: [title]` still renders three columns (Title | Date | Author); `field-display-names:` is ignored for headers; items missing `date`/`author` produce per-item "Undefined variable" doctemplate diagnostics surfaced as one Q-12-10 warning per listing.

Real-world hit: Posit Connect docs `how-to/index.md` (`type: table, fields: [title], field-display-names: {title: "How To"}`) renders three columns (two empty) + 8 diagnostics vs. Q1's single "How To" column.

## Dependency graph

**Empty.** `braid dep tree` / `dep list` show no edges in this skein. The origin strand (`br-listing-table-fields-hes1dsib`) lives in the *connect-docs porting* skein, not this one — the description carries its context forward. No incoming `blocks` pressure here, but the P1 + real-world-docs hit sets the urgency.

## What the code looks like today

Reproduced at HEAD (`main` @ 4bb32844) with the repro under
`claude-notes/plans/listing-table-fields-investigation/repro/` (copied from the
connect-docs repro, build artifacts stripped, extended with
`field-display-names: {title: "How To"}`):

```
$ cargo run --bin q2 -- render .   # in the repro dir
Warning [Q-12-10]: Listing `guides` doctemplate produced 4 diagnostic(s); first: Undefined variable: date
```

Rendered `_site/index.html` has `<th>Title</th><th>Date</th><th>Author</th>` — `fields:` and `field-display-names:` both ignored. Output was inspected directly.

Where each piece lives:

- **Config already parses everything.** `crates/quarto-core/src/project/listing/config.rs` parses `fields` (line 466) and `field-display-names` (line 469, into `Listing::field_display_names: BTreeMap<String, String>`, with Q-12-5 on non-string values). Table-type default fields are `[date, title, author]` (line ~887).
- **The binding carries `fields` but nothing table-shaped.** `binding.rs::build_listing_map` exposes `listing.fields` as a list and `build_item_map` computes per-item `show.*` flags from `listing.fields`; optional item fields are *omitted* when absent (deliberately, so `$if(field)$` works) — which is exactly why the hardcoded `$date$` reference produces "Undefined variable" for date-less items.
- **The templates are static.** `templates/listing-table.template` hardcodes the `| Title | Date | Author |` header; `templates/item-table.template` hardcodes the three cells. Both are `include_str!`-embedded and served via `MemoryResolver` (`templates.rs`), so L8 custom templates can shadow them by name.
- **The template language can't fix this alone.** doctemplate is Pandoc-style/logic-less: there is no dynamic map indexing (`item[field]`), so no static template can render an author-chosen column set. The dynamism has to come from Rust — either in the binding or in generated template source.
- **Render path:** `transforms/listing_render.rs::render_one` → `render_builtin` compiles the embedded source, renders against the binding, re-parses the output as qmd markdown, splices into the AST. Template output being *markdown* (re-parsed) means pre-rendered markdown strings in the binding work naturally.

### Q1 reference semantics (external-sources/quarto-cli)

`src/resources/projects/website/listing/listing-table.ejs.md` loops `listing.fields` for both `<th>` headers and `<td>` cells:

- **Headers:** `utilities.fieldName(field)` = merged display-name map — built-in localized defaults (`_language.yml`: Title, Date, Author, Description, File Name, Modified, Subtitle, Reading Time, Word Count, Categories) overlaid with the author's `field-display-names`; unknown fields fall back to the **raw field name** (not title-case).
- **Cells:** `outputValue` special-cases `image` (img/placeholder), joins array values with `", "`, supports dotted-path field access, and wraps linked fields via `outputLink` (default `field-links` includes `title`; missing values render `&nbsp;`).
- Q1 emits a raw **HTML** table (with sort-ui anchor headers, `table-hover` row onclick, `metadataAttrs` per row); Q2's current template emits a markdown pipe table.

## Proposed phases (draft)

Skeleton only — actual phase contents wait on the design discussion.

- **Phase 0 — Test plan (TDD).** Failing tests first: (a) end-to-end project render fixture (`type: table, fields: [title], field-display-names`) asserting a single "How To" column and zero Q-12-10 warnings; (b) binding unit tests for whatever new keys the design settles on; (c) missing-value tolerance (item without `date` in a `fields: [title, date]` listing → empty cell, no diagnostic); (d) default-fields table still renders Title | Date | Author unchanged (snapshot parity).
- **Phase 1 — Binding/mechanism.** Compute the dynamic column set from `listing.fields` + `field_display_names` (mechanism per design question 1).
- **Phase 2 — Templates.** Rewrite `listing-table.template` / `item-table.template` to consume the new mechanism; confirm L8 shadowing story still holds.
- **Phase 3 — End-to-end verification + docs.** Re-run the committed repro through `q2 render`, inspect output, update `docs/` listing documentation if it describes table columns.

## Open design questions for the user

1. **Mechanism.** Two candidates:
   - **(A) Pre-rendered binding keys** — the binding computes e.g. `listing.table-header` (header + separator rows) and a per-item `table-row` (or per-item `table-cells`) markdown string; the templates shrink to interpolating those. Keeps templates static, is purely *additive* to the L8 binding contract (non-breaking per binding.rs's contract note), and follows the existing precedent of pre-rendered helpers (`image-html`, `category-html`, `metadata-attrs`).
   - **(B) Dynamically generated template source** — `top_level_template_source` becomes listing-aware and emits per-listing template text. More invasive: it breaks the "templates are readable canonical reference files" property and complicates L8 partial shadowing of `item-table`.
   I lean (A); confirm?
2. **Header fallback for fields without a display name.** Q1 parity = port the built-in default display-name map (Title, Date, Author, Reading Time, …) and fall back to the **raw field name** for unknown/extra fields. The strand suggests title-case fallback instead. Which do we want — Q1 parity, or title-case as a deliberate Q2 improvement?
3. **Per-field cell rendering rules.** Proposed: `title` → `[$title$]($path$){.no-external}` link (as today); `image` → `image-html`; `date`/`date-modified` → pre-formatted strings (already in the binding); `categories`/`authors` → comma-joined; unknown fields → `extra.*` lookup; missing value → empty cell. Q1 also supports dotted-path fields (`a.b`) and a `field-links` option (which fields become links) — in scope now, or deferred to a follow-up strand?
4. **Cell escaping.** A markdown pipe table breaks on cell values containing `|` (or block content). Escape `|` as `\|` and accept the limitation, or switch the table listing to raw-HTML emission like Q1 (bigger change, but sidesteps the whole class)?
5. **Sort-ui/hover parity.** Q1 table headers are sortable anchors and `table-hover` adds row onclick. The current Q2 template has neither, so this bug's scope could stay "columns only" with sort-ui parity as a separate filed strand. Agree?

## Risks / tradeoffs (draft)

- The binding is the **load-bearing public contract for L8 custom templates** (binding.rs header comment): adding keys is safe, but if we *change* what `item-table` receives or how the built-in table templates are structured, custom templates that shadow `item-table`/`listing-table` by name will see the new call shape. Worth a call-out in the commit either way.
- Option (B) would interact with `builtins_resolver()`'s six static names and the `Custom`-type fallback path; option (A) leaves both untouched.
- Snapshot churn: default table listings (no `fields:` override) must render byte-identically or the change needs snapshot review per the CLAUDE.md snapshot policy.
