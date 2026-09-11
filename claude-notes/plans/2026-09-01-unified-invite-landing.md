# Unified InviteLanding for collection and document invites

**Strand:** bd-fxdcxbpq · **Branch:** `onboarding/invite-landing` · **Scope:** `hub-client/`
**Design handoff:** `design_handoff_invite_landing/` (README.md is the spec; `screenshots/3a-unified-invite-landings.png` is the authoritative mock)

## Overview

Both invite entry paths — `#/join-collection/…` and `#/share/…` — get one shared
**InviteLanding** card: kicker, inviter line, title, display-only payload
preview, what-is-QuartoHub explainer, single CTA. Signed-out users see a Google
CTA and round-trip back to the same invite URL via the existing pre-auth hash
save/restore; signed-in users get one-click join/open. After joining, the user
lands in the editor on the intended file (not the home screen) with a one-time
dismissible welcome banner. Share routes stop auto-connecting; connection
happens on CTA click.

Out of scope (later PRs): zero-setup cold start (ProjectSetSetup removal),
seeded first-run samples, revocable/role-scoped links, sender UI redesign
(only the URL builders change).

## Current-state seams (from 2026-09-01 exploration)

| Concern | Location |
|---|---|
| Route types / parse / build | `hub-client/src/utils/routing.ts:99-178`, share parse `236-251`, invite parse `266-286`, builders `367-393`, `433-451` |
| Pre-auth hash save/restore | `routing.ts:566-602`, driven from `src/main.tsx:29` — already round-trips invite URLs unchanged |
| Share auto-connect | `App.tsx:460-507` (`connectToSharedProject`), called at `510-532`; **second caller** at `551-567` (ephemeral preview recovery) must keep connecting eagerly |
| Signed-out gate to bypass | `App.tsx:785-792` (`AUTH_ENABLED && !auth → LoginScreen`) |
| Invite render site / post-join landing | `App.tsx:815-824`; `onDone` → `navigateToProjectSelector` at `821` (the "dumped on home" behavior) |
| Join call | `JoinCollectionLanding.tsx:80-106` → `onSubscribe` → `useCollectionSets.ts:414-418` → `projectSetService.connectCollection` |
| Preview payload source (cached peeks) | `ProjectSetEntrySummary` (`ts-packages/quarto-automerge-schema/src/index.ts:138-147`); written `App.tsx:310-325`, `ProjectsHome.tsx:1178-1195`; read via `collectionItemsOf`/`peopleOn` `ProjectsHome.tsx:1254-1289` |
| Sender URL builders | `buildInviteUrl` `ProjectsHome.tsx:1265-1273`; `buildShareableUrl` `routing.ts:433-451` (call sites `ProjectsHome.tsx:1144`, `Editor.tsx:849-855`) |
| Identity | `src/services/userSettings.ts`; Google-name upgrade `App.tsx:222-235`; facepile/initials `ProjectsHome.tsx:162,1043-1062`, `src/utils/facepile.ts` |
| Card CSS to reuse | `.qh-join-card` `ProjectsHome.css:452-521` (~90% of the card spec already); peek styles `.qh-peek*`; tokens in `theme.css` map 1:1 to the handoff hexes |
| Dismissal pattern | keyed localStorage flag à la `MOVE_WARNING_KEY` (`ProjectsHome.tsx:126,572,2024`) |
| Dev harness | `DEV_PAGES` registry `DevHarness.tsx:686`, route `#/dev/<page>` |

## Design decisions

1. **Token mapping** (prefer hub-client tokens over mock hexes, per handoff):
   card border `--border-color`; dividers `--posit-blue-light-1`; title
   `--text-primary`; secondary `--text-secondary`; kicker + primary CTA
   `--accent-action-bg`/`--accent-action-text`; radii `--radius-xl/lg/md`;
   shadow `--shadow-3`; mono `--font-mono`. Explainer tint: **opaque**
   `color-mix(in srgb, var(--posit-blue) 8%, var(--bg-modal))` — the
   hub-client-theme rule forbids new alpha tokens (do not reuse
   `--posit-blue-alpha-08`).
2. **`preview=` / `start=` / `from=` encoding**: base64url(JSON), compact keys
   (follow the existing `entries={d,s,n}` precedent). Display-only — never doc
   ids. Parser accepts absent/malformed payloads (legacy links render the
   card without the preview block, generic CTA text).
3. **Preview payload shape** (v1, versioned `{v:1,…}`):
   - collection: `{v, p:[{n,f,c,i:[initials]}...], t:totalProjects, m:[firstNames]}` capped at 3 projects × 2 files
   - document: `{v, f:filename, s:[topFiles], c:fileCount, i:[initials]}`
