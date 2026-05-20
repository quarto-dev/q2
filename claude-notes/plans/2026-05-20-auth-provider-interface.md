# 2026-05-20 — Isolate GIS coupling behind an AuthProvider interface

## Overview

Before this refactor, `@react-oauth/google` was referenced directly in
five production files (`main.tsx`, `LoginScreen.tsx`, `useAuth.ts`,
`authService.ts`, plus `App.tsx`'s `AUTH_ENABLED` gate keyed off
`VITE_GOOGLE_CLIENT_ID`). The refactor introduces a thin
`AuthProvider` interface that mediates every IdP-specific call, and
migrates the existing Google Identity Services (GIS) usage to a single
`GoogleAuthProvider` implementation behind it.

**The goal is preparation, not behavioural change.** The running app
authenticates against Google via GIS exactly as it did before
(`GoogleLogin` redirect button + `useGoogleOneTapLogin` silent renewal
+ `googleLogout`). What changes is that adding a second IdP backend
later (e.g. an `OidcAuthProvider` over `oidc-client-ts`) becomes a
self-contained addition: write a new implementation of the interface,
swap one factory call in `main.tsx`, done. No churn in `LoginScreen`,
`useAuth`, or any consumer.

### Goals

- Define an `AuthProvider` interface that captures the three
  IdP-coupled operations: interactive sign-in, silent renewal,
  IdP-side sign-out.
- Implement `GoogleAuthProvider` wrapping the existing GIS calls.
- Migrate every consumer to depend on `AuthProvider` via React context
  rather than on `@react-oauth/google` directly.
- Preserve current behaviour bit-for-bit: same redirect flow, same
  One Tap renewal, same logout shape, same `AUTH_ENABLED` semantics.

### Non-goals

- **No** second IdP implementation. That is separate, future work;
  this refactor only adds the boundary.
- **No** switch to Authorization Code + PKCE. Out of scope.
- **No** change to the hub-side validator, the cookie shape, the
  `/auth/callback` / `/auth/refresh` / `/auth/me` / `/auth/actor`
  endpoints, or the `useAuth` public API. This refactor is internal
  to hub-client.
- **No** change to test coverage scope. Existing tests for `useAuth`
  and `authService` continue to pass after migration; only the mock
  shape changes (`AuthProvider` context instead of
  `@react-oauth/google` directly).

## Pre-refactor baseline

Direct `@react-oauth/google` imports as of 2026-05-20 (line numbers
are pre-refactor):

| File | Symbol used | What it did |
|---|---|---|
| `src/main.tsx:3,35` | `GoogleOAuthProvider` | Wraps app, loads GIS SDK with `clientId` |
| `src/components/auth/LoginScreen.tsx:15,32` | `GoogleLogin` | Renders sign-in button, `ux_mode="redirect"`, `login_uri=/auth/callback` |
| `src/hooks/useAuth.ts:29,82` | `useGoogleOneTapLogin` | Silent renewal via `auto_select`, gated by `disabled` |
| `src/services/authService.ts:10,71` | `googleLogout` | Called inside `logout()` after the server-side POST |
| `src/App.tsx:63` | (indirect) | `AUTH_ENABLED = !!VITE_GOOGLE_CLIENT_ID` — gates 7 conditionals in App.tsx |

Tests with direct `@react-oauth/google` mocks:

- `src/hooks/useAuth.test.ts:20-24` — mocked `useGoogleOneTapLogin`
- `src/services/authService.test.ts:13-18` — mocked `googleLogout`

## Final state

Direct `@react-oauth/google` imports in production (post-refactor):

| File | Symbol used | Role |
|---|---|---|
| `src/main.tsx` | `GoogleOAuthProvider` | SDK loader; outermost wrap. Stays here because `useGoogleOneTapLogin` needs the GIS context in tree. |
| `src/auth/GoogleAuthProvider.tsx` | `GoogleLogin`, `useGoogleOneTapLogin`, `googleLogout` | The dedicated wrapper; the *only* file that touches GIS APIs by name. |

