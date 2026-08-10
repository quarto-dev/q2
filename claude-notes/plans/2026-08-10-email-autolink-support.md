# Bare email autolinks `<user@example.com>` parsed as raw HTML (bd-email-autolink-dropped-2jj38iiv)

**Date:** 2026-08-10
**Braid:** bd-email-autolink-dropped-2jj38iiv (bug, P2, labels: pampa, parity)
**Checkout:** invoked on `main` @ `46cacc88` (no new worktree/branch created; user decides where implementation lands)
**Status:** Design settled 2026-08-10 (user answered all questions); implementation in progress.

## Triage verdict

**Ready to design.** The strand (filed today, by Carlos, with a ruling already
recorded: "fix belongs in q2") contains an accurate root-cause analysis — every
code pointer checks out at HEAD, the bug reproduces exactly as described, and
the suggested fix shape fits the existing scanner/pampa division of labor. Only
a handful of behavior-policy questions need answers before implementation.

## Issue context

CommonMark email autolinks (`Contact <sales@example.com> now.`) should render
as `<a href="mailto:sales@example.com">sales@example.com</a>`. q2 0.15.0 (and
HEAD) instead lexes the construct as an HTML element and emits it as raw HTML —
browsers swallow the unknown tag and **the address is invisible in the rendered
page**, with only a generic Q-2-9 warning as signal (drowned out in real docs:
Posit Connect docs emit 2833 legitimate Q-2-9s). Every mainstream Markdown
implementation supports this production (pandoc all readers, cmark, comrak,
markdown-it, goldmark, pulldown-cmark, micromark, kramdown, Python-Markdown);
the sole exception (MDX) fails loudly rather than silently dropping content.

Real-world hit: Connect docs `admin/user-management/index.md` had to regress
`<sales@posit.co>` to `<mailto:sales@posit.co>` (visible text now shows the
`mailto:` prefix). Origin strand in the connect-docs skein:
br-email-autolink-dropped-287gi3pl.

## Dependency graph

**Empty in this skein** — no deps, no dependents (`braid dep tree`/`dep list`
show only the strand itself). Context instead lives in:

- **Origin (external):** br-email-autolink-dropped-287gi3pl in the
  connect-docs porting skein; the docs side will revert to the bare form once
  this ships.
- **Related by code area (found by search, not linked):** bd-ly83qewg
  (closed) — the most recent change to the same scanner function
  (`parse_open_angle_brace`), plan at
  `claude-notes/plans/2026-08-07-angle-bracket-inner-whitespace.md`. Good
  template for how to test/land scanner changes. The strand description
  already confirms bd-ly83qewg did *not* fix this bug.

## What the code looks like today

Reproduced at HEAD (main @ 46cacc88); full transcript + pandoc reference
output in `claude-notes/plans/email-autolink-investigation/notes.md`; repro
fixture copied to `claude-notes/plans/email-autolink-investigation/repro.qmd`.

- `crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c:1875-1894`
  (`parse_open_angle_brace`): AUTOLINK is emitted only when a "url-like
  character" (`:` or `%`) was seen before `>` (`had_url_like_character`).
  Bare emails have neither → `HTML_ELEMENT`.
- `crates/pampa/src/pandoc/treesitter.rs:850` → `process_uri_autolink`
  (`treesitter_utils/uri_autolink.rs`): emits `Link` with class `uri`, link
  text = raw content. No email awareness.
- `crates/pampa/src/pandoc/treesitter.rs:1537` (`html_element` arm): where
  Q-2-9 raw-HTML conversion happens today — the behavior to preserve for
  angle-bracket content that is neither a URI nor a valid email.
- Pandoc reference: `markdown` reader emits
  `Link ("",["email"],[]) [Str "sales@example.com"] ("mailto:sales@example.com","")`;
  `commonmark` reader is identical but without the `email` class. Q1 = the
  `markdown` reader behavior.
- Over-approximation safety: real HTML open tags with attributes always
  contain whitespace (already disqualifies autolink); tag names cannot
  contain `@`. So gating AUTOLINK on "saw `@`" only captures strings that
  were never valid HTML anyway.

## Proposed phases (draft)

Skeleton only — actual phase contents wait on the design discussion.

- **Phase 0 — Test plan (TDD, failing tests first).**
  - tree-sitter corpus tests (`test/corpus/`): bare email → `(autolink)`;
    non-email `@`-content (e.g. `<foo@@bar>`) → expected token per Q2 below;
    control cases (`<http://...>`, `<mailto:...>`, genuine HTML) unchanged.
  - pampa integration tests: native/JSON AST shape (`Link` with
    `mailto:` target, bare address text, class per Q1 below); HTML writer
    output; the CommonMark spec's email-autolink examples (valid + invalid
    sets, incl. `<a@b>`, `<foo+special@Bar.baz-bar0.com>`, backslash-escape
    rejection).
  - End-to-end: `cargo run --bin q2 -- render` on the repro fixture, inspect
    emitted HTML.
