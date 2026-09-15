# Handoff: Unified Invite Landing (collection + document invites)

Branch suggestion: `onboarding/invite-landing`
Repo: `quarto-dev/q2` · scope: `hub-client/`

## Overview
Redesign of QuartoHub's two invite entry paths so they share one landing pattern:

1. **Invite to a collection** (`#/join-collection/…`) — today: login wall → name/color form → dumped on home screen.
2. **Invite to a document** (`#/share/…`) — today: login wall → silently cold-opens the editor with zero orientation.

New behavior: both links land on a single **InviteLanding** card that shows *who invited you*, *what you're being invited to* (a display-only preview), a short *what-is-QuartoHub* explainer, and exactly **one CTA**. Signed-out users get a Google sign-in CTA and round-trip back to the same card; signed-in users get a one-click join/open. After joining, the user lands **in the editor on the intended file** (not the home screen) with a one-time welcome banner.

Out of scope for this PR (deferred to later PRs): zero-setup cold start (removing ProjectSetSetup), seeded first-run samples, revocable/role-scoped links.

## About the Design Files
`QuartoHub Onboarding.dc.html` in this bundle is a **design reference created in HTML** — a canvas of annotated mockups, not production code. The task is to **recreate the designs in `hub-client`'s existing React + CSS environment**, reusing its components, tokens (`theme.css`, `ui.css`), auth flow, and routing utilities. The authoritative mocks are the two cards in section **3a** ("One invite pattern, two payloads"), as amended: no "Signed in already…" copy and no "Anyone with this link…" footnote in the cards — the CTA is the last element. Sections 1a (current-state map) and 2a/2b give context; 2b (zero-setup) is NOT in this PR.

## Fidelity
**High-fidelity for structure, hierarchy, and copy; medium for exact pixel values.** Recreate the card anatomy, ordering, and copy exactly. For colors, radii, type, and spacing, prefer the hub-client's own tokens over the hex values below (the mocks approximate them). Where the mock and an existing hub-client pattern conflict, follow hub-client.

## The Shared Card Anatomy (both invite types)
Centered card on the app background, max-width ~460px, in this exact order:

1. **Kicker** — `COLLECTION INVITATION` / `DOCUMENT INVITATION`. Small caps label, brand accent color, ~11px bold, letter-spacing .06em.
2. **Inviter line** — inviter avatar (initials circle, ~28px) + "**Carlos Scheidegger** invited you to" (collection) / "invited you to edit" (document). Inviter name bold, rest regular, ~13.5px.
3. **Title** — collection or project name. ~22–24px bold headline.
4. **Payload preview** (bordered rounded box, display-only data — differs by type):
   - *Collection*: list of up to ~3 projects, each row = project name (13px semibold) + top files/file-count line in mono (`report.qmd · 12 files`, ~11.5px) + contributor initials facepile (overlapping ~19px circles). Final row: `+ N more projects · Carlos, Jenny and Mine work here`.
   - *Document*: document thumbnail placeholder (~74px tall, subtle line texture) with a mono filename chip (`report.qmd`) bottom-left, then one row: mono file summary (`figures/ · data.csv · 12 files`) + contributor facepile.
