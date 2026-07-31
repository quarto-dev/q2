# Temporarily disable hub login nonce verification

## Overview

Permit an already deployed SPA, which does not return the newer login nonce,
to authenticate with the current hub server. A callback that presents a nonce
continues to require an exact match against the sealed login-state cookie.
Universal nonce enforcement must be restored after the old SPA is no longer in
use.

**Tracking strand:** `bd-mc00s2ws`

## Checklist

- [x] Write a regression test proving a nonce-less old SPA callback mints a session.
- [x] Observe the regression test fail while nonce enforcement is enabled.
- [x] Temporarily bypass nonce verification with a TODO linked to `bd-mc00s2ws`.
- [x] Restore nonce-bearing rejection tests.
- [x] Verify nonce-bearing rejection tests fail under the broad bypass.
- [x] Restrict the bypass to nonce-less tokens.
- [x] Retain the `stale_client` callback reason for future compatibility branches.
- [x] Re-run the focused authentication test to verify old SPA compatibility.
- [x] Run the complete hub test suite (`442 passed`).
- [ ] Run workspace regression and strict verification. Skipped before this
  commit at the user's request; focused and complete hub verification passed.

## Details

The bypass is intentionally restricted to `check_login_nonce` in
`crates/quarto-hub/src/server.rs`; signature, issuer, audience, expiry, CSRF,
allowlist, ban, and session-mint validation remain unchanged.
