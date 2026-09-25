# Fix: Monaco editor auto-closes backtick, inserting a doubled `` ` ``

**Strand:** bd-w1s38lbe

## Overview

A quarto-hub.com user reports an annoyance in the **Monaco** editor view
(not the tiptap rich-text editor) of hub-client:

> If I have existing text with a word that I want to wrap in inline backticks,
> I'll go to the front of the word and add a backtick, then when I go to the
> end of the word and add a backtick, it adds two of them, and I have to
> delete one.

### Root cause

`hub-client/src/components/quartoTheme.ts` registers the `qmd` language with a
`LanguageConfiguration` whose `autoClosingPairs` includes the backtick pair
(`quartoTheme.ts:171`):

```ts
const qmdLanguageConfiguration: Monaco.languages.LanguageConfiguration = {
  // ...
  autoClosingPairs: [
    { open: '[', close: ']' },
    { open: '(', close: ')' },
    { open: '{', close: '}' },
    { open: '`', close: '`' },   // <-- this is the problem
    { open: '"', close: '"' },
    { open: '$', close: '$' },
  ],
  surroundingPairs: [
    { open: '[', close: ']' },
    { open: '(', close: ')' },
    { open: '`', close: '`' },   // <-- keep this one
    { open: '*', close: '*' },
    { open: '_', close: '_' },
  ],
};
```

This config is applied in `registerQmdLanguage()`
(`quartoTheme.ts:201`, via `monaco.languages.setLanguageConfiguration`),
which is called from `Editor.tsx`\'s `beforeMount` handler
(`Editor.tsx:19`, `Editor.tsx:546-551`).

### Why the exact symptom appears

Monaco only fires an auto-close when the character *after* the cursor is
whitespace, end-of-line, or a closing bracket — never when it is a word
character. So the user's two keystrokes behave differently:

- **Front of word** — cursor at `|word`. The next char is `w` (a word char),
  so Monaco does **not** auto-close. One backtick is inserted: `` `word``.
- **End of word** — cursor at `` `word|`` (EOL after). The next char is the
  line end, so Monaco **does** auto-close, inserting the pair: `` `word``|``.
  Two backticks appear; the user deletes one.

Removing the backtick from `autoClosingPairs` makes the end-of-word keystroke
insert a single backtick, matching the user's expectation. Keeping it in
`surroundingPairs` preserves the genuinely-useful behavior of wrapping a
*selection* by typing `` ` `` (Monaco distinguishes the two lists).

### Scope decision

- Remove **only** the backtick entry from `autoClosingPairs`. Leave `[`, `(`,
  `{`, `"`, `$` auto-closing as they are (no report against them, and `$` /
  brackets are far less likely to be typed singly in prose-with-math).
- Leave the backtick in `surroundingPairs` untouched.
- This is the minimal, targeted fix. Do **not** rework the broader
  auto-closing strategy in this strand.

## Work Items (TDD order)

### Phase 1 — Test first
- [x] Export `qmdLanguageConfiguration` from `quartoTheme.ts` (currently a
      module-private const) so it can be asserted against. (It is the
      configuration object, not behavior — exporting it is the mechanical seam.)
- [x] Add a test to `hub-client/src/components/quartoTheme.test.ts`:
  - [x] `autoClosingPairs` does **not** contain a `` { open: '`', … } `` entry
        (regression guard for this bug).
  - [x] `surroundingPairs` **still** contains the backtick entry (so the fix
        doesn't over-correct and lose wrap-selection).
- [x] Run the test, confirm the first assertion **fails** against current code
      (backtick still in `autoClosingPairs`). ✅ Confirmed red.

### Phase 2 — Fix
- [x] Remove `{ open: '`', close: '`' }` from `autoClosingPairs` in
      `quartoTheme.ts` (line \~171). Add a short comment explaining why backtick
      is intentionally absent from auto-closing but present in surrounding.
- [x] Run the test, confirm it now passes. ✅ 5/5 green.

### Phase 3 — Verify
- [x] `cd hub-client && npm run test:ci` — full hub-client test suite green.
      ✅ 18 files / 121 tests pass.
- [x] `cd hub-client && npm run build:all` — production build succeeds (stricter
      than `tsc --noEmit`; required for hub-client changes per CLAUDE.md). ✅
- [x] **End-to-end, in a browser**: open a `.qmd` in the Monaco view, place the
      cursor at the end of a word, type `` ` ``, and confirm only **one**
      backtick is inserted. Also confirm: selecting a word and typing `` ` ``
      still wraps it (surroundingPairs intact). ✅ User tested against a local
      hub-client and confirmed the bug is fixed (2026-06-24).
- [ ] Update `hub-client/changelog.md` per the two-commit workflow in CLAUDE.md
      (hub-client changes require a changelog entry referencing the commit hash).

## Notes / caveats

- The config-level test is a **mechanical regression guard**, not proof of the
  runtime behavior. Monaco's auto-close logic runs inside the Monaco runtime;
  the test only asserts the configuration we feed it. The browser check in
  Phase 3 is what actually verifies the user-visible behavior — do not skip it.
- Files involved:
  - `hub-client/src/components/quartoTheme.ts` (config + `registerQmdLanguage`)
  - `hub-client/src/components/quartoTheme.test.ts` (tests)
  - `hub-client/src/components/Editor.tsx` (calls `registerQmdLanguage`; no
    change expected here)
  - `hub-client/changelog.md` (entry)