Direct `@react-oauth/google` imports in tests: only
`src/auth/GoogleAuthProvider.test.tsx` (mocks the lib to unit-test
the wrapper itself — that is testing the boundary, not breaching it).
`src/hooks/useAuth.test.tsx:5` contains a JSDoc *mention* of the
package name in its file-header narrative; that is documentation, not
coupling.

Every other consumer (`LoginScreen`, `useAuth`, `authService`) reads
the IdP behind the `AuthProvider` interface via `useAuthProvider()`.

## Interface design

> **Updated by Phase 8:** the two files described below were merged
> into a single `src/auth/AuthProvider.tsx`, which also exports a
> `noopAuthProvider` (the default context value, used when no IdP is
> configured). `useAuthProvider()` now returns a non-nullable
> `AuthProvider`. `SignInButtonProps.onError` has been removed.
> The shape of `AuthProvider`, `useSilentRenewal`, and the context
> API is otherwise unchanged.

The interface lives in `src/auth/types.ts` and is consumed through a
context defined in `src/auth/AuthProviderContext.tsx`.

```ts
// src/auth/types.ts

import type { ComponentType } from 'react';

export interface AuthProvider {
  /**
   * React component that renders the interactive sign-in affordance
   * (today: the GIS "Sign in with Google" button in redirect mode).
   *
   * The component is responsible for the entire sign-in initiation —
   * including the redirect to the IdP. Success is observed by the SPA
   * via the existing `GET /auth/me` polling on the next page load
   * (today's behaviour, unchanged).
   */
  readonly SignInButton: ComponentType<SignInButtonProps>;

  /**
   * Hook that enables/disables silent credential renewal.
   *
   * When `enabled` becomes true, the provider attempts to obtain a
   * fresh JWT without user interaction and invokes `onCredential` with
   * the JWT. If renewal fails or no IdP session exists, `onError` is
   * invoked. Both callbacks are one-shot per enabled→success transition;
   * the consumer is expected to set `enabled` back to false in the
   * callback.
   *
   * Providers without a silent-renewal capability MUST still implement
   * the hook as a no-op: enabling it never invokes either callback.
   * Consumers detect that scenario via timeout, not via a return value
   * — keeps the interface symmetric across capabilities.
   */
  useSilentRenewal(opts: SilentRenewalOpts): void;

  /**
   * Best-effort IdP-side sign-out. For Google this calls
   * `googleLogout()` which revokes the GIS session (no network round
   * trip). Synchronous on purpose — callers should not block UI on it.
   */
  signOut(): void;
}

export interface SignInButtonProps {
  /**
   * Server endpoint the IdP credential should land at. For GIS-redirect
   * mode this is wired into `<GoogleLogin login_uri={...} />`; for a
   * future Code+PKCE provider it would be the redirect URI the SPA
   * registers with the IdP. Either way, the hub side already exposes
   * `/auth/callback`.
   */
  loginUri: string;

  /** Called when the IdP signals an unrecoverable error before redirect. */
  onError?: () => void;
}

export interface SilentRenewalOpts {
  enabled: boolean;
  onCredential: (jwt: string) => void;
  onError: () => void;
}
```

Initialization wrapper:

```tsx
// src/auth/AuthProviderContext.tsx

import { createContext, useContext, type ReactNode } from 'react';
import type { AuthProvider } from './types';

const AuthProviderContext = createContext<AuthProvider | null>(null);

export function AuthProviderRoot({
  provider,
  children,
}: {
  provider: AuthProvider | null;
  children: ReactNode;
}) {
  return (
    <AuthProviderContext.Provider value={provider}>
      {children}
    </AuthProviderContext.Provider>
  );
}

/** Returns null when auth is disabled (no provider configured). */
export function useAuthProvider(): AuthProvider | null {
  return useContext(AuthProviderContext);
}
```