- **Phase 1 — Scanner over-approximation** (`scanner.c`,
  `parse_open_angle_brace`): track `saw_at` alongside
  `had_url_like_character`; emit AUTOLINK when either holds (still requires
  no whitespace, no leading `/`). Rebuild grammar
  (`tree-sitter generate; tree-sitter build`), run `tree-sitter test`.
- **Phase 2 — Precise classification in pampa** (`process_uri_autolink`):
  if content has no scheme and matches the CommonMark email production →
  `Link` to `mailto:<addr>` with bare address as text (+ class per Q1);
  else if it looks like today's URI autolink → current behavior; else →
  fallback per Q2 (likely: replicate the html_element raw-HTML + Q-2-9 path,
  or literal text per CommonMark).
- **Phase 3 — Verification & docs.** Full `cargo nextest run --workspace`,
  `cargo xtask verify` (WASM leg — pampa is in hub-client's closure), re-render
  repro end-to-end, note in docs/ if user-facing syntax docs mention autolinks.

## Design decisions (user-confirmed 2026-08-10)

1. **Link class:** `email` (Q1/pandoc-`markdown` parity; consistent with the
   existing `uri` class on URI autolinks).
2. **Invalid `@`-content fallback:** conservative — preserve today's raw HTML
   + Q-2-9 behavior. (CommonMark-correct literal text can be filed separately
   later.)
3. **Scanner precision:** over-approximate in C (`saw_at`), precise
   CommonMark email-production validation in Rust.
4. **Scope:** bracketed form only. GFM-style bare `sales@example.com`
   autolinking is out of scope.
5. **Diagnostics:** minimal fix; no new email-specific diagnostic for the
   fallback path (may be considered later).

Classification order in pampa (consequence of 1–3): a token that lexed as
AUTOLINK is (a) a valid CommonMark email autolink → `mailto:` Link with class
`email` and bare address text; else (b) contains `:` or `%` (i.e. would have
lexed as AUTOLINK before this change) → existing URI-autolink behavior,
unchanged; else (c) newly-captured invalid content (e.g. `<foo@@bar>`) → raw
HTML + Q-2-9, byte-identical to its pre-change html_element treatment. Note
(a) before (b) means `<a%b@c.com>` (valid email whose local part contains
`%`) upgrades from a schemeless `uri` link to a proper `mailto:` email link —
that matches pandoc and is part of the fix.

## Work items

- [x] Phase 0 — failing tests first: 3 corpus tests in
      `test/corpus/link.txt` (all failed pre-fix) + 8 integration tests in
      `crates/pampa/tests/integration/test_email_autolink.rs` (4 failed
      pre-fix; the uri-unchanged and raw-HTML-fallback tests passed pre-fix
      by design — they are regression guards)
- [x] Phase 1 — scanner `had_at_sign` over-approximation in
      `parse_open_angle_brace` (scanner.c); grammar rebuilt, all 557
      `tree-sitter test` cases green (no parser.c diff — grammar.js
      untouched)
- [x] Phase 2 — classification in `process_uri_autolink` (email →
      `mailto:` + class `email`; `:`/`%` → existing uri behavior;
      else raw-HTML fallback reproducing the html_element arm, Q-2-9
      included). All 4332 pampa tests pass.
- [x] Phase 3 — 11469/11469 workspace tests pass; end-to-end verified
      (`cargo run --bin q2 -- render` on the repro fixture; output HTML
      inspected):
      - `<sales@example.com>` →
        `<a href="mailto:sales@example.com" class="email">sales@example.com</a>`
      - `<mailto:sales@example.com>` → unchanged
        `<a href="mailto:sales@example.com" class="uri">mailto:sales@example.com</a>`
      Full `cargo xtask verify` (WASM leg included) run before commit.

## Risks / tradeoffs (draft)

- Scanner changes are the highest-risk part of the tree (bd-ly83qewg's plan
  documents the workflow); the over-approximation argument (no valid HTML tag
  contains `@` without whitespace) keeps the blast radius small, but corpus +
  full workspace tests are the real safety net.
- `process_uri_autolink` currently `panic!`s on content not wrapped in
  `<...>`; extending its responsibilities means the new classification logic
  must be total (no new panic paths).
- Any change to what Q-2-9 fires on will shift diagnostic counts in large
  ports (the Connect docs' 2833 Q-2-9s) — expected and desirable here (bare
  emails stop warning entirely), worth a release-note line.
- pampa is in the WASM closure → full `cargo xtask verify` (not
  `--skip-hub-build`) before landing.
