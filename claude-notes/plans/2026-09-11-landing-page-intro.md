# Landing page explains Quarto Hub; Learn more points at the real site

**Strand:** bd-g0uyp2v1 · **Branch:** `braid/bd-g0uyp2v1-landing-page-intro` · **Scope:** `hub-client/`
**Closes:** bd-rh2n4d7q (the provisional `quarto.org` Learn more URL)

## Overview

`quarto-hub.com` renders `LoginScreen` for anyone without a session: a logo,
the words "Quarto Hub", "Sign in with Google to continue", and the GIS
button. A first-time visitor — including someone who followed an invite
link — learns nothing about what they are signing in to.

Add a short intro borrowed from the project's own marketing site
(<https://quarto-dev.github.io/quarto-hub/>), and point every "Learn more"
at that site: the new landing copy and the invite cards, which currently
link to `quarto.org` as a placeholder.

## Source copy (from the marketing site, 2026-09-11)

- Headline: *"Prose and code belong in one place. So do the people."*
- Subhead: *"Quarto Hub is a Quarto editor in the browser that renders while
  you type."*
- *"You edit the Quarto markdown (source) or directly make changes in the
  rendered page, and either way it's the same `.qmd` underneath."*
- What works today: Websites and docs · Slides (revealjs, edited and
  previewed in place) · Meeting notes (one `.qmd` the whole team types into
  during the call) · Agents (point an LLM at the project; it edits the same
  files)
- *"quarto-hub.com, the experimental service, is currently available by
  invite only."*

## Decisions

1. **One screen, not a new route.** `LoginScreen` *is* the landing page
   (App renders it whenever `AUTH_ENABLED && !auth`), so the intro goes
   there rather than into a new marketing route.
2. **The intro coexists with the error/expiry states.** The same component
   serves session expiry and eleven auth-error reasons, whose copy
   currently *replaces* the default line. The intro stays in all states:
   for "not authorized to access this hub" the invite-only note is the
   most useful thing on the page. Existing LoginScreen tests keep passing
   unchanged.
3. **Do not borrow the "no account needed" claim.** The site says *"Anyone
   with a project link can open the project, read it, and comment. They
   don't need an account or a Quarto install."* That is the product's
   intent, but on this deployment the visitor is looking at a sign-in wall
   and an allowlist, so printing it here would contradict the page it sits
   on. Left out deliberately.
4. **Say it is invite-only.** A visitor who cannot get in should learn that
   from the page rather than from a failed sign-in.
5. **Copy lives in `strings.ts`**, per that module's own rule ("when you add
   user-facing copy, add it here in the same commit").
6. **One shared URL constant.** `LEARN_MORE_URL` moves out of
   `InviteLanding.tsx` so the landing page and the invite cards cannot
   drift; both point at the marketing site.

## Phase 1 — Tests first

- [x] `LoginScreen.test.tsx` additions: renders the tagline, the
      what-it-is line, the invite-only note, and a Learn more link
      pointing at the marketing site; the intro is present alongside an
      `errorReason` and alongside a `message`.
- [x] `InviteLanding.test.tsx`: the footnote's Learn more href is the
      marketing site, not `quarto.org`.
- [x] Run all, confirm red for the right reasons.

## Phase 2 — Implementation

- [x] Add the landing copy to `strings.ts`.
- [x] Shared URL constant: `links.quartoHub` in `strings.ts`, consumed by
      both `LoginScreen` and `InviteLanding`. `LEARN_MORE_URL` deleted.
- [x] Rebuild `LoginScreen`'s markup: lockup, tagline, what-it-is,
      invite-only footnote, status slot, sign-in button. Inline styles
      replaced by `LoginScreen.css` so `lint:css` can check them.
- [x] Dev-harness pages `landing`, `landing-expired`, `landing-denied`,
      each wrapping `LoginScreen` in a provider that renders a stand-in
      for the GIS button.

## Phase 3 — Verification

- [x] `npm run typecheck`, unit, integration, wasm, `lint:css`,
      `npm run build:all`.
- [x] Theme harness: the new page in light and dark (the invite work's
      lesson — dark mode was where the defects hid).
- [x] Screenshots of the landing page, light and dark, for the PR.
- [x] Real browser (`vite dev` + the harness routes): all three states
      render, and every Learn more across both surfaces resolves 200 at
      the marketing site with no redirects.

## Design iterations (Andrew, 2026-09-11)

Recorded because several reverse an earlier decision above.

7. **"What works today" removed.** Decision 5's four labels made the card
   a feature list. Cut entirely; `worksToday*` strings deleted.
8. **Mixed alignment, deliberately.** Masthead, status line, and CTA are
   centered; the description and the invite-only footnote stay
   left-aligned. Centering the description was rejected: it is the one
   block a first-time visitor must read, and centering moves the start of
   every line. Pinned by `e2e/landing-card.harness.spec.ts`, which
   measures rendered glyph positions rather than CSS properties so the
   mix survives a later "make this consistent" edit.
9. **No status line in the default state.** It said "Sign in with Google
   to continue" directly above a button reading "Continue with Google".
   The slot now renders only for the error and expiry states, which have
   something to report. `landing.signIn` deleted.
10. **Headline on the type scale.** It was a hand-picked `21px`; the scale
    is 13/18/24. Now `--text-xl`, with the card widened 460 → 520px so
    neither sentence of the two-line headline wraps.
11. **New semantic token `--bg-page-recessed`.** The landing page derived
    its page surface as `color-mix(--border-color 14%, --bg-modal)`, which
    only reads right in light mode: in dark, `--border-color`
    (`--posit-blue-dark-1`) is *lighter* than `--bg-modal`
    (`--posit-blue-dark-2`), so the page came out lighter than the card it
    framed — elevation inverted, at 1.06:1. The token is per-theme because
    the direction is not derivable; dark mixes 40% toward `--black` for
    1.38:1 the right way round. The load-bearing assertion in the theming
    spec is the *direction*: 1.06 clears any magnitude floor.
12. **`.il-wrap` deliberately not migrated.** It still derives the old way
    and so still inverts in dark mode. Andrew scoped this branch to the
    login/front page; the invite card keeps only the Learn more change.
    Worth its own strand, along with that card's own off-scale type
    (`.il-title: 23px`, `.il-inviter: 13.5px`, `.il-file-summary: 11.5px`).
13. **Learn more moved into the description**, and set as a block on its
    own line there. Run on inline, whether it landed on the last line or
    dangled under a full one was decided by the paragraph's character
    count — it broke on all three copy revisions this branch went through.
    The spec pins the mechanism, not the wrap.
14. **Both Learn more links open a new tab** (`rel="noopener noreferrer"`).
    Following in place abandons the sign-in the visitor came to complete,
    and on an invite the invite link with it.
15. **The description names collaboration.** "So do the people" only
    gestures at it. Revised per the `gopen-rea` skill: three sentences,
    each ending on its own new idea (live render, one `.qmd`, real-time
    collaboration). Andrew's draft ended "…on the same project in
    QuartoHub"; the product name was dropped from that slot because the
    stress position is the emphatic one and the name is the paragraph's
    own opening subject, the lockup above, and the link below.

## Resolved questions

1. ~~How much of "what works today" to include~~ — none; see 7.
2. The invite cards' footnote keeps "New to Quarto Hub? Learn more."