A `null` provider models the existing `AUTH_ENABLED=false` mode (no
`VITE_GOOGLE_CLIENT_ID` configured). Consumers branch on `null`
exactly as they branch on `AUTH_ENABLED` today.

### Why this shape

- **`SignInButton` as a component, not a `signIn()` method.** GIS's
  `<GoogleLogin>` component is the integration point with the GIS
  SDK; it owns its own DOM, its own one-time JS click handler, its
  own styling defaults. Reducing it to an imperative `signIn()` would
  either require us to re-implement the button or to render an
  invisible `<GoogleLogin>` somewhere and trigger it programmatically.
  Both are worse than letting the provider decide its own DOM.

- **`useSilentRenewal` as a hook, not a method.** Today's silent
  renewal *is* `useGoogleOneTapLogin`, which must be called from
  component scope and re-runs when its options change. Wrapping it in
  an imperative API would force us to re-implement that lifecycle
  ourselves. A non-Google provider's silent renewal would likewise
  typically need component lifecycle (timers, iframe lifecycle).

- **`signOut` as a method, not a component.** No DOM involved; it's
  a one-shot SDK call (`googleLogout()`) invoked from
  `useAuth.logout()`.

- **JWT delivery is *not* in the interface for the redirect path.**
  Today, GIS-redirect-mode delivers the credential server-side via
  form_post to `/auth/callback` — the SPA never sees it. The
  interface preserves this: `SignInButton` initiates the flow and the
  credential lands on the server out-of-band. Only the
  `useSilentRenewal` path delivers a JWT to JS, because One Tap *does*
  return it to JS. A future Code+PKCE provider would deliver the
  credential to JS in both paths; we can extend `SignInButtonProps`
  with an optional `onCredential` then without breaking GIS consumers
  (just a wider union of patterns, both supported).

- **`AUTH_ENABLED` stays in `App.tsx`.** It could be derived from
  `useAuthProvider() !== null`, but App.tsx evaluates it at module
  scope (line 63) outside the React tree. Moving the gate inside the
  tree is a separate refactor of 7 call sites in App.tsx; out of
  scope here. The env-var name `VITE_GOOGLE_CLIENT_ID` stays for the
  same reason — renaming env vars is a deploy-time change we don't
  need to take on here.

### Project convention: file layout

