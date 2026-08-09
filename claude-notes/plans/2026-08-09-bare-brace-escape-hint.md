# pampa: bare-brace parse error should hint at escaping literal braces (bd-brace-escape-hint-0tmemkyt)

**Date:** 2026-08-09
**Braid:** bd-brace-escape-hint-0tmemkyt (feature, p2, label `diagnostics`)
**Branch:** `main` @ `ec8a35f9` (investigation committed in place; no worktree created)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design.** The mechanism is proven (Q-2-36 "path B" pure-corpus
precedent), the target `(state, sym)` pairs are captured and verified
unclaimed in the autogen table, and the repro is confirmed at HEAD. What
remains is wording, case scope, highlight treatment, and code/catalog
registration — all user-facing decisions.

## Issue context

Filed 2026-08-09 by Carlos. A bare brace run in prose — e.g. `the request
returns the task {guid} immediately.` — is a fatal parse error in q2,
reported only with the generic fallback "Parse error: unexpected character
or token here". Single-file render produces no output; project render
silently drops the page (`warning: profile-pass skipped <file>`).

The brace reservation is **by design and not in question**: escaped braces
`\{...\}` parse cleanly in q2 and render identically under Pandoc, so
escaping is the correct Q1-compatible source fix. The strand asks only for
a **targeted diagnostic** hinting at escaping when the fallback fires at a
brace run.

Real-world driver: porting Q1 projects — REST API docs write path
parameters as `{name}` constantly (the generated Posit Connect API
reference hit this dozens of times across ~160 endpoints). Origin strand in
the connect-docs skein: `br-brace-escape-hint-z8vy6sis`; external repro at
`~/repos/github/cscheid/q2-connect-docs/llms-info/repros/bare-braces-parse-error/`
(README states expected vs. actual; a copy of the repro's behavior facts is
in this plan's investigation dir).

## Dependency graph

**Empty** — no edges in this skein (`braid dep tree` / `dep list` show
nothing). The why-filed context lives in the origin strand in the
connect-docs project skein (`br-brace-escape-hint-z8vy6sis`) and is fully
restated in this strand's description, so nothing is lost.

## What the code looks like today

All paths in the strand description check out at HEAD:

- Generic fallback: `crates/quarto-parse-errors/src/error_generation.rs:243-249`
  (`DiagnosticMessageBuilder::error("Parse error").problem("unexpected character or token here")`).
- Reproduced at HEAD (`ec8a35f9`): `cargo run --bin pampa -- repro.qmd`
  emits the generic fallback with the highlight on the word *inside* the
  braces. Fixture: `claude-notes/plans/bare-brace-escape-hint-investigation/repro.qmd`.
- Error-state capture (full table + collision check in
  `bare-brace-escape-hint-investigation/error-states.md`):
  - Prose `{guid}` → `(2613, _language_specifier_token)` — **unclaimed** in
    `_autogen-table.json`.
  - Link-text `{guid}` → `(2589, _language_specifier_token)` — **unclaimed**.
  - `[text]{guid}` (attribute-intent typo) → **same** `(2613, _language_specifier_token)`;
    one mapping covers both readings, so the message must serve both.
  - Unclosed `trailing {guid` at EOL → `(2613, shortcode_name)` — different
    lookahead; separate scope decision.
- Mechanism precedent: Q-2-36 (`claude-notes/plans/2026-05-14-q-2-36-knitr-style-chunk-options.md`,
  commit `666f8b7e`), path B — add a corpus JSON under
  `crates/pampa/resources/error-corpus/`, run
  `./crates/pampa/scripts/build_error_table.ts` (deno) to regenerate
  `case-files/*.qmd` + `_autogen-table.json`, and the corpus snapshot tests
  in pampa glob the new case files automatically. **No grammar, scanner, or
  error_generation.rs change needed.**
- Error code numbering: highest existing is Q-2-40 → **Q-2-41** is next
  free. Note: corpus codes and `crates/quarto-error-catalog/error_catalog.json`
  are not fully synced today (Q-2-36 has a corpus entry but no catalog
  entry; Q-2-40 has a catalog entry) — registration is a design question
  below.

## Proposed phases (draft)

Skeleton only — contents wait on the design discussion.

- **Phase 0 — Test plan (TDD).** Add `Q-2-41.json` corpus entry with the
  agreed cases; regenerate the table; confirm the new case-file snapshots
  under `crates/pampa/snapshots/error-corpus/` show the targeted message
  (before regeneration, running the case inputs shows the generic fallback
  — that asymmetry is the failing-test artifact, same shape as Q-2-36
  Phase 0b).
- **Phase 1 — Corpus + table regeneration.** Land the corpus entry, the
  regenerated `_autogen-table.json`, and generated `case-files/Q-2-41-*.qmd`.
- **Phase 2 — Highlight treatment (if any).** Depending on design answer:
  nothing (keep narrow token highlight), or enroll Q-2-41 in
  `widen_diagnostic_to_line` (`crates/pampa/src/readers/qmd_error_messages.rs`),
  or new brace-run widening (probably over-engineering — flag if we get here).
- **Phase 3 — End-to-end verification.** `cargo run --bin pampa --` on the
  fixtures + `cargo run --bin q2 -- render` on a copy of the repro
  (single-file and project-render forms, since the project form is where
  the page-drop symptom shows); full workspace nextest; snapshot-change
  report per CLAUDE.md.
- **Phase 4 — Docs + catalog (per design answers).** Possibly register
  Q-2-41 in `quarto-error-catalog`; possibly a line in docs/ about escaping
  literal braces.

## Open design questions for the user

1. **Message wording.** `(2613, _language_specifier_token)` fires for both
   prose braces (`task {guid}`) and attribute-intent typos
   (`[text]{guid}`), so the message should serve both readers, like
   Q-2-36's either/or wording. Draft:
   *"Curly braces are reserved for attribute syntax in Quarto markdown.
   To write literal braces, escape them as `\{...\}`. If you meant to
   attach an attribute, use `.class` / `#id` / `key="value"` syntax, e.g.
   `[text]{.class}`."* — Adjust title/message/hint split? (Corpus entries
   render the `message` as the inline span label, as Q-2-36 does.)
