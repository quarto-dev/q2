# Login screen carries a "use test data, not real data" disclaimer

**Strand:** bd-m6u9qu3u · **Scope:** `hub-client/` · **Related:** bd-g0uyp2v1 (landing page intro)

## Overview

The landing / sign-in screen (`hub-client/src/components/auth/LoginScreen.tsx`)
is the one page every visitor to quarto-hub.com sees before they can enter
any data. It should carry a disclaimer, roughly this markdown:

```
### Disclaimer

**Use test data, not real data.**

This is a live demo of a product still in development. Please don't enter
anything sensitive, confidential, or proprietary — no customer data,
credentials, internal business info, or anyone else's personal information.
If you wouldn't post it on a public forum, don't put it here.

**Anything you enter is not protected, and we won't return it.**

Data you share during this demo may be logged, cached, or otherwise retained
by Posit and any third-party services the demo relies on. We don't guarantee
confidentiality, security, or deletion, and we can't retrieve or return data
you enter once the demo session ends. Enter information only if you're
comfortable with that.
```

The component today renders flat strings from `strings.ts` into `<p>`
elements; it has no way to express a heading or bold lead sentences.

## What the changelog setup is, and why it does not transfer

`AboutTab.tsx` shows `changelog.md` / `resources/more-info.md` by importing
the markdown with Vite's `?raw`, rendering it through the WASM pipeline
(`renderContentToHtml` from `@quarto/preview-runtime`) into a complete HTML
document, hand-injecting a theme stylesheet (`utils/changelogDoc.ts`
duplicates the theme tokens because the iframe cannot see them), and
displaying that in a sandboxed `<iframe srcDoc>` inside a modal.

Three reasons not to borrow it for the login screen:

1. **The WASM is not loaded yet.** `wasmStatus` is owned by `Editor.tsx`
   and `PreviewRouter.tsx`, both of which mount only after sign-in.
   `LoginScreen` is rendered by `App` whenever auth is enabled and absent,
   before any project or editor exists. Rendering the disclaimer this way
   would mean loading the multi-megabyte WASM bundle on the cold landing
   page for four paragraphs of text (and showing "Loading…" until it did).
