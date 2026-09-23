# Restore universal hub login nonce enforcement (GH #564, reverse PR #446)

## Overview

PR #446 (commit `0adbe8288`, 2026-07-31) temporarily made `POST /auth/callback`
accept nonce-less ID tokens so an already-deployed pre-nonce SPA could keep
logging in. GH #564 ("hub: Restore universal nonce enforcement") tracks
undoing that now that the nonce-aware SPA (the solution to #447) has shipped
and deployed. Tracking strand: `bd-mc00s2ws`.

The fix is a **faithful reversal** of PR #446's code and test changes (the
user confirmed faithful reversal over a uniform "all nonce-less →
`stale_client`" rule). After the change:

| Callback shape | Result |
|---|---|
| Nonce-less token, no login-state cookie | redirect `?auth_error=stale_client`, audit `login_state_stale_client` |
| Nonce-less token, valid login-state cookie | redirect `?auth_error=restart`, audit `login_state_token_nonce_missing` |
| Nonce-bearing token, no cookie | `restart` / `login_state_missing` (unchanged) |
| Nonce mismatch / tampered / expired blob / foreign secret | `restart` (unchanged) |
| `--allow-insecure-auth` | check skipped with WARN (unchanged) |

The hub-client needs **no changes**: `stale_client` is already mapped in
`hub-client/src/auth/authError.ts` ("This app is out of date and updating…")
with coverage in `authError.test.ts` and `LoginScreen.test.tsx`. The MCP e2e
auth flow uses `--allow-insecure-auth` and is unaffected.

Hand-applied reversal, **not** `git revert 0adbe8288` — a revert would have
deleted the completed historical plan
`claude-notes/plans/2026-07-31-temporarily-disable-hub-login-nonce-check.md`,
which stays.

## Work Items

### Phase 1 — Tests first (TDD)

- [x] Restored the pre-#446 module-doc wording (dropped the "temporarily
      accepted" note) in `crates/quarto-hub/tests/integration/login_nonce.rs`.
- [x] Replaced `a_nonceless_token_is_accepted_even_with_a_login_cookie` with
      `callback_with_a_token_carrying_no_nonce_is_rejected` (nonce-less token
      **with** a valid login-state cookie → `restart`).
- [x] Replaced `a_nonceless_token_without_a_cookie_mints_a_session_for_an_old_spa`
      with `a_nonceless_token_without_a_cookie_audits_as_stale_client`
      (→ `stale_client` + audit `login_state_stale_client`).
- [x] **Red step watched**: both restored tests failed against the bypass
      (`left: "/"`, `right: "/?auth_error=stale_client"` / `"restart"` — the
      bypass minted sessions).

### Phase 2 — Reverse PR #446 in `crates/quarto-hub/src/server.rs`

- [x] Restored the `LOGIN_STATE_STALE_CLIENT` doc comment.
- [x] Restored the `check_login_nonce` doc comment (the
      `--allow-insecure-auth` skip wording).
- [x] Deleted the `TODO(bd-mc00s2ws)` bypass block (`claims.nonce.is_none()
      → NonceCheck::Skipped`).
- [x] Restored the cookie-absent branch discrimination:
      `Failed(if claims.nonce.is_none() { LOGIN_STATE_STALE_CLIENT } else {
      "missing" })` with the two-readings comment.
- [x] Restored the `auth_callback` comment at the error-mapping site.
- [x] Green step: `cargo nextest run -p quarto-hub` — **474/474 pass**,
      including both restored tests and
      `insecure_mode_skips_enforcement_with_a_warning`.
- [x] Faithfulness check: `git diff 0adbe8288^ -- <the two files>` shows
      **zero** nonce-region differences vs pre-#446 (only the later,
      unrelated `/auth/me` credential field and shutdown-message changes).

### Phase 3 — Workspace verification

- [x] `cargo xtask lint` — clean (1148 files).
- [x] `cargo fmt --all -- --check` — clean.
- [x] `RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets -- -D warnings` — clean.
- [x] `RUSTFLAGS="-D warnings" cargo build --workspace` — clean.
- [x] `cargo nextest run --workspace` (via two runs, log in
      `target/nextest-workspace.log`): **14325 passed (1 leaky), 47 failed,
      303 skipped**. All 47 failures are environmental, none touch
      quarto-hub:
      - 46 × `Q-20-2 Pandoc Version Too Old` — this machine has pandoc
        3.7.0.2, below the 3.11 floor introduced in `e8242e6ee`
        (2026-09-18). Failing modules: `render_pandoc_formats_e2e`,
        `pandoc_typst_writer`, `pandoc_typst_compile`, `pandoc_render_to_file`,
        `pandoc_goldens`, `conditional_content_pandoc_e2e`,
        `website_post_render_format_gate`, `pandoc_execute_defaults`,
        `project_pandoc_gate_e2e`, plus the 103 `assert_pandoc_available`
        L-tier tests (characterized separately, same floor panic).
      - 1 × `typst must be on PATH` (typst not installed).
- [ ] `cargo xtask verify --skip-hub-build` — **blocked by environment**:
      the verify pandoc preflight (no skip flag) hard-fails before any leg
      runs. Its constituent legs were run manually instead (above). Full
      verify needs pandoc ≥ 3.11 installed.

### Phase 4 — Bookkeeping

- [x] Plan recorded here (project convention); working draft at
      `.posit/assistant/plans/2026-09-23-1033-fix-564-enforce-auth-nonce-reverse-pr-446.md`.
- [x] `braid comment bd-mc00s2ws` summarizing the change.
- [x] Committed the two source files + this plan. **Not pushed** (policy).
- [ ] PR with `Closes #564`; on merge, `braid close bd-mc00s2ws --reason
      "Universal nonce enforcement restored (reversal of PR #446)"`.

## Details

### Behavior restored

`check_login_nonce`'s cookie-absent branch again distinguishes the two
readings using the signature-validated token's own `nonce` claim: nonce-less
⇒ `stale_client` (no current client produces this; a reload fixes it),
nonce-bearing ⇒ `missing` (cookie lost in transit or replay). The
`stale_client` detail maps to `AuthErrorReason::StaleClient` in
`auth_callback` (retained by #446), and the hub-client renders it as the
"app is out of date" message. The `NonceCheck::Skipped` doc ("insecure mode
only") is accurate again now that the bypass is gone.

### Security posture restored

Without the nonce binding, a captured Google ID token could be replayed to
the callback for its full (~1 h) validity from a browser that never ran the
pre-flight. The sealed-cookie nonce binding ties the token to *this*
browser's login attempt.

### End-to-end note

The integration tests drive real HTTP through the axum router against a mock
OIDC provider — that is the end-to-end auth path for this change. No
browser-level check was performed: it would require a deployed hub plus the
old SPA, which is exactly what this removes.

### Out of scope

- `/auth/session` (Generic provider JSON mint) stays replay-able within token
  validity — an accepted boundary documented in the test module doc.
- No hub-client / TypeScript changes; no docs changes.