Tests live co-located with source (e.g. `useAuth.test.tsx` next to
`useAuth.ts`), matching the existing project pattern — there is no
`__tests__/` directory convention in this codebase. The
`MockAuthProvider` test double lives next to the interface at
`src/auth/MockAuthProvider.tsx` (with the `Mock` prefix as the
signal that production code shouldn't import it) rather than under a
`src/test-utils/` subdirectory; this keeps the interface's
verification scaffolding next to the interface itself.

## Phases

### Phase 1 — Add the interface, context, and a test double

TDD ordering: write the context test first, then implement.

- [x] `src/auth/types.ts` declares `AuthProvider`,
      `SignInButtonProps`, `SilentRenewalOpts` exactly as specified
      above.
- [x] `src/auth/AuthProviderContext.tsx` provides `AuthProviderRoot` +
      `useAuthProvider`.
- [x] `src/auth/MockAuthProvider.tsx` exposes `createMockAuthProvider()`
      returning `{ provider, lastSilentRenewalOpts,
      signInButtonClicks, lastLoginUri, signOutCalls, reset() }`.
      The captured `lastSilentRenewalOpts` lets tests synchronously
      trigger `onCredential` / `onError` to simulate IdP responses.
- [x] `src/auth/AuthProviderContext.test.tsx` covers three cases:
  - `useAuthProvider returns null when no AuthProviderRoot is mounted`
  - `useAuthProvider returns the provided value inside AuthProviderRoot`
  - `useAuthProvider returns null when AuthProviderRoot is mounted with provider={null}`
    (the explicit-disabled mode that Phase 3's `main.tsx` uses when
    `VITE_GOOGLE_CLIENT_ID` is unset).

### Phase 2 — Implement GoogleAuthProvider

TDD ordering: failing test first against the wrapper's contract,
then implement.

- [x] `src/auth/GoogleAuthProvider.test.tsx` covers eight cases.
      Mocks `@react-oauth/google` at module scope to capture
      `GoogleLogin` props and `useGoogleOneTapLogin` opts:
  - `SignInButton renders GoogleLogin in redirect mode with the given loginUri`
  - `SignInButton forwards onError prop to GoogleLogin` — locks in
    the optional-onError-prop wiring through to the underlying GIS
    button.
  - `useSilentRenewal({enabled: true}) calls useGoogleOneTapLogin with auto_select:true and disabled:false`
  - `useSilentRenewal({enabled: false}) calls useGoogleOneTapLogin with disabled:true`
  - `useSilentRenewal forwards onCredential when one-tap success carries a credential`
  - `useSilentRenewal forwards onError when one-tap success carries no credential`
    — Phase 5 relies on this mapping. Pre-refactor, GIS's "success
    with no credential" response dropped the consumer out of the
    refreshing state via a dedicated branch in
    `useAuth.ts`; collapsing it into `onError` at the provider layer
    keeps the consumer's branch structure simple while preserving
    the same state-machine outcome.
  - `useSilentRenewal forwards onError on one-tap error`
  - `signOut() calls googleLogout()`

- [x] `src/auth/GoogleAuthProvider.tsx` exports
      `createGoogleAuthProvider(): AuthProvider` — no parameters.
      `<GoogleOAuthProvider clientId={...}>` lives in `main.tsx`
      (the wrap-once decision below), and GIS hooks/components read
      `clientId` from React context. The factory therefore has no
      use for it and does not produce its own provider scope. Future
      config (scopes, prompt mode) can be added when actually needed.

Implementation constraint: `useGoogleOneTapLogin` requires
`<GoogleOAuthProvider>` in the React tree above it. The migration in
Phase 3 keeps that wrap in `main.tsx`; the GIS SDK boot lifecycle is
not moved inside `GoogleAuthProvider`.

### Phase 3 — Migrate `main.tsx`

- [x] Construct the provider:
      ```tsx
      const authProvider = GOOGLE_CLIENT_ID ? createGoogleAuthProvider() : null;
      ```
- [x] Wrap the tree:
      ```tsx
      <GoogleOAuthProvider clientId={GOOGLE_CLIENT_ID || 'disabled'}>
        <AuthProviderRoot provider={authProvider}>
          {root}
        </AuthProviderRoot>
      </GoogleOAuthProvider>
      ```
      `<GoogleOAuthProvider>` remains the outermost wrap because
      `useGoogleOneTapLogin` (called from
      `GoogleAuthProvider.useSilentRenewal` in Phase 5) needs it in
      tree.
- [x] Unit-verify: full test suite green; `tsc` reports 0 errors in
      `src/auth/` + `src/main.tsx`.
- [ ] **e2e/dev-server boot test punted to Phase 7.** `main.tsx` is
      the entry point — changes here are only meaningfully validated
      by booting the app. Phase 7 owns that walkthrough.

### Phase 4 — Migrate `LoginScreen.tsx`

- [x] `src/components/auth/LoginScreen.test.tsx` is new (no
      LoginScreen test existed before). 4 cases:
  - `renders the provider's SignInButton with loginUri = origin + /auth/callback`
    — driven by `createMockAuthProvider()`; asserts the mock's
    SignInButton mounted (testid) and that `lastLoginUri` was
    threaded through.
  - `does not render a SignInButton when the provider is null` —
    defensive; the call site in `App.tsx:554` gates LoginScreen
    rendering on `AUTH_ENABLED`, so this path is unreachable in
    practice, but the component is well-defined either way.
  - `renders the default copy when error is false/absent`
  - `renders the error copy when error={true}` — regression-guards
    the pre-existing `error` prop UI branch.
- [x] Replaced the direct `<GoogleLogin>` JSX with
      `{provider && <provider.SignInButton loginUri={...} />}`. The
      surrounding modal chrome (logo, title, copy) is unchanged; only
      the button itself moves behind the provider boundary.
- [x] Removed `import { GoogleLogin } from '@react-oauth/google'`.

### Phase 5 — Migrate `useAuth.ts`

- [x] Renamed test file `src/hooks/useAuth.test.ts` →
      `useAuth.test.tsx` because the `AuthProviderRoot` wrapper
      requires JSX.
- [x] Replaced `vi.mock('@react-oauth/google', ...)` with a
      `MockAuthProvider` mounted via `AuthProviderRoot` in
      `renderHook`'s `wrapper` option. The mechanical translations:
  - `oneTapCallbacks.onSuccess?.({ credential: 'foo' })` →
    `mockProvider.lastSilentRenewalOpts?.onCredential('foo')`
  - `oneTapCallbacks.onError?.()` →
    `mockProvider.lastSilentRenewalOpts?.onError()`
  - `oneTapCallbacks.disabled === true` →
    `mockProvider.lastSilentRenewalOpts?.enabled === false`
- [x] Added 3 tests beyond the existing 17:
  - `works when no provider is mounted (auth-disabled mode)` — the
    null-provider branch is exercised by App.tsx whenever
    `VITE_GOOGLE_CLIENT_ID` is unset; locks in that the hook stays
    callable.
  - `calls provider.signOut() during logout` — locks in the new
    signOut wiring at the consumer.
  - `logout works without a provider (no signOut to call)` —
    defends against a regression where `provider?.signOut()` becomes
    `provider.signOut()`.
- [x] In `useAuth.ts`:
  - Added `const provider = useAuthProvider();` at top of `useAuth`.
  - Replaced `useGoogleOneTapLogin({...})` with
    `provider?.useSilentRenewal({...})`. The interface's
    `onCredential` handles "got a JWT"; the provider collapses "GIS
    reported success with no credential" into `onError` (per
    Phase 2), so the consumer no longer needs an inner
    `if (response.credential)` branch.
  - In `logout`, added `provider?.signOut()` before `setAuth(null)`,
    with `provider` added to the `useCallback` dependency list.
    This synchronous IdP revocation slightly improves on the
    pre-refactor sequence — see § Logout sequencing below.
  - Removed `import { useGoogleOneTapLogin } from '@react-oauth/google'`.
  - Updated the head JSDoc to refer to the "active AuthProvider"
    instead of "Google One Tap" in three places. The mechanism
    description (15-min buffer, throttling rationale,
    visibility-aware refresh, 401-triggered refresh) is unchanged.