2. **An iframe inside the card is the wrong container.** It has no natural
   height, gets its own scrollbar, needs the duplicated-token stylesheet
   from `changelogDoc.ts` to look themed (the GH #624 bug class), and
   cannot participate in the card's typography or the e2e geometry
   contract in `e2e/landing-card.harness.spec.ts`.
3. **hub-client has no markdown-to-DOM library** (no `react-markdown`,
   `marked`, `remark`). Adding one, plus a sanitizer, for one static block
   is disproportionate, and `dangerouslySetInnerHTML` is confined to the
   two render-internal files today; it should stay there.

## Approach: structure in strings, plain JSX in the component

The disclaimer's formatting is entirely fixed: one heading, two
`<strong>` lead sentences, two body paragraphs. Express that *structure*
in `strings.ts` and render it with ordinary elements, exactly as the
component already does for the two-line tagline (`taglineLead` /
`taglineFollow`). No parser, no innerHTML, nothing to sanitize, and the
copy stays in the single source of truth `strings.ts` requires.

```ts
// strings.ts, inside `landing`
disclaimer: {
  heading: 'Disclaimer',
  useTestData: {
    lead: 'Use test data, not real data.',
    body:
      'This is a live demo of a product still in development. ' +
      'Please don’t enter anything sensitive, confidential, or proprietary — ' +
      'no customer data, credentials, internal business info, or anyone ' +
      'else’s personal information. If you wouldn’t post it on a public ' +
      'forum, don’t put it here.',
  },
  notProtected: {
    lead: 'Anything you enter is not protected, and we won’t return it.',
    body:
      'Data you share during this demo may be logged, cached, or otherwise ' +
      'retained by Posit and any third-party services the demo relies on. ' +
      'We don’t guarantee confidentiality, security, or deletion, and we ' +
      'can’t retrieve or return data you enter once the demo session ends. ' +
      'Enter information only if you’re comfortable with that.',
  },
},
```

```tsx
// LoginScreen.tsx
<section className="ls-disclaimer" aria-labelledby="ls-disclaimer-heading">
  <h2 id="ls-disclaimer-heading" className="ls-disclaimer-heading">
    {landing.disclaimer.heading}
  </h2>
  {[landing.disclaimer.useTestData, landing.disclaimer.notProtected].map((item) => (
    <p key={item.lead}>
      <strong>{item.lead}</strong> {item.body}
    </p>
  ))}
</section>
```

(Whether each lead is its own paragraph or runs into the body paragraph is
a layout choice; see Decisions §3.)

## Decisions (all confirmed by Carlos, 2026-09-15)

1. **Placement: above the sign-in button, directly after the invite-only
   footnote.** The component's own rule (comment in `LoginScreen.tsx`) is
   "everything that qualifies the offer sits above the button, so nothing
   competes with it for last word", and `LoginScreen.test.tsx` pins
   "nothing below the button". The disclaimer qualifies the offer more
   than anything else on the page, so it belongs above. The cost is a
   taller card: the CTA moves roughly 180–220px further down at 520px
   width. The alternative (below the button, as a footer) keeps the CTA
   high but would need the "nothing below the button" test rewritten and
   reads as fine print rather than a condition of use. **Confirmed: above.**
2. **Always visible, not a `<details>` disclosure.** Collapsing it would
   make the card compact, but a notice the visitor must see before
   entering data should not require a click. **Confirmed: visible.**
3. **Layout of the leads.** Render each bold lead as the first run of its
   body paragraph (`<p><strong>Lead.</strong> Body…</p>`), not as its own
   paragraph. The markdown draft separates them, but at the card's
   footnote type size two extra paragraph gaps add height for no gain in
   scannability; the bold run already anchors the eye. **Confirmed: leads run into their body paragraph.**
4. **Type size.** The invite-only footnote sits at `--text-2xs` (10px),
   which is too small for ~120 words of prose. Set the disclaimer at
   `--text-xs` (11px) with `--leading-base`, heading at `--text-sm`
   semibold; colors `--text-secondary` for body, `--text-primary` for the
   heading and leads. Share the footnote's hairline top border so the
   footnote + disclaimer read as one "conditions" block. All values come
   from `theme.css` tokens (design-system.md; `lint:css` enforces). **Confirmed: 11px body.**
5. **Heading level.** The card's tagline is its `<h1>`; "Disclaimer" is an
   `<h2>` (no `<h3>` jump, whatever the markdown draft used).
6. **Punctuation follows `strings.ts` conventions**: real apostrophes and
   the Unicode em dash (already used in the session-expired message), never
   `--`.
7. **Keep the disclaimer in every state.** Like the intro (bd-g0uyp2v1),
   it must survive the auth-error and session-expiry states; the same
   person is about to sign in again.
8. **Out of scope, filed as follow-ups if wanted:** (a) showing the same
   disclaimer on the invite landing cards (`InviteLanding.tsx`), which a
   signed-out invitee sees *instead of* `LoginScreen`; (b) reconciling
   `resources/more-info.md`'s "DO NOT USE FOR PRIVATE DATA YET" section
   with the new wording (it currently talks about public automerge sync
   servers, not Posit retention).

## Phase 1 — Tests first

- [x] `LoginScreen.test.tsx`, new `describe('LoginScreen disclaimer')`:
  - renders a heading named "Disclaimer" at level 2
    (`getByRole('heading', { level: 2, name: /disclaimer/i })`);
  - renders both lead sentences inside `<strong>` elements
    (`container.querySelectorAll('.ls-disclaimer strong')` has length 2,
    texts equal `landing.disclaimer.*.lead`);
  - renders both body texts (`getByText(/live demo of a product/)`,
    `getByText(/won.t return it/)` etc.), and the block's text content
    equals the strings module verbatim (guards the `lead + ' ' + body`
    join);
  - the block sits after `.ls-footnote` and before `.ls-actions`, and the
    existing "nothing below the button" assertion still holds (extend
    `cardOrder`-based tests);
  - survives `errorReason="denied"` and the session-expired `message`.
- [x] `e2e/landing-card.harness.spec.ts`: add `.ls-disclaimer` to the
  "prose blocks stay left-aligned" loop (both themes), and assert the
  disclaimer's computed font size is a `--text-*` step (same technique as
  the headline test).