5. **Explainer block** — light tinted rounded box: Quarto logo (circle with 4 quadrants; use the app's real `quarto` icon asset) + "**New to Quarto Hub?** It's where teams write Quarto documents together — live, in the browser. Nothing to install." (~11.5px, bold lead-in.)
6. **CTA — single button, last element, nothing below it.**
   - Signed **out**: white/outlined button with Google logo — "Continue with Google to join" (collection) / "Continue with Google to open" (document).
   - Signed **in**: primary filled button — "Join and open Quarterly report" (collection, using the start-here project name; if no start target, "Join Team docs") / "Open Quarterly report" (document).

Do NOT render: name input, cursor-color picker, "anyone with this link" warnings, or any explanation of the signed-in variant.

## Interactions & Behavior
- **Signed-out**: invite routes render InviteLanding (not the generic LoginScreen). Google CTA triggers the existing auth flow; the existing pre-auth hash save/restore returns the user to the same invite URL, now showing the signed-in variant with the user's Google name/avatar ("Joining as **Amy Mora**" may appear near the CTA in the signed-in state — see turn-2 step-2 mock).
- **Collection join CTA**: performs today's join (subscribe to collection) with identity from the Google account (name + avatar); no identity form. Then navigate to the `start` target (project + file) if present and resolvable after sync; else home with the joined collection visible.
- **Document open CTA**: connection to the shared project happens **on CTA click**, not on route load (today `#/share/` connects silently). Then open the editor on the shared file.
- **Editor welcome banner** (one-time, dismissible, persist dismissal): tinted bar under the toolbar — "Welcome to **Team docs** — Carlos suggested starting here. You're editing live as **Amy Mora**." + a "Change name" outline button (opens existing rename affordance) + × to dismiss. Document-invite variant: "**Carlos** shared this document with you. You're editing live as …".
- **Legacy URLs** (no new params): must keep working — render the landing without the preview block and with generic CTA text ("Join collection" / "Open document").
- Loading: if preview payload absent/invalid, skip the payload block gracefully. Broken share links keep today's error handling but styled within the landing card.

## URL / Data Changes (`routing.ts`)
- `share` route gains: `from=` (inviter display name) and display-only `preview=` (filename shown in thumb chip, top files, file count, contributor initials).
- `join-collection` route gains: `preview=` (up to ~3 projects × {name, top files, file count, contributor initials} + total count + member first-names) and `start=` (project + file to open after join).
- Preview payloads are **display-only** — never include doc ids or anything that grants access. Cap size for URL length (~3 projects × 2 files). Encode compactly (e.g. base64url JSON).
- All params optional; parsers must accept legacy links.

## Sender side
- `ProjectsHome.tsx` — `buildInviteUrl` embeds `preview=` from the already-cached peek summaries (top files + contributor facepiles) and an optional start-here pick.
- `ShareDialog.tsx` / `buildShareableUrl` — embed `from=` + `preview=`.
- (Sender UI redesign is not in this PR; only the URL builders change.)

## Component Plan
- **New `InviteLanding.tsx`** — replaces `JoinCollectionLanding.tsx`; one layout, props: `kind: 'collection' | 'document'`, inviter, title, preview payload, signedIn, onCta. Kicker text, payload renderer, and CTA verb switch on `kind`.
- **`App.tsx`** — invite routes render InviteLanding when signed out (skip LoginScreen); share route stops auto-connecting; post-join navigation to `start` target.
- **`JoinCollectionLanding.tsx`** — removed/absorbed (name + cursor-color form deleted; identity comes from the Google account, falling back to the existing identity service defaults).
- **Editor** — one-time welcome banner component.

## State Management
- `signedIn` from existing auth context.
- Parsed invite params (kind, inviter, title, preview, start) from the route.
- `joinState`: idle → joining/connecting → navigating; CTA shows a busy state while joining.
- Banner dismissal persisted (per collection/project id) in the same storage used for similar one-time UI.

## Design Tokens (approximations used in mocks — prefer hub-client equivalents)
- Card border `#A2B8CB`, inner dividers `#D1DBE5`, radius 16px card / 12px payload box / 8px buttons.
- Text: near-black `#17212B` (titles), muted blue-gray `#305775` (secondary), accent teal `#2E6E71` (kicker, primary button bg), link/brand blue `#447099`.
- Explainer tint `rgba(68,112,153,.08)`.
- Mono for file names/summaries (the app's existing mono, JetBrains Mono in mocks).
- Card shadow `0 20px 60px rgba(0,0,0,.15)`.

## Assets
- Quarto logo: use the app's existing quarto icon (the mocks draw a 4-quadrant circle approximation — do not ship that).
- Google logo: existing sign-in asset.
- Avatars: initials circles with the existing user-color assignment.

## Tests
- InviteLanding renders both kinds, signed-in and signed-out.
- Legacy invite/share URLs (no `preview`/`from`/`start`) render and function.
- Collection join → lands on `start` target; no `start` → home.
- Document CTA → connect happens on click, then editor opens on the file.
- Welcome banner shows once, dismissal persists.
- Dev-harness pages for both landing variants.

## Files in this bundle
- `QuartoHub Onboarding.dc.html` — design canvas. Section **3a** = the authoritative mocks (note: later chat edits removed the footnote lines under the CTAs; CTA is the final element). Section 2a = the three-step flow context (signed-out → back-from-Google → editor banner). Section 1a = current-state map of all entry paths with pain points.
- `screenshots/3a-unified-invite-landings.png` — the two authoritative invite cards (collection + document) with annotations.
- `screenshots/2a-invite-flow-steps.png` — full flow: signed-out landing → back from Google → editor with welcome banner.