4. **Google CTA — resolved gap.** There is no local Google asset; the current
   button is rendered by Google Identity Services (`<GoogleLogin>`), whose
   label/styling cannot read "Continue with Google to join". Plan: add a small
   inline Google "G" SVG and a custom `.qh-btn.outline` CTA that submits the
   same nonce-gated redirect form the GIS button drives (must preserve the
   nonce gating in `GoogleAuthProvider.tsx:76-78` — render disabled until the
   nonce resolves). The `AuthProvider` interface grows a `SignInCta`
   (labelled) alongside the existing `SignInButton`; `noopAuthProvider` gets a
   matching stub. **Fallback if the redirect can't be reproduced reliably:**
   keep `<GoogleLogin>` inside the card with the handoff copy above it — flag
   to Andrew before taking the fallback.
5. **Share connect-on-CTA**: the `route.type === 'share'` arm in the initial
   navigation effect stops calling `connectToSharedProject`; instead it stores
   parsed invite state and renders InviteLanding. `connectToSharedProject`
   itself is unchanged (the ephemeral-preview recovery caller keeps eager
   behavior). The security hash-scrub (`navigateToProjectSelector({replace:true})`
   before connect) moves to CTA click time; the landing itself must not keep
   the doc id in the visible URL longer than today — scrub on mount, keep
   parsed route in state.
