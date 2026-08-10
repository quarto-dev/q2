# pampa: bare-brace parse error should hint at escaping literal braces (bd-brace-escape-hint-0tmemkyt)

**Date:** 2026-08-09
**Braid:** bd-brace-escape-hint-0tmemkyt (feature, p2, label `diagnostics`)
**Branch:** `main` @ `ec8a35f9` (investigation committed in place; no worktree created)
**Status:** Design settled with user (2026-08-09) — implementation in progress.

## Settled design decisions (user-confirmed 2026-08-09)

1. **Wording:** approved as drafted (see Phase 1 below for the exact strings).
2. **Scope:** narrow — only the two `_language_specifier_token` states
   (bare-paragraph and link-text brace runs). The unclosed `{guid` EOL form
   (`(2613, shortcode_name)`) is deliberately left out: that lookahead
   likely also fires for shortcode typos, and we have overdesigned
   diagnostics before and had to trim them back.
3. **Highlight:** keep the default narrow token highlight; no
   `widen_diagnostic_to_line` enrollment.
4. **Code:** Q-2-41, registered in `crates/quarto-error-catalog/error_catalog.json`
   from the start. The Q-2-36/37/38 catalog+docs lapses are filed as
   **bd-cx1det1y** (chore, discovered-from this strand).
5. **Docs:** write a draft docs page (`docs/errors/markdown/Q-2-41.qmd`)
   per the `docs/errors/README.md` template — do not repeat the Q-2-36
   skip.

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

## Work items

### Phase 0 — TDD baseline (failing-test artifact)

- [x] Confirmed at HEAD (`ec8a35f9`) that both case inputs produce the
  generic fallback "Parse error: unexpected character or token here"
  (prose case via `repro.qmd`, states captured in
  `bare-brace-escape-hint-investigation/error-states.md`). The corpus
  mechanism's failing-test artifact is this asymmetry: before table
  regeneration the case inputs show the fallback; after, they must show
  `[Q-2-41]` (same shape as Q-2-36 Phase 0b).

### Phase 1 — Corpus entry + table regeneration

- [x] Add `crates/pampa/resources/error-corpus/Q-2-41.json`:
  - code `Q-2-41`, title **"Curly braces are reserved for attribute syntax"**
  - message: *"Curly braces are reserved for attribute syntax in Quarto
    markdown. To write literal braces, escape them as `\{...\}`. If you
    meant to attach an attribute, use `.class` / `#id` / `key="value"`
    syntax, e.g. `[text]{.class}`."*
  - cases: `prose` (`the request returns the task {guid} immediately.`)
    and `link-text` (`see [the {guid} link](https://example.com) here.`);
    `captures: []`, no notes (narrow highlight by design).
- [x] Run `./crates/pampa/scripts/build_error_table.ts` (deno, from
  `crates/pampa/`); confirmed `case-files/Q-2-41-{prose,link-text}.qmd`
  generated and exactly two new `_autogen-table.json` entries at
  `(2613, _language_specifier_token)` and `(2589, _language_specifier_token)`
  (diff purely additive: +30 lines, 0 deletions — no state renumbering).
  Duplicate-pair warnings unchanged (pre-existing Q-2-10/Q-2-11/… ones
  only, none for Q-2-41). Stray `deno.lock` created by the deno run was
  removed (deleted deliberately in a258b2ae).
- [x] `cargo run --bin pampa --` on both case files → `[Q-2-41]` with the
  approved message; narrow highlight on the word inside the braces.
  Controls verified: `\{guid\}` → literal text, `[text]{.cls}` → Span,
  `[text]{guid}` → Q-2-41 (either/or wording serves it).
- [x] pampa test suite: **4300/4300 pass, 2 skipped.** Zero snapshot
  changes (matches Q-2-36 experience: corpus snapshot tests glob
  top-level `error-corpus/*.qmd`, which is empty; the `case-files/`
  iterating tests assert diagnostics are produced).

### Phase 2 — Catalog + docs