- [x] Run: 6 new unit tests failed, 25 existing passed (the e2e addition was exercised only after implementation; it needs a built bundle).

## Phase 2 — Strings

- [x] Add `landing.disclaimer` to `hub-client/src/strings.ts` as above,
  with a doc comment stating why it is structured (heading + lead/body
  pairs, so the component can format without a markdown parser) and that
  copy changes must keep the leads as single sentences.

## Phase 3 — Component and CSS

- [x] `LoginScreen.tsx`: render the `<section>` between the footnote and
  the status/actions slot. Update the header comment and the
  "everything that qualifies the offer sits above the button" comment to
  mention the disclaimer.
- [x] `LoginScreen.css`: `.ls-disclaimer`, `.ls-disclaimer-heading`,
  `.ls-disclaimer p` — tokens only, logical properties only, opaque
  colors only (`.claude/rules/hub-client-theme.md`). Adjust
  `.ls-footnote`'s bottom margin so footnote → disclaimer → CTA spacing is
  even; keep the CTA's own breathing room.
- [x] Unit tests green (31/31 in the file, 1202/1202 suite-wide); `lint:css` clean.

## Phase 4 — Look at it

- [x] `npm run dev` in `hub-client`, open `#/dev/landing`,
  `#/dev/landing-expired`, `#/dev/landing-denied` in light and dark
  (the dev harness pages already exist). Check the card at a laptop
  viewport height (~700px): the CTA must still be reachable without the
  page feeling like a wall of text; if it is not, revisit Decisions §1–§4
  with the user rather than shrinking the type further.
- [x] `npm run test:harness` (or the single spec) for the e2e geometry
  contract.

## Phase 5 — Verify and commit

- [x] `cd hub-client && npm run build:all` (the `tsc -b` build is stricter
  than vitest) and `npm run test:ci`.
- [x] Two-commit workflow: feature commit, then
  `hub-client/changelog.md` entry under `### 2026-09-15` with the hash.
- [x] Record the end-to-end check (harness URL, both themes, screenshot or
  DOM snippet showing `section.ls-disclaimer > h2 + p > strong`) in this
  plan before closing the strand.

## End-to-end verification record (2026-09-15)

The Chrome extension was not connected, so the harness pages were driven
with headless Chromium through Playwright against `vite --port 5174`
(`#/dev/landing`, `#/dev/landing-denied`, light and dark, 1280×720). The
rendered markup, inspected from the page:

```html
<section class="ls-disclaimer" aria-labelledby="ls-disclaimer-heading">
  <h2 id="ls-disclaimer-heading" class="ls-disclaimer-heading">Disclaimer</h2>
  <p><strong>Use test data, not real data.</strong> This is a live demo of a product still in development. …</p>
  <p><strong>Anything you enter is not protected, and we won’t return it.</strong> Data you share during this demo …</p>
</section>
```

Measured geometry (identical in both themes):

| page            | card height | fits 720px viewport | body / heading font |
| --------------- | ----------: | :-----------------: | ------------------- |
| landing         |       565px |         yes         | 11px / 12px         |
| landing-denied  |       597px |         yes         | 11px / 12px         |

Screenshots were inspected in both themes: the heading and bold leads
read in `--text-primary`, the body in `--text-secondary`, the block sits
under the invite-only hairline and above the Continue with Google button.

Test runs (all under the pinned Node 24 — under the Node 26 that
happened to be on PATH, 30 unrelated jsdom tests in `usePreference` and
`EditorWelcomeBanner` fail with `Cannot read properties of undefined
(reading 'clear')`; that is an environment artifact, not this change):

- `vitest run src/components/auth/LoginScreen.test.tsx`: 31 passed
- `npm run test`: 101 files, 1202 passed
- `npm run test:integration`: 131 passed; `npm run test:wasm`: 133 passed
- `VITE_E2E=1 npm run build` (tsc -b + vite) clean, then
  `playwright test --config playwright.harness.config.ts e2e/landing-card.harness.spec.ts`: 12 passed
- `npm run build:all` exit 0; `npm run lint:css` clean