#### Logout sequencing

Pre-refactor, `googleLogout()` sat inside `authService.logout()`
*after* `await fetch('/auth/logout')`, while `setAuth(null)` ran
synchronously in the outer `useAuth.logout()`. So the actual
execution order was:

```
useAuth.logout() →
  serverLogout()    // async fetch then googleLogout() in the await chain
  setAuth(null)     // sync, runs BEFORE the chain resolves
```

Post-refactor:

```
useAuth.logout() →
  serverLogout()       // async fetch POST, no IdP work
  provider?.signOut()  // sync; runs immediately
  setAuth(null)        // sync
```

The IdP session is now revoked immediately rather than after the
server roundtrip. This is a small intended improvement, not a
regression: there is no observable downside, and the "best-effort
server logout" semantics are preserved — `setAuth(null)` still runs
regardless of whether the fetch succeeds.

### Phase 6 — Migrate `authService.ts`

- [x] Updated `src/services/authService.test.ts`:
  - Removed the `vi.mock('@react-oauth/google', ...)` block and the
    `import { googleLogout }` / `mockGoogleLogout` plumbing.
  - Trimmed the logout test title and body to assert only on the
    server-side POST. IdP-side sign-out is covered by
    `GoogleAuthProvider.test.tsx` (`signOut() calls googleLogout()`)
    and by `useAuth.test.tsx` (`calls provider.signOut() during logout`).
