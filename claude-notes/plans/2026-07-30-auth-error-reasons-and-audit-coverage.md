# Auth failure: distinguishable reasons and audit coverage

**Status:** implemented 2026-07-30 on `braid/bd-htis60s7-auth-error-reasons`.
**Date:** 2026-07-30.
**Follows:** `claude-notes/plans/2026-07-27-auth-current-flow-hardening.md`
(item H2, shipped in `807fd96c`, PR #427) and
`claude-notes/plans/2026-07-06-hub-server-minted-sliding-sessions.md` (C0–C7).
**Ops doc to keep in sync:** `dev-docs/quarto-hub/session-auth-operations.md`.

## Overview

Every way `POST /auth/callback` can fail lands the user on the same screen
with the same sentence:

> Sign-in failed. Your account is not authorized to access this hub.

Eleven distinct causes collapse into that one message, and only two of them
(a banned user, and an email matching no allowlist) are actually about
authorization. The others — an expired sign-in, a stale browser tab, a lost
cookie, a CSRF mismatch, a server-side mint failure — are misreported as a
permissions problem, sending the user to an administrator instead of to the
reload button that would fix it.

Two audit gaps sit underneath:

- The CSRF check (`server.rs:1049`) calls `auth_error()` without emitting an
  audit event, so a deployment failing there is invisible in
  `journalctl -u hub`.
- `check_login_nonce` returns `login_state_missing` for a cookie-absent
  callback without inspecting the token, conflating "this browser is running
  an old bundle" with "the cookie did not survive Google's cross-site POST".
  Those want opposite remedies. The same return also emits **doubled** —
  it returns `"login_state_missing"` (`server.rs:997`) and the emit site
  wraps it in `format!("login_state_{detail}")` (`server.rs:1071`), so the
  log shows `detail=login_state_login_state_missing` while the ops doc
  (line 77) documents `login_state_missing`. No test pins any of the six
  detail strings.

| Item | What | Size |
|------|------|------|
| E0 | `login_state_missing` conflates stale-client with cookie-loss, and is emitted double-prefixed | S |
| E1 | `auth_error()` collapses 11 causes into "not authorized", discarding the 403/401 status that separates the two real denials from the rest | M |
| E2 | CSRF failures are audit-silent | S |
| E3 | Auto-generated secrets are silent (multi-instance hazard) | S |

E0 lands before E1 — it is what makes E1's `stale_client` reason
expressible. E1 is the highest-leverage item: it turns the next auth failure
into a legible error instead of a debugging session.

## Design constraints

1. **User-facing reasons stay coarse** — `stale_client`, `restart`, `denied`,
   `server`. They land in a URL the user can see and an attacker can
   enumerate, so fine distinctions (tampered / kid mismatch / expired /
   nonce mismatch) live **only** in the audit log. The mapping is
   deliberately many-to-one.

2. **The audit log is where precision goes.** Every failure path carries a
   `detail` discriminator. Where a path already has one, pin it in a test
   rather than adding a coarser one on top.

3. **`auth_error()` holds one invariant** — every exit path from the callback
   clears the sealed login-state cookie, so a single pre-flight can complete
   at most one login (`server.rs:1034-1042`). Parameterizing the closure must
   not break that; each new reason path gets a test asserting the
   clear-cookie is present.

4. **`denied` means an identity was established and then refused.** Exactly
   two causes qualify: a ban, and an allowlist miss. `authenticate_claims`
   already separates the latter from credential failures by status — 403 for
   "good credentials, wrong user", 401 for everything else
   (`auth.rs:358-360`) — so the callback reads that status rather than
   discarding it.

## Work items

### Phase 1 — E0: make `login_state_missing` self-disambiguating

Tests first, in `crates/quarto-hub/tests/integration/login_nonce.rs`
(harness already present: `secure_google_setup`, `fetch_nonce`,
`post_callback`, `assert_auth_error`, `snapshot_events`):

- [x] A callback with **no cookie and a nonce-less token** asserts, via
  `snapshot_events`, the **exact** emitted detail `login_state_stale_client`.
- [x] A callback with **no cookie but a nonce-bearing token** asserts the
  **exact** emitted detail `login_state_missing`. Exactness is the point —
  today the log emits `login_state_login_state_missing`.

Then implement:

- [x] Split the early return in `check_login_nonce` (`server.rs:996-997`),
  which currently fires before `claims.nonce` is ever examined. Return the
  **bare class** from both branches; the emit site (`server.rs:1071`) adds
  the `login_state_` prefix:
  - cookie absent **and** `claims.nonce.is_none()` ⇒ return `"stale_client"`
    (emitted as `login_state_stale_client`).
  - cookie absent but `claims.nonce.is_some()` ⇒ return `"missing"`
    (emitted as `login_state_missing`, aligning code with the ops doc).

  The check runs after `authenticate_claims`, so `claims` is already
  signature-validated — reading `claims.nonce` here is safe.
- [x] Add both discriminators to the ops doc's audit quick reference
  (`dev-docs/quarto-hub/session-auth-operations.md:163`).
- [x] Rewrite the `login_state_missing` description (ops doc line 77) to
  carry **both readings**: a nonce-bearing token with no cookie is either
  benign cookie loss (`SameSite=None` / `Path=/auth` / the reverse proxy —
  the fix is configuration) **or the replay shape the doc records today** (a
  captured token replayed from a browser that never did the pre-flight). The
  two are indistinguishable per event; telling them apart needs correlation
  (volume, distinct `sub`s, source IPs). Do not erase the attack signature
  the current text documents.
- [x] Document `stale_client` in the ops doc as "old bundle **or** a login
  attempt made outside the app" — not "definitely an old bundle". A current
  client cannot submit a nonce-less token (`GoogleAuthProvider` renders
  nothing until the nonce is in hand,
  `hub-client/src/auth/GoogleAuthProvider.tsx:70-77`), but the hub's Google
  client ID ships in the SPA, so anyone driving GIS directly can mint a
  signature-valid nonce-less token for our `aud`. Enforcement is unaffected;
  the heuristic is about honest clients, not a guarantee of benignity.
- [x] Note in the ops doc that tooling exact-matching the old doubled string
  must be updated. Substring greps for `login_state_missing` matched both
  forms and keep working.

### Phase 2 — E2: audit-log the CSRF failure path

CSRF is the only genuinely silent path. Every `Err` return from
`authenticate_claims_for_kind` (`context.rs:571-661`) already emits an
`auth_fail` event with a discriminator strictly *finer* than a blanket
`credential_invalid` would be: `jwt_decode:<err>`, `azp_or_iat_rejected`,
`email_not_verified`, `user_not_allowlisted`. **Do not add a blanket emit at
`server.rs:1055-1057`** — it would fire a second, less specific WARN
alongside each of those, burying the detail an operator needs.

Tests first:

- [x] Assert via `snapshot_events` that a bad CSRF pair emits `auth_fail`
  with `detail = "callback_csrf"`.
- [x] **Pin the existing credential-path details.** Assert that a
  non-allowlisted (but otherwise valid) credential emits
  `detail = "user_not_allowlisted"`, and that an undecodable credential emits
  a `jwt_decode:`-prefixed detail. Nothing asserts these from the callback
  path today. The first doubles as the fixture Phase 3 needs for the `denied`
  mapping.

Then implement:

- [x] Emit the existing `auth_fail` audit shape (the one at
  `server.rs:1063-1071` — `target: "quarto_hub::audit"`, `Level::WARN`,
  `action`, `outcome`, `credential_kind`, `detail`) at the CSRF rejection
  (`server.rs:1049`) **only**.
  - **`sub` is not available** — the CSRF check runs before the token is
    parsed. Omit the field rather than inventing a placeholder, and do not
    restructure the handler to make a `sub` available; an unvalidated `sub`
    in the audit log is worse than an absent one.
- [x] Add `callback_csrf` to the ops doc's audit quick reference, with one
  caveat sentence: it is emittable by unauthenticated callers (garbage POSTs
  to `/auth/callback`), so WARN volume there is attacker-influenceable —
  already true of the nonce path, not a new exposure. `user_not_allowlisted`
  is already documented (line 170); no change.

### Phase 3 — E1: split `auth_error()` into distinguishable reasons

Tests first:

- [x] Extend `login_nonce.rs` so each failure class asserts its own redirect
  target. Generalize `assert_auth_error` (`login_nonce.rs:101`) to take an
  expected reason; the existing call sites become the per-reason assertions.
  There are **seven**: `:231`, `:249`, `:264`, `:287`, `:315`, `:340`
  (`callback_with_a_blob_sealed_under_a_foreign_secret_is_rejected`) and
  `:388` (`a_session_token_is_not_a_login_state_cookie`) — the last two
  assert `restart`.
- [x] **Two dedicated tests for the 403/401 split** — the one mapping whose
  failure mode is silent and user-visible:
  - a signature-valid credential whose email matches no configured allowlist
    ⇒ redirect reason `denied`;
  - an undecodable or wrong-`aud` credential ⇒ redirect reason `restart`.
- [x] Assert the clear-login-state cookie is present on **every** new reason
  path (design constraint 3).
- [x] Add a `LoginScreen` case per user-facing message, in
  `hub-client/src/components/auth/LoginScreen.test.tsx` shape.
- [x] Client-side reason parsing has **three presence states**, each with a
  test:
  - parameter absent (`.get()` → `null`) ⇒ no error; normal sign-in prompt.
  - parameter present with an **empty value** (`/?auth_error`, what a pre-E1
    server or a cached redirect emits; `.get()` → `''`) ⇒ the `restart` copy,
    **not** nothing. A naive `if (reason)` truthiness check regresses this.
  - parameter present with an **unknown value** ⇒ the `restart` copy.

Then implement, server side:

- [x] Give the `auth_error` closure (`server.rs:1037`) a reason parameter and
  redirect to `/?auth_error=<reason>` instead of the bare `/?auth_error` it
  emits today. Collapse the **twelve post-E0 causes** into the four coarse
  reasons. Every cause must appear in this table, and a reasonless
  `auth_error()` call must not compile.

  | Reason | Causes | Exact user-facing copy |
  |--------|--------|------------------------|
  | `stale_client` | the E0 discriminator: no cookie **and** a nonce-less token | This app is out of date and updating. Please try again in a few minutes. |
  | `restart` | `login_state_missing` (nonce-bearing token), `nonce_mismatch`, `expired`, `tampered`, `kid_mismatch`, `token_nonce_missing`, `callback_csrf`, and the **401 family** from `authenticate_claims` (`jwt_decode:*`, `azp_or_iat_rejected`, `email_not_verified`) | Sign-in didn't complete. Please try again. |
  | `denied` | banned user; **`user_not_allowlisted`** (the 403 from `authenticate_claims`) | Sign-in failed. Your account is not authorized to access this hub. |
  | `server` | session mint failure | Something went wrong on the hub. Please try again shortly. |

  (Counted at *handler* granularity — branches `auth_callback` can actually
  distinguish. The 401 family is one cause because the handler sees a single
  `StatusCode`; its audit details are listed for orientation, not as rows to
  map.)

- [x] **Read the status; do not discard it.** `server.rs:1057` is
  `Err(_status) => return auth_error()` today, which is what makes the
  allowlist denial invisible to this mapping. It becomes:

  ```rust
  Err(status) => return auth_error(if status == StatusCode::FORBIDDEN {
      Reason::Denied      // allowlist miss: identity established, refused
  } else {
      Reason::Restart     // 401 family: no identity was established
  }),
  ```

  No new plumbing: `check_allowlists_for` already draws this line and
  documents it (`auth.rs:358-360` — "403, not 401: the user authenticated
  successfully but is not permitted").

  - **The 403 is `denied`.** Signature, `aud`, `azp` and `iat` all passed and
    the email is verified: an identity was established and then refused on
    policy. `--allowed-emails`/`--allowed-domains` is the standard admission
    gate (`session-auth-operations.md:141`), so this is the likelier of the
    two denials; mapping it to `restart` would put a permanently-refused user
    in a retry loop.
  - **The 401 family is `restart`, never `denied`.** No identity was
    established, so "not authorized" would be false. On client-ID drift
    (hub's configured `aud` diverging from the SPA's) every user in the
    deployment fails at this line at once; `denied` would tell all of them
    their account is not authorized, indistinguishable from a mass
    de-allowlisting.
  - **Known coarse mapping:** `email_not_verified` is a 401 where `restart`
    is also wrong (retrying will not verify an email), but `denied` is no
    better — no administrator can fix it either. Accepted deliberately;
    revisit only if it appears in production logs.
  - The reason strings are server-controlled constants; nothing
    user-supplied is ever reflected into the redirect.

Then implement, client side:

- [x] `hub-client/src/App.tsx:180` currently does
  `new URLSearchParams(window.location.search).has('auth_error')` — a
  boolean. Keep `.has('auth_error')` for **presence** and add
  `.get('auth_error')` for the **value**: on the bare `/?auth_error` a pre-E1
  server emits, `.get()` returns `''` (falsy), so presence and value must be
  read separately or the error message silently disappears.
- [x] `hub-client/src/components/auth/LoginScreen.tsx:17` takes
  `error?: boolean` and hardcodes the "not authorized" sentence at `:27`.
  Change the prop to carry the reason and map it to the copy above. The
  reason value is only ever a lookup key into the four fixed strings —
  **never render it**.
- [x] **Treat an unknown or empty reason as `restart`,** not as `denied`. An
  unrecognized reason means client/server skew, and the retry copy is the
  safer default: a false "try again" costs one retry, a false "not
  authorized" sends users to administrators. The parameter is also a
  craftable URL, so a `denied` fallback would let any `/?auth_error=anything`
  link render the alarming sentence. A real ban is never hidden — `denied` is
  in the client's vocabulary from day one, so skew only affects reasons added
  later.

### Phase 4 — E3: warn when secrets are auto-generated

Two hub instances each generating their own secret reject each other's
cookies and sealed blobs, surfacing as intermittent, self-healing sign-in
failure. The ops doc documents `QUARTO_HUB_SESSION_SECRET` as the
multi-instance mechanism, but nothing surfaces it at the moment it matters.

Tests first:

- [x] Assert the warning fires on the auto-generate branch.
- [x] Assert it stays silent for the env-var and existing-config branches.

Then implement:

- [x] `tracing::warn!` in `resolve_session_secret`'s generate-and-persist
  branch (`crates/quarto-hub/src/storage.rs:247-253`; fn at `:236`), stating
  that the secret is now pinned to this data directory and that
  **multi-instance deployments must set `QUARTO_HUB_SESSION_SECRET`**. The
  warning names the env var and the data directory — **never the secret value
  itself** ("token contents are never logged" applies here too).
- [x] The same for `resolve_server_secret` (branch at `storage.rs:217-223`;
  fn at `:206`).

**Priority note.** If production logs ever show `login_state_kid_mismatch`
dominating, this item is the fix for that incident and rises to `-p 0` ahead
of everything else here: `kid_mismatch` is the signature of two instances
with divergent auto-generated secrets.

### Phase 5 — verification

- [x] `cargo nextest run --workspace` — **10801 passed, 0 failed**, 197
  skipped.
- [x] `cargo xtask verify --skip-rust-tests` (the workspace run above covers
  that leg) — all 14 steps passed, including `hub-client` `build:all` and
  `test:ci`.
- [x] `cd hub-client && npm run test:ci` — **130 passed** across 21 files
  (18 of them the new/changed auth tests).
- [x] End-to-end, **partially**. See "End-to-end record" below for what ran,
  what was observed, and — importantly — the two legs that **cannot** run.
- [x] `hub-client/changelog.md` — two-commit workflow, hash from the first
  commit (CLAUDE.md).

## End-to-end record

### What ran, and what was observed

**Server, through the real `hub` binary.** New script
`scripts/hub-auth-error-reasons-e2e.mjs`, 13/13 checks passing:

```
cargo build --bin hub
node scripts/hub-auth-error-reasons-e2e.mjs
```

```
PASS  POST /auth/callback is routed for a Google issuer — status=303
PASS  CSRF pair mismatch -> /?auth_error=restart — location=/?auth_error=restart
PASS  CSRF field absent entirely -> /?auth_error=restart — location=/?auth_error=restart
PASS  undecodable credential -> /?auth_error=restart — location=/?auth_error=restart
PASS  csrf: no session cookie minted / login-state cookie cleared
PASS  jwt:  no session cookie minted / login-state cookie cleared
PASS  callback_csrf is in the audit log (was silent before E2)
        WARN quarto_hub::audit: action="auth_fail" outcome="deny"
             credential_kind="cookie" detail="callback_csrf"
PASS  the jwt_decode detail survives, unburied by a blanket emit
        INFO quarto_hub::audit: action="auth_fail" outcome="deny"
             credential_kind="unknown" detail=jwt_decode:JWT error: InvalidToken
PASS  startup warns that it generated a session secret
PASS  startup warns that it generated a server secret
```

**E3, through the real binary,** on a fresh data dir — first start emits both
warnings, second start on the same dir is byte-for-byte silent, and neither
log contains the secret value:

```
first start  — 'generated a new' lines: 2
second start — 'generated a new' lines: 0
second start log size:        0 bytes
session secret appears in logs: 0
```

Observed text (ANSI stripped):

```
WARN quarto_hub::storage: generated a new session secret and persisted it to
  hub.json — it is now pinned to this data directory. Multi-instance
  deployments must set QUARTO_HUB_SESSION_SECRET to the same value on every
  instance; otherwise instances reject each other's session cookies.
  hub_dir=/tmp/hub-e3-Ci6s
```

**Client copy, through the real production bundle.** All four sentences are
present in an auth-enabled `vite build`, matching the E1 table verbatim
(`1` = number of bundle files containing the string):

```
VITE_GOOGLE_CLIENT_ID=verify.apps.googleusercontent.com npx vite build --outDir <tmp>
1    This app is out of date and updating. Please try again in a few minutes.
1    Sign-in didn't complete. Please try again.
1    Sign-in failed. Your account is not authorized to access this hub.
1    Something went wrong on the hub. Please try again shortly.
0    (superseded) This version of the app is out of date. Please reload the page and try again.
```

**Re-run 2026-07-30** after the `stale_client` copy was shortened; the first
record of this leg showed the superseded sentence, so it was re-measured
rather than edited. The `0` line is an added check — the old copy is absent,
so no bundle carries both. The build's PWA leg reported **166 precache
entries, 67138 KiB**, against the 63 MB recorded in
`2026-07-30-hub-client-sw-precache-and-update.md`'s Measurements block.

Note the default `npm run build` **omits** these strings entirely, and that
is correct: `AUTH_ENABLED = !!import.meta.env.VITE_GOOGLE_CLIENT_ID`, so
without that variable vite statically eliminates the whole `LoginScreen`
branch as dead code. Grepping plain `dist/` for the copy is therefore not a
valid check.

### What could NOT be run, and why

Two legs the plan asked for are **not reachable**, and no amount of scripting
fixes it:

1. **`stale_client` and `denied` against the real binary.** Both require
   first *passing* `authenticate_claims`, i.e. presenting a credential
   Google actually signed for our audience. Forging one is precisely what
   the hub exists to prevent. They are covered instead by
   `login_nonce.rs`, which drives the real router over real HTTP against a
   mock provider whose JWKS the hub genuinely fetches — bypassing only CLI
   parsing and OIDC discovery, both of which the new script covers.

2. **Extending `scripts/hub-sliding-sessions-e2e.mjs` as written.** That
   script mocks the IdP on localhost, which yields a **Generic** provider,
   and `POST /auth/callback` is registered only for a **Google** provider
   (`AuthConfig::uses_form_post_callback`, with the provider derived from
   the issuer being exactly `https://accounts.google.com`). Verified
   empirically, not inferred — a mock-IdP hub answers the callback with
   **404**. Hence the separate script, which uses Google's real public
   discovery/JWKS to make the route exist and needs outbound network.
   Forcing the provider via a new CLI flag would mean adding production
   surface for test convenience; not done.

3. **No browser leg.** The four sentences were verified in the production
   bundle and in jsdom (`LoginScreen.test.tsx`), **not** in a real browser —
   browser tooling was unavailable this session. The untested surface is
   narrow (React rendering a string chosen by a pure lookup), but it is
   untested, and this is a hub-client change, so CLAUDE.md's rule applies:
   stating it rather than inferring success.

## Sequencing

E0 (Phase 1) before E1 (Phase 3): the `stale_client` reason is not
expressible until the discriminator exists. E2 is independent, but it is
small and sits in the same handler, so landing it before E1's larger refactor
keeps the diffs legible.

## Not in scope

- Any change to *when* the callback fails. This plan changes only what the
  server reports and what the user is told; the failure conditions are
  untouched.
- A client build-id / `/version` skew endpoint (the SPA sending the `gitInfo`
  commit hash from `hub-client/vite.config.ts:12` for the server to flag a
  mismatch). Still rejected, but not for the reason first given: nginx serves
  the SPA in production, so the hub does not know which client build is
  canonical — and a skew endpoint carries the *same* one-generation lag as
  `stale_client`, since acting on the server's answer needs client code the
  stale client does not have. No better positioned, rather than redundant.

## Current state (verified against `e86a9275`)

Server, `crates/quarto-hub/src/server.rs`:

- `auth_error` redirects to a bare `/?auth_error` (`:1037-1042`).
- The CSRF path (`:1049`) emits no audit event. `validate_callback_csrf`
  (`:891-919`) returns a bare `bool` and emits nothing on any of its three
  false-returning branches.
- The `login_state_missing` early return (`:996-997`) precedes any read of
  `claims.nonce`, and its return string double-prefixes the emit at `:1071`.
- The nonce-failure audit event (`:1063-1071`) is the shape the CSRF path
  should copy.
- `Err(_status) => return auth_error()` at `:1057` discards the
  `authenticate_claims` status.
- The `/auth/callback` route is registered only when `auth_config` is `Some`
  (`:1661-1665`), so `auth_disabled` and `missing_credential` are unreachable
  from the callback.

Auth and audit:

- The email allowlist is enforced inside `authenticate_claims` →
  `check_allowlists` (`auth.rs:317`, `check_allowlists_for` at `:324-362`):
  **403** for a verified email matching no list, **401** for an unverified
  email.
- `authenticate_claims_for_kind` (`context.rs:571-661`) has no silent `Err`
  path — all five returns emit an `auth_fail` event first: `auth_disabled`
  (`:571-581`), `missing_credential` (`:583-593`), `jwt_decode:<err>`
  (`:603-615`), `azp_or_iat_rejected` (`:620-638`), and
  `user_not_allowlisted` / `email_not_verified` (`:640-660`).
- No test asserts any of the six login-state detail strings.
- `assert_auth_error` has seven call sites (`:231`, `:249`, `:264`, `:287`,
  `:315`, `:340`, `:388`). `snapshot_events` lives in
  `tests/integration/support.rs:89` and is already imported by
  `login_nonce.rs:26`.

Client:

- `hub-client/src/App.tsx:180` uses `.has('auth_error')`.
- `LoginScreen.tsx:17` takes `error?: boolean` and hardcodes the "not
  authorized" sentence at `:27`.
  `hub-client/src/components/auth/LoginScreen.test.tsx` exists.
- The adjacent pre-flight failure ("Could not start sign-in. Please reload
  the page.", `GoogleAuthProvider.tsx:64-66`) is a different, pre-callback
  path, untouched by this plan.

Ops doc, `dev-docs/quarto-hub/session-auth-operations.md`: audit quick
reference starts at line 163; `login_state_missing` described at line 77;
`user_not_allowlisted` at line 170; allowlist removal semantics at line 141.

Cause inventory, exhaustively: `callback_csrf`, the `authenticate_claims`
failures (401 family + the 403 `user_not_allowlisted`),
`login_state_missing`, `kid_mismatch`, `tampered`, `expired`,
`token_nonce_missing`, `nonce_mismatch`, `user_banned`, mint failure —
**eleven**. E0 splits `login_state_missing` in two, giving the **twelve**
E1's table maps. (`NonceCheck::Skipped` is insecure-mode-only and not a
failure path.)

## Braid strands

Filed 2026-07-30, all `discovered-from` the H2 strand `bd-uqjiac5a`:

- `bd-htis60s7` — E0+E1, `-p 1` (feature). Together they make every future
  auth failure diagnosable.
- `bd-sxnfoefn` — E2, `-p 2` (task).
- `bd-sx7k3vid` — E3, `-p 2` (task); rises to `-p 0` on any production
  `kid_mismatch`.

Implemented together on branch `braid/bd-htis60s7-auth-error-reasons` — one
branch rather than three, because the three items touch the same handler and
the same ops doc, and splitting them would have made the diffs harder to read
rather than easier.
