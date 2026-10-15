# Bare-canonical author ID (server-side prefix strip)

Strand: bd-evxckg64 (supersedes bd-9t8zsh98's MCP-side resolution).

## Overview

The hub mints per-project author IDs as
`HMAC-SHA256(server_secret, sub || "\0" || project)` with `project` taken
verbatim from the query param, so the bare and `automerge:`-prefixed
spellings of the same document mint two unrelated identities. bd-9t8zsh98
fixed a reported MCP mismatch by canonicalizing hub-mcp to the prefixed
form, on the premise that the SPA resolves authors over the prefixed URL
form. That premise is wrong:

- The SPA resolves bare for every project it *created*
  (`createNewProject` passes `parseAutomergeUrl(...).documentId`, bare,
  to `resolveAuthorId`, and stores the bare id verbatim into IDB and the
  project set). Only share-joined projects are prefixed (App.tsx
  normalization).
- The author-ID transition\'s E2E sign-off pinned **bare** byte-for-byte
  as the hub-mcp <-> hub-client contract (plan 2026-09-30, lines 685-687:
  "both pass the bare id").

Resolution: the canonical HMAC input is the **bare** document id. The
server strips a leading `automerge:` so client spelling no longer
matters; hub-mcp then drops its prefixing as cleanup.

## Checklist

### Phase 1 — server (quarto-hub)

- [x] Record finding on bd-9t8zsh98 (comment c-ien0j4vg); create
      bd-evxckg64 with `supersedes` dep
- [x] Failing test first: bare and prefixed spellings mint the same
      author ID (`crates/quarto-hub/src/auth.rs`) — confirmed red
      (bare `ebb74b0d…` vs prefixed `bf1e4db0…`)
- [x] Strip a leading `automerge:` inside
      `sub_to_actor_id_for_project` (one point covers `/auth/author` and
      `/auth/actor`, preserving D5 byte-identity); update doc comment
- [x] `cargo nextest run -p quarto-hub` green (506 passed)
- [x] `cargo nextest run --workspace` green (via `cargo xtask verify`;
      an earlier standalone run hit the known-flaky
      `book_refactor_guard` typst pair — passes in isolation and under
      verify, fails only under fail-fast cancellation)
- [x] Commit phase 1 (fdc1ec20b)

### Phase 2 — hub-mcp cleanup (ships any time after the hub deploys)

- [x] Failing test first: `fetchAuthorId` sends the project id as given
      (bare), no `automerge:` prepending
      (`ts-packages/quarto-hub-mcp/src/connection-manager.test.ts`) —
      confirmed red (3 failures, all prefixed-URL assertions)
- [x] Remove the canonicalization in `connection-manager.ts`
      `fetchAuthorId`; rewrite the stale comment (it claims the prefixed
      form is "the one the SPA stores and resolves authors over")
- [x] `npm test -w ts-packages/quarto-hub-mcp` green (621 passed,
      69 files)
- [x] `cargo xtask verify --skip-hub-build` green (all 14 steps,
      including strict claude-notes render of this plan and the
      ts-packages build + MCP smoke)
- [x] End-to-end: HTTP-level test through the real axum routes —
      `auth_author_bare_and_prefixed_project_spellings_match` in
      `crates/quarto-hub/tests/integration/session_auth.rs` asserts both
      spellings mint one value on `/auth/author` and `/auth/actor`;
      confirmed red on the unfixed mint (stash check), green after.
      (A curl check against a live hub needs a mock IdP; the
      integration harness is the same route + query extraction a real
      request hits.)
- [x] Commit phase 2 (b005e71c3); ask before pushing
- [x] Close bd-evxckg64 and bd-9t8zsh98 (superseded)

## Details

- Deploy order: hub first. Once the server strips, any client spelling
  mints the bare-canonical value; the MCP cleanup is cosmetic and can
  ship whenever. SPA needs no change at all.
- Who flips once (immutable history can\'t be migrated): share-attached
  collaborators and MCP users move from their prefixed-minted key to the
  bare-minted key — which is the key that already stamps every
  SPA-created project\'s creation history. `connect()` re-writes
  `setIdentity` with the resolved author on every connect
  (quarto-sync-client client.ts:1296), so name/color follow the new key
  automatically.
- No collision risk: a bs58 document id cannot contain `:`, so `"x"` and
  `"automerge:x"` only ever denote the same project.
- Out of scope (separate hygiene, tracked nowhere yet): mixed-spelling
  IDB/project-set entries and cross-spelling duplicate rows. With server
  normalization these no longer affect identity.