- [x] In `authService.ts`:
  - Removed `import { googleLogout } from '@react-oauth/google'`.
  - Removed the trailing `googleLogout()` call from `logout()`; the
    function is now purely the server-side POST.
  - Updated the head JSDoc and the `logout` JSDoc to drop the
    "revoke Google session" wording; pointed at the AuthProvider
    boundary as the home for IdP-side signout.

Note on the TDD red phase: `authService.test.ts` runs in the node
environment (no `@vitest-environment jsdom`), so once the
`@react-oauth/google` mock was removed but `authService.ts` still
imported `googleLogout`, the real `googleLogout` ran and crashed on
`ReferenceError: window is not defined`. Removal refactors usually
don't produce a literal red phase, but the missing jsdom delivered
one here — a useful confirmation that the test fully exercised the
removal.

### Phase 7 — Verification and dependency audit

- [x] **`@react-oauth/google` production-import audit.** Grep
      against `hub-client/src/` excluding `*.test.{ts,tsx}` returns
      exactly:
      ```
      src/main.tsx
      src/auth/GoogleAuthProvider.tsx
      ```
      Both are intended targets — `main.tsx` for the
      `<GoogleOAuthProvider>` SDK wrap, `auth/GoogleAuthProvider.tsx`
      for the wrapper. Test-file imports: only
      `auth/GoogleAuthProvider.test.tsx` (mocking the lib to
      unit-test the wrapper — appropriate).
      `useAuth.test.tsx:5` mentions `@react-oauth/google` in a JSDoc
      comment ("MockAuthProvider in place of `@react-oauth/google`")
      — documentation, not coupling.
- [x] **Test verify (this session).** `npm run test:ci` from
      `hub-client/` runs three suites, all green:
  - Unit (`vitest run`): **38 test files, 554 tests** passing.
    Progression across the refactor:
    pre-baseline 536 → +3 Phase 1 → +8 Phase 2 → +4 Phase 4 →
    +3 net Phase 5 (20 new − 17 deleted from `useAuth.test.ts`) →
    +0 Phase 6 = 554.
  - Integration: **8 test files** passing.
  - WASM: **14 test files, 79 tests** passing (smoke-all: 60
    passed, 24 skipped, 0 failed).
- [ ] **`npm run build:all` — not completed in this session; failures
      are pre-existing and not from this refactor.** Honest report
      per CLAUDE.md:
  - `tsc -b` against `tsconfig.app.json` reports **0 errors in any
    file this refactor touched** (`src/auth/`, `src/main.tsx`,
    `src/components/auth/`, `src/hooks/useAuth.ts`,
    `src/services/authService.ts`).
  - The 139 errors `tsc -b` does report are all `TS2307 Cannot find
    module '@quarto/preview-renderer/...'` /
    `'@quarto/preview-runtime'`, plus a handful of implicit-`any`
    warnings in `monacoProviders.ts` / `templateService.ts` /
    `diagnosticToMonaco.ts`. None of those files are touched by
    this refactor. Root cause:
    `ts-packages/preview-runtime/dist/` and
    `ts-packages/preview-renderer/dist/` are not built in this
    checkout — vitest aliases the workspace packages to source dirs
    for tests, but `tsc -b` can't resolve them without built
    `.d.ts` outputs. The workspace-wide `npm run build` (run from
    repo root) also fails on pre-existing issues in
    `q2-demos/hub-react-todo` and `q2-demos/kanban` (WASM bridge
    resolution).
  - **Hand-off:** to fully satisfy CLAUDE.md's build-verify rule,
    run `npm run build:all` from `hub-client/` in a checkout where
    workspace `dist/` directories exist (i.e. after the normal
    monorepo build cadence). Expect zero auth-related errors; any
    errors that surface will be in the same files that fail today
    on a fresh checkout.