- [x] Registered Q-2-41 in `crates/quarto-error-catalog/error_catalog.json`
  (subsystem `markdown`, `docs_url`
  `https://quarto.org/docs/errors/markdown/Q-2-41`, `since_version`
  `99.9.9` placeholder, matching Q-2-40's shape).
- [x] Added `docs/errors/markdown/Q-2-41.qmd` per the README template
  (front matter matching the catalog; status `stub`; What this means /
  Why this happens / How to fix, escaped-brace + attribute-syntax fix
  examples; `Q-2-38` cross-ref left as a code span per convention since
  that page doesn't exist yet).
- [x] `cargo run --bin q2 -- render docs/` — **190/190 files rendered**;
  page appears at `_site/errors/markdown/Q-2-41.html` and in the errors
  index listing. The 25 warnings are pre-existing missing-image warnings
  in `guides/authoring/figures.qmd`, unrelated.

### Phase 3 — End-to-end verification

- [x] Single-file: `cargo run --bin q2 -- render repro.qmd` → output
  carries `[Q-2-41] Curly braces are reserved for attribute syntax` with
  the full hint text pointing at the brace run (observed output recorded
  in "Verification output" below).
- [x] Project render (page-drop symptom): fixture website with `index.qmd`
  (good) + `bad.qmd` (bare braces) → `warning: profile-pass skipped
  …/bad.qmd: Error: [Q-2-41] Curly braces are reserved for attribute
  syntax`, `Rendered 1 of 2 files … — 1 error`. The skip warning now
  names the targeted diagnostic instead of the generic parse error.
- [x] `cargo nextest run -p pampa`: 4300/4300. Workspace build + tests +
  hub legs via full `cargo xtask verify` — **all steps passed** (see
  Phase 4).
- [x] Snapshot changes: **zero** `.snap` files added or modified.

### Phase 4 — Commit / wrap-up

- [x] Full `cargo xtask verify` (all 14 steps, including hub-client
  TypeScript + Vite + WASM build and hub tests): **all passed.**
- [x] Per user direction, moved the two commits to branch
  `braid/bd-brace-escape-hint-0tmemkyt`, rebased onto the new
  `origin/main` tip (PR #482 touched no parser/grammar/corpus files, so
  the captured LR states are unaffected), re-verified with
  `cargo xtask verify --skip-hub-build` (green), and reset local `main`
  to `origin/main`.
- [x] Pushed as `feature/bd-brace-escape-hint-0tmemkyt`; **PR #483**:
  https://github.com/quarto-dev/q2/pull/483
- [ ] Close bd-brace-escape-hint-0tmemkyt after the PR merges.
- [ ] bd-cx1det1y (Q-2-36/37/38 backfill) stays open for a separate session.

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

## Verification output (end-to-end, output inspected)

### Single-file render (the strand's verbatim example)

```
$ cargo run --bin q2 -- render <scratch>/repro.qmd
Rendering single file: <scratch>/repro.qmd
warning: profile-pass skipped <scratch>/repro.qmd: Error: [Q-2-41] Curly braces are reserved for attribute syntax
   ╭─[ <scratch>/repro.qmd:1:31 ]
   │
 1 │ the request returns the task {guid} immediately.
   │                               ──┬─
   │                                 ╰─── Curly braces are reserved for attribute syntax in Quarto markdown.
   │                                      To write literal braces, escape them as `\{...\}`. If you meant to
   │                                      attach an attribute, use `.class` / `#id` / `key="value"` syntax,
   │                                      e.g. `[text]{.class}`.
───╯

1 error
```

(ANSI + hyperlink escapes stripped; the message renders as one long span
label in the terminal.)

### Project render (page-drop form)

```
$ cargo run --bin q2 -- render <scratch>/brace-proj      # index.qmd good, bad.qmd has bare braces
warning: profile-pass skipped <scratch>/brace-proj/bad.qmd: Error: [Q-2-41] Curly braces are reserved for attribute syntax
Rendered 1 of 2 files to <scratch>/brace-proj/_site — 1 error
```

### Controls (pampa)

```
$ printf 'escaped \{guid\} braces.\n' | cargo run --bin pampa --
[ Para [Str "escaped", Space, Str "{guid}", Space, Str "braces."] ]      # escaped braces → literal text
$ printf 'attr [text]{.cls} span.\n' | cargo run --bin pampa --
[ Para [… Span ( "" , ["cls"] , [] ) [Str "text"] …] ]                   # valid attribute → clean parse
$ printf 'typo [text]{guid} span.\n' | cargo run --bin pampa --
Error: [Q-2-41] Curly braces are reserved for attribute syntax           # attr typo → same either/or hint
```

### Docs render

`cargo run --bin q2 -- render docs/` → 190/190 files;
`_site/errors/markdown/Q-2-41.html` exists and the errors index lists
"Curly braces are reserved for attribute syntax".