2. **Case scope.** Definitely: bare-paragraph brace run + brace run inside
   link text (the two states from the strand). Also include the unclosed
   `trailing {guid` EOL form, which is a *different* pair
   `(2613, shortcode_name)`? Risk: that lookahead may also fire for broken
   shortcode syntax (`{{< ... >}}` typos) — mapping it to a brace-escape
   message could mislead shortcode authors. My lean: leave it out, note it
   as a possible follow-up.
3. **Highlight treatment.** Today the fallback highlights the word inside
   the braces (`guid`), excluding the braces themselves. Options:
   (a) accept the default narrow highlight — the message text carries the
   meaning; (b) enroll Q-2-41 in `widen_diagnostic_to_line` like
   Q-2-35/Q-2-36 — but prose lines can be long, and line-wide underlines
   on a paragraph feel worse than on a code-block header. My lean: (a).
4. **Error code + catalog registration.** Use Q-2-41? And should it be
   registered in `crates/quarto-error-catalog/error_catalog.json` (Q-2-40
   is registered; Q-2-36 never was)? If the catalog is meant to be the
   authoritative code registry, this is also a chance to backfill Q-2-36 —
   or file that as a separate chore strand.
5. **Docs.** Q-2-36 deliberately added no docs page. Is there an existing
   docs/ page on qmd syntax differences where "braces are reserved; escape
   literal braces as `\{...\}`" belongs, or skip docs entirely and let the
   diagnostic carry it?

## Risks / tradeoffs (draft)

- **State-number churn.** Corpus mappings key on LR state numbers; any
  grammar regeneration renumbers states and the build script re-derives
  them from the case files. That's the established maintenance model
  (all 37 existing codes live with it) — no new risk, just worth knowing
  the mapping is example-derived, not hand-pinned.
- **Overbreadth of the mapping.** Any input that lands in
  `(2613, _language_specifier_token)` gets the brace-escape message. The
  captures suggest this state is specifically "just consumed `{` in inline
  context, content isn't valid attribute syntax", and the either/or wording
  covers the plausible intents. If a colliding non-brace input surfaces
  later, the corpus snapshot tests would show it.
- **The page-drop symptom is out of scope.** The silent
  `profile-pass skipped` project-render behavior (page dropped from site on
  parse error) is pre-existing reader/render architecture, not part of this
  strand. If the user wants that loudness improved, it should be its own
  strand.