- [ ] **End-to-end browser walkthrough — user-owned.** Cannot be run
      in this session; requires booting the dev server and driving a
      real browser. Per CLAUDE.md's end-to-end verification rule,
      this is *the* gating check before declaring the refactor truly
      complete. Walkthrough script:
  1. `cd hub-client && npm run dev` against a hub started with
     `VITE_GOOGLE_CLIENT_ID` configured.
  2. Land on the login screen. **Expect:** "Sign in with Google"
     copy, GIS button rendered. (Phase 4 behaviour preservation.)
  3. Click the button → redirect to Google → return. **Expect:**
     `quarto_hub_token` cookie set, `/auth/me` returns user info,
     SPA shows authenticated UI. (Phase 3 SDK wrap + Phase 4 button
     wiring.)
  4. Sit on a project tab for ~45 minutes (or use devtools to
     manually fire the visibility-change path: hide tab → show
     tab → observe One Tap fires). **Expect:** silent renewal
     succeeds, no UI shown, cookie refreshed. (Phase 5
     `useSilentRenewal` wiring.)
  5. Click sign-out. **Expect:** SPA returns to login screen;
     subsequent attempt to sign in shows Google's account chooser
     (not silent re-auth — confirms `googleLogout()` ran via the
     new `provider.signOut` path). (Phase 5 + Phase 6 IdP-side
     sign-out relocation.)
  6. Boot a hub with `VITE_GOOGLE_CLIENT_ID` unset. **Expect:**
     `useAuth` returns `auth=null` with no console errors; no
     login screen rendered. (Phase 1 null-provider path.)
- [x] **Workspace verify:** not needed — no Rust changed.
      `cargo xtask verify` is irrelevant to this refactor.

### Phase 8 — Post-implementation simplification

Code-review pass after Phase 7. Five changes, no behaviour change.

1. **`AuthProvider | null` → `noopAuthProvider`.** `useAuthProvider()`
   now returns a non-nullable `AuthProvider`; the context default is
   `noopAuthProvider` (`SignInButton` renders null, `useSilentRenewal`
   and `signOut` are no-ops). Consumers drop `?.` / `&&` branches:
  - `useAuth.ts` — `provider.useSilentRenewal(...)` /
    `provider.signOut()`. Also removes the conditional-hook-call
    shape `react-hooks/rules-of-hooks` would flag.
  - `LoginScreen.tsx` — unconditional `<provider.SignInButton ... />`.
  - `main.tsx` — `GOOGLE_CLIENT_ID ? googleAuthProvider : noopAuthProvider`.

2. **Removed `SignInButtonProps.onError`.** No consumer passed it;
   only the wrapper's own test exercised it. YAGNI.

3. **`createGoogleAuthProvider()` → `googleAuthProvider` const.** No
   parameters, no closure state — the factory was indirection without
   benefit. Re-introduce when a parameter actually appears.

4. **Merged `auth/types.ts` + `auth/AuthProviderContext.tsx` →
   `auth/AuthProvider.tsx`.** Single file for the interface, types,
   context, `AuthProviderRoot`, `useAuthProvider`, and
   `noopAuthProvider`. The pre-existing
   `react-refresh/only-export-components` lint moves with the merge
   (mixed exports); resolving cleanly would re-split the file and is
   left for a HMR-feedback-driven pass.

5. **`MockAuthProvider.SignInButton`: render-body write → `useEffect`.**
   `react-hooks/immutability` flagged `state.lastLoginUri = loginUri`
   during render. Wrapped in `useEffect(() => { ... }, [loginUri])`;
   `render()` flushes effects synchronously via the surrounding
   `act()` so existing `mock.lastLoginUri` assertions still hold.

#### Test delta

5 unit tests removed across four files — all covered paths that no
longer exist:

- `AuthProviderContext.test.tsx` → `AuthProvider.test.tsx`: 3 → 2
  (the `provider={null}` and "no Root mounted" cases collapse into a
  single default-noop case).
- `GoogleAuthProvider.test.tsx`: 8 → 7 (dropped onError forwarding).
- `LoginScreen.test.tsx`: 4 → 3 (dropped null-provider case).
- `useAuth.test.tsx`: 20 → 18 (dropped two null-provider cases).