6. **Post-join `start` target**: after `onSubscribe` resolves, resolve the
   `start` project within the joined collection's entries after sync; if
   resolvable, connect + `navigateToFile`; else home with the collection
   visible (today's behavior).
7. **Welcome banner dismissal**: keyed localStorage flag
   `qh-invite-welcome-dismissed:<collectionOrProjectDocId>` (the
   `usePreference` record shape doesn't fit per-id keys).
8. **Identity**: name/color form is deleted; identity comes from the existing
   Google-name upgrade (`App.tsx:222-235`) falling back to
   `userSettingsService` defaults. The banner's "Change name" opens the
   existing avatar-menu rename affordance.

## Phase 1 — Test specifications (write first, red)

- [x] `routing.test.ts` additions: parse/build round-trips for `from=`,
      `preview=`, `start=` on both routes; legacy URLs (no new params) parse
      to the same route objects as today; malformed/oversized `preview=`
      degrades to `undefined`; payload encoder caps at 3 projects × 2 files.
- [x] New `invitePreview.test.ts`: base64url JSON encode/decode unit tests
      (pure functions, no DOM).
- [x] `InviteLanding.test.tsx` (jsdom pragma): renders both `kind`s ×
      {signed-in, signed-out}; exact kicker/CTA copy per matrix (incl.
      "Join and open <start name>" / generic legacy text); payload block
      skipped when preview absent; no name input, no color swatches, no
      footnote below CTA; CTA busy state while joining.
- [x] `App`-level "share does not connect on load, connects on CTA" —
      **deferred to a Phase 4 harness e2e**: App's initial-navigation effect
      is too entangled for a unit test; the harness route + Playwright spec is
      the honest surface for it.
- [x] Welcome banner test: shows once, dismissal persists per id, "Change
      name" invokes the rename affordance callback.
- [x] Run all new tests, confirm they fail for the right reason before Phase 2.
      (2026-09-01: 4 files red — 3 missing-module import failures, 2 routing
      round-trip assertion failures; all 95 pre-existing routing tests green.)

`start=` encoding note: the start target is `{d: indexDocId, f: filePath}`
base64url JSON — a doc id is acceptable here (unlike `preview=`, which stays
display-only) because the collection invite already grants access to every
entry via the collection doc; name-based matching would break on renames.

## Phase 2 — Routing + payload plumbing

- [x] `routing.ts`: extend `ShareRoute` (`from?`, `preview?`) and
      `JoinCollectionRoute` (`preview?`, `start?`); parse + build; keep all
      params optional.
- [x] New `src/utils/invitePreview.ts`: payload types, base64url
      encode/decode, size cap, version check.
- [x] Phase 1 routing/payload tests green (106/106 incl. all pre-existing).

## Phase 3 — InviteLanding component

- [x] New `src/components/InviteLanding.tsx` (+ `InviteLanding.css`): props
      `{kind, inviter, title, preview, signedIn, startName, joinState,
      ctaDisabled, error, onCta}`; card anatomy per handoff §"Shared Card
      Anatomy"; base styles adapted from `.qh-join-card`; quarto icon from
      `/quarto-icon.svg` (with `--logo-filter`); CSS lint clean (logical
      box props).
- [x] Google CTA — **decision 4 revised (Andrew, 2026-09-01)**: the hub's
      /auth/callback (crates/quarto-hub/src/auth.rs) only accepts GIS-minted
      `credential=` form POSTs, so a custom button cannot legitimately drive
      the flow. Resolution: the provider's own `<GoogleLogin>` with
      `text="continue_with"` ("Continue with Google"), passed into
      InviteLanding as the `signInCta` node; `SignInButtonProps` gained a
      `text` variant. The "to join"/"to open" suffix from the mock is
      dropped (GIS controls the label).
- [x] Dev harness pages: `invite-landing-collection{,-signed-in,-legacy}`,
      `invite-landing-document{,-signed-in}`, `invite-welcome-banner`.
- [x] Component tests green (17/17); harness pages visually checked against
      `3a-unified-invite-landings.png` in the dev server (collection,
      document, and signed-in variants all faithful).

## Phase 4 — App wiring

- [x] Signed-out invite routes render InviteLanding above the LoginScreen
      gate; the GIS CTA round-trips via pre-auth hash save/restore (share
      hashes re-saved explicitly since the URL is scrubbed on mount —
      `savePreAuthHash` gained an optional hash param).
- [x] Share arm: auto-connect removed for non-ephemeral links (hash still
      scrubbed on mount; route captured into `pendingShare` at boot);
      `connectToSharedProject` hoisted to component scope with
      `{quiet, addToSet}` opts; ephemeral preview boot/reload keep eager
      connect.
- [x] Collection CTA: `subscribeCollection` with account identity (no form) →
      `start` target opened via `connectToSharedProject` (addToSet: false) →
      editor on file, else home. `JoinCollectionLanding` deleted (component,
      test, and its `.qh-join*` CSS). The invite-first silent root creation
      effect now also covers share invitees. Known trade-off: the
      auth-expired "Sign in again" secondary action from the old landing is
      not carried over (error text still shows).
- [x] Broken/incomplete links: error copy rendered inside the landing card,
      CTA disabled.
- [x] Integration tests green (118/118); `share-link-project-set.spec.ts`
      updated to click through the landing (this is the "no connect on load,
      connect on CTA" e2e coverage).

## Phase 5 — Welcome banner

- [x] `src/components/EditorWelcomeBanner.tsx`: tinted bar rendered via a new
      optional `banner` prop on Editor (next to EphemeralSessionBanner);
      collection + document copy variants; × dismiss persists per decision 7.
      **Deviation from handoff**: there is no existing in-editor rename
      affordance (the avatar menu lives on the home screen), so "Change name"
      opens a small inline rename in the banner itself (input + Save →
      `updateUserName`). Flagged for Andrew's review.
- [x] Banner tests green (8/8).

## Phase 6 — Sender side

- [x] `buildInviteUrl` (`ProjectsHome.tsx`): embeds `preview=` from cached
      peek summaries (3 projects × 1 file + facepile initials + member first
      names) and `start=` = first project + its first cached top file (per
      Andrew's default-to-first decision).
- [x] `buildShareableUrl` gained `opts {from, preview}`; both call sites
      embed them (ProjectsHome card menu from the peek summary; Editor's
      ShareDialog from live files + identities, via a new `userName` prop on
      Editor). `initialsFor` promoted to `utils/facepile.ts`.
- [x] URL-length sanity test with max payload (maximal invite hash < 1500
      chars, round-trips).

## Phase 7 — Verification + ship prep

- [ ] `cd hub-client && npm run preflight` and `npm run test:ci`.
- [ ] `npm run build:all` (stricter than typecheck — required for hub-client).
- [ ] End-to-end per CLAUDE.md: real browser against a running hub — generate
      an invite link from one profile, open signed-out in another, complete
      Google round-trip (or `--allow-insecure-auth` local equivalent), verify
      landing → join → editor-on-file → banner → dismissal. Record invocation
      + observed output here.
- [ ] Legacy-link e2e: pre-change share + join-collection URLs still work
      (`e2e/share-link-project-set.spec.ts` still green).
- [ ] Two-commit changelog workflow (`hub-client/changelog.md`).
- [ ] Review checklist (`claude-notes/instructions/review.md`) before each
      commit; no push without Andrew's approval.

## Resolved questions (Andrew, 2026-09-01)

1. Google CTA: build the custom nonce-gated button with an inline "G" SVG
   (decision 4 as written; fallback only if the redirect can't be reproduced,
   flagged before taking it).
2. `start=`: default to the collection's first project + its first cached top
   file; sender override comes with the later sender-UI PR.

## Notes / gotchas

- Test files are excluded from `tsconfig.app.json`; route-type tightening only
  surfaces in vitest, not the app build.
- `JoinCollectionLanding.test.tsx`'s route literal omits `entries` — dies with
  the component in Phase 4.
- CSS is linted for token usage (`scripts/lint-css.mjs`); no new alpha colors.
