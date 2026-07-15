# Auth reshape — overview & path (index)

**Status:** proposed (index)
**Date:** 2026-07-06

This is a **thin index**. The work is decomposed into **three self-contained,
independent plans** (plus a deferred adoption follow-on to plan 1); the substance
lives in each. This doc only records the map, the sequencing, and the
cross-cutting facts that let them stay independent.

## The three plans

1. **`2026-07-06-hub-client-connection-gated-local-first.md`** — Connection-gated
   auth + local-first documents. The SPA opens with no login gate; documents live
   in browser IndexedDB; auth is required only on "Connect to a hub." **v1** ships
   local-first + gate removal + a "Connect to a hub" action that opens/creates
   **hub-side** projects. **Publishing an existing local project up to a hub
   (adoption)** — the actor-switch + sync-up — is a **deferred fast-follow** in
   its own plan file (`2026-07-06-hub-client-local-project-adoption.md`), gated
   on the D1 durability fix (`bd-10bdjmjb`). *Pure hub-client / sync-client work —
   no server-auth changes.*

2. **`2026-07-06-hub-client-auth-unification-pkce.md`** — Unify hub-client ↔
   hub-mcp auth on Authorization Code + PKCE. Migrate the SPA from the GIS button
   to a **public** PKCE client that obtains a Google ID token and hands it to the
   existing server callback (**pattern (i)** — no hub token-exchange). **v1 builds
   the SPA provider standalone; extracting shared OAuth-config + PKCE primitives
   with hub-mcp is deferred** (only if duplication proves real).

3. **`2026-07-06-hub-server-minted-sliding-sessions.md`** (`bd-ey6jg70f`) —
   Server-minted sliding sessions. Validate the Google token once at login, then
   mint a hub-signed, compact, HttpOnly cookie with sliding expiry — decoupling
   session lifetime from Google's 1 h token and from Google One-Tap. *Adds the
   hub's first token-minting capability.* **v1 uses a single session secret;
   `kid`/rotation and a revocable store are deferred.**

## The path (sequencing)

- **Plan 1 ships first** (user decision) — it's self-contained and delivers the
  local-first value with zero server-auth risk. Its **adoption follow-on**
  (publish a local project to a hub) lands afterward, once D1 (`bd-10bdjmjb`)
  is fixed — separate plan file, same Epic 1.
- **Plans 2 and 3 are independent follow-ons**, either order. Two 2026-07-06
  investigations confirmed plan 2 (PKCE) is **orthogonal** to plan 3 (sessions):
  pattern (i) keeps the browser a token *provider*, so no hub minting is needed.
- **One soft dependency:** plan 2's "retire Google One-Tap renewal" phase (B2)
  needs a durable renewal path → sequence it after plan 3, or keep One-Tap as the
  interim. Everything else is decoupled. **No hard `blocks` between the plans.**
- Plan 3 also has **standalone production value** (it structurally fixes the
  closed-but-only-tactically-mitigated bug `bd-3o8zmz46` and the >3800-byte
  cookie-drop risk), so it can be prioritized early if desired.
- **Standalone (in no epic):** the Bearer-on-`/auth/actor`+`/auth/me` fix
  (`bd-3g0aijb3`) is a small independent server-side bug — a **standalone
  strand**, not a phase of plan 2 or plan 3. Fix it whenever; it gates nothing.

## Cross-cutting realities (why the split is clean)

1. **A browser is a public OAuth client** — it cannot bind a loopback listener,
   use an OS keychain, or hold a `client_secret`; those hub-mcp mechanisms are
   inherently native-only (RFC 8252). Unification is therefore at the IdP /
   auth-code+PKCE / shared-primitives level, not identical transport/storage. The
   hub *already* bridges the SPA and MCP client IDs via a shared JWKS + audience
   allowlist. *(Owned by plan 2.)*
2. **Automerge changes are immutable** — you can redirect *future* changes to a
   new actor but never re-attribute *past* ones; reconciliation is the
   display-layer `identities` map. This is why the local→connected transition is
   a *forward actor switch + display bridge*, never a rewrite. *(Owned by the
   deferred adoption follow-on `2026-07-06-hub-client-local-project-adoption.md`;
   plan 1 v1 keeps the local actor and never switches.)*
3. **The hub is a stateless validator today** — the cookie value *is* Google's
   ID token; the hub mints/signs nothing and has no session store. Plan 3 is what
   changes that (adds minting; a session store only in its deferred revocable
   variant). Plans 1 and 2 leave it stateless. *(Owned by plan 3.)*

## Shared strand map

Three epics, no hard cross-epic `blocks`; phase sub-strands filed when each epic kicks off:
- **Epic 1 — `bd-o3if4hrm`** (epic, p1) → **v1: A0–A4 + A7v1** (plan 1); **deferred adoption: A5, A6, A7adopt** (documented in `2026-07-06-hub-client-local-project-adoption.md`; `conditional-blocks` on `bd-10bdjmjb` until D1 lands). Related: `bd-10bdjmjb` (D1 — gates adoption only), `bd-3nzyd` (E2E 401 tests), `bd-qxgoti2b` (Epic 2).
- **Epic 2 — `bd-qxgoti2b`** (epic, p2) → **B1, B2, B4**; **B0 deferred** (shared package, only if duplication proves real). Related: `bd-cmp48`/`bd-81cfshmw` (hub-mcp reference), `bd-ra5ypj3s` (client registration), `bd-ey6jg70f` (B2 soft dep).
- **Epic 3 — `bd-ey6jg70f`** (epic, p2) → **C0–C4, C6, C7**; **C5 (revocable store) + C5b (`kid`/rotation) deferred**. Related: `bd-3o8zmz46` (root-cause bug it fixes).
- **Standalone (not in any epic):** `bd-3g0aijb3` — Bearer on `/auth/actor`+`/auth/me`, a small independent server-side fix.