Unit suite: 554 → 549. Integration and WASM unchanged.

## What this refactor explicitly does NOT change

- `AUTH_ENABLED` in `App.tsx` keeps reading `VITE_GOOGLE_CLIENT_ID`.
- The server endpoints `/auth/callback`, `/auth/refresh`, `/auth/me`,
  `/auth/logout`, `/auth/actor` are unchanged.
- The cookie shape and validator are unchanged.
- `useAuth`'s public API (`{ auth, loading, logout, triggerRefresh }`)
  is unchanged; only its internal mechanism for silent renewal moves
  behind the interface.
- The GIS One Tap silent-refresh behaviour is preserved.

## What this refactor enables (future, not in scope)

When/if a second IdP becomes required, the work is bounded to:

1. Implement `OidcAuthProvider` (or `MicrosoftAuthProvider`,
   `Auth0AuthProvider`, …) against the same interface — typically
   wrapping `oidc-client-ts` or `oauth4webapi`.
2. In `main.tsx`, choose the provider based on configuration (e.g.
   `VITE_AUTH_PROVIDER=google|oidc` plus the per-provider config
   vars).
3. The hub-side validator already accepts JWTs against a JWKS-based
   allowlist (Phase 2 of
   `2026-05-05-hub-mcp-device-flow-implementation.md` extends the
   audience allowlist; the issuer policy is a separate future
   change).

No consumer file (`LoginScreen`, `useAuth`, `App.tsx`, `authService`)
needs to change.

## Risks and outcomes

- **GIS SDK lifecycle.** `<GoogleOAuthProvider>` must wrap the tree
  for `useGoogleOneTapLogin` to function. Phase 3 keeps the wrap in
  `main.tsx`. A future provider with its own SDK-load wrapper would
  either nest both (cheap) or motivate moving SDK loading into
  per-provider initialisation (deferred until that requirement is
  real).
- **Test refactor surface.** `useAuth.test.tsx` had the most edits.
  The `oneTapCallbacks` capture pattern transferred cleanly to
  `MockAuthProvider.lastSilentRenewalOpts`; the fall-back of keeping
  `vi.mock('@react-oauth/google')` plus a `useAuthProvider` stub was
  not needed.
- **Rollback.** The refactor is local to hub-client and was
  additive in Phases 1–2, so a rollback during Phases 3–6 could
  have left the new `src/auth/` files in place as dead code (zero
  runtime cost) while reverting consumers. Not needed.

## Verification log

- 2026-05-20 — Phase 7 in-session verification: full
  `npm run test:ci` green (38 unit / 8 integration / 14 wasm test
  files); `tsc --noEmit -p tsconfig.app.json` reports 0 errors
  inside `src/auth/`, `src/main.tsx`,
  `src/components/auth/LoginScreen.tsx`, `src/hooks/useAuth.ts`,
  `src/services/authService.ts`. `npm run build:all` not completed
  due to pre-existing workspace-level failures unrelated to this
  refactor.
- 2026-05-20 — Phase 8 in-session verification: full
  `npm run test:ci` green (38 unit / 8 integration / 14 wasm test
  files; 549 / 66 / 79 tests). `tsc --noEmit -p tsconfig.app.json`
  reports 0 errors inside `src/auth/`, `src/main.tsx`,
  `src/components/auth/LoginScreen.tsx`, `src/hooks/useAuth.ts`.
  `eslint src/auth/MockAuthProvider.tsx` is now clean (was 1
  `react-hooks/immutability` error). Remaining lint findings in the
  auth tree (`react-refresh/only-export-components` on
  `AuthProvider.tsx` and `GoogleAuthProvider.tsx`) are HMR-only
  warnings, not correctness issues.
- (pending) Phase 7 end-to-end browser walkthrough — user-owned.
  See the six-step script under Phase 7 above. Phase 8 is internal
  and does not require a separate walkthrough; the Phase 7 script
  exercises the same surfaces.
