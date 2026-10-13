---
title: 'Consolidate the quarto-hub MCP tools: 23 to 8 via `action` arguments (bd-lj6f7z9s)'
date: 2026-10-13
description: 'Fold same-subject MCP verbs into four multi-action tools (auth, project, read, edit) while keeping the read/write split; a registration-layer refactor with handlers unchanged.'
status: done
braid:
  strand: bd-lj6f7z9s
  priority: P2
---

## Overview

The quarto-hub MCP server lists 23 tools in production posture (21 data
tools plus 2 auth tools; `render` is opt-in via `--allow-render` and sits
outside the default listing). A smaller tool surface is more effective for
models: fewer names to select between, and far less repeated schema text
(`PROJECT_PARAM_DESC` alone rides ~19 input schemas today).

The server already consolidates by argument where that is cheap: presence
folded into `get_project_info` via `include`, binary reads ride `read_file`
via `encoding`/`metadata_only`, there is deliberately no `get_share_url`
tool (ERG-5). This plan extends the same logic to its conclusion.

The work is a **registration- and dispatch-layer refactor only**: every
existing handler (`handleConnectProject`, `handlePatchFile`, and the rest)
stays as-is, and the new tool callbacks map a `(tool, action)` pair onto
the existing `handleTool` switch.

One premise, verified against the code and worth stating plainly: the
existing test suites are **not** handler-level. They call tools by name
through the real protocol surface (`callTool` in `in-memory-fixture.ts`
is `client.callTool({ name, arguments })` over the in-memory linked
transport) — roughly 195 call sites across 25 test files. Renaming the
surface with no alias shims therefore breaks our own suites first, so a
mechanical call-site migration (Phase 3) must land in the same commit as
the registration flip. After migration those suites keep covering
behavior through the merged tools; new tests cover the mapping and the
merged schemas.

## Target surface

Eight tools in the production read-write listing (23 today):

| New tool          | Absorbs                                                                                          | Selector                                            |
|-------------------|--------------------------------------------------------------------------------------------------|-----------------------------------------------------|
| `auth`            | `authenticate`, `authenticate_clear`                                                             | `action`: `sign_in`, `clear`                        |
| `project`         | `connect_project`, `disconnect_project`, `get_project_info`, `list_projects`, `set_default_project_set` | `action`: `connect`, `disconnect`, `info`, `list_sets`, `set_default_set` |
| `read`            | `list_files`, `read_file`, `get_outline`, `search_files`, `get_file_history`                     | `action`: `list`, `read`, `outline`, `search`, `history` |
| `edit`            | `write_file`, `patch_file`, `delete_file`, `rename_file`, `restore_file_version`, `create_folder`, `delete_folder` | `action`: `write`, `patch`, `delete`, `rename`, `restore`, `create_folder`, `delete_folder` |
| `wait_for_change` | —                                                                                                | unchanged                                           |
| `get_errors`      | —                                                                                                | unchanged                                           |
| `docs`            | —                                                                                                | unchanged                                           |
| `create_project`  | —                                                                                                | unchanged                                           |

Read-only mode lists 6 (no `edit`, no `create_project`); `--allow-render`
adds `render` as today. The floor of 6 is reachable by folding
`wait_for_change` (`action: watch`) and `get_errors` (`action: check`)
into `read`, but we do not go there: the watch arm would force
`idempotentHint: false` onto every read mode, and both arms carry long
behavioral guidance that belongs in a dedicated description.

## Design decisions

- **The read/write split is load-bearing.** `--read-only` registers read
  tools only, and hosts use `readOnlyHint`/`destructiveHint` for
  permission UX, so no merged tool may straddle the boundary. `auth`,
  `project`, `read` are read-side (they mutate at most local keyring or
  connection state, exactly as today); `edit` and `create_project` are
  write-side and stay behind the `readOnly` early-return.
- **Arm values keep the current verbs** (`patch`, `restore`, `rename`,
  `outline`, `history`): error messages and docs stay close to the
  vocabulary users and models already see. The folder arms are no
  exception — `create_folder`/`delete_folder`, not `mkdir`/`rmdir`
  shorthand.
- **One selector name: `action` on all four merged tools.** An earlier
  draft gave `read` a `mode` selector on the theory that its arms are
  flavors of one verb — but `list`, `search`, and `history` are as
  verb-distinct as the `project` arms ("they are all reads" describes the
  side-effect class, not the verb). With no real distinction left, a
  single key name means one schema convention for the model to learn and
  no wrong-key validation errors when switching between tools. Uniformity
  beats semantic hair-splitting.
- **Annotation homogenization is accepted on `edit`** — the merged tool is
  `destructiveHint: true`, `idempotentHint: false`, so `create_folder`
  looks scarier than it is — and rejected on reads (see the
  `wait_for_change` point above).
- **`connect_project` folds into `project`.** Every tool auto-connects on
  first use, so its unique value (file list, `shareUrl`, early auth error)
  rides the `connect` action.
- **Schema shape is the one open technical question**, settled by the
  Phase 0 spike: prefer `z.discriminatedUnion` on the selector so per-arm
  required fields survive into the advertised JSON Schema; fall back to
  flat all-optional parameters plus runtime enforcement (the existing
  `PATH_DEFAULTABLE` pattern, also how `docs` handles its mutually
  exclusive `query`/`page`) if the SDK or its `ToolSchema` rejects
  top-level `anyOf`. Where arms overlap heavily (`read` modes all share
  `project`, most share `path`), flat-with-enum may be the better shape
  regardless.
- **Breaking change.** The public tool names change; hosts that allowlist
  or hardcode tool names must update. Call it out in release notes; no
  alias shims.

## Phase 0 — Spike: schema shape (test first)

- [x] Add a failing conformance test: a probe registration with a
  `z.discriminatedUnion` `inputSchema`, asserting the `tools/list` entry
  validates against the SDK `ToolSchema` and the emitted JSON Schema keeps
  per-arm `required` lists
- [x] Record the outcome here; on failure, adopt the flat-optional
  fallback and delete the probe

**Outcome (2026-10-13): the discriminated union works.** The SDK's
zod→JSON-Schema converter emits `type: "object"` at the top level plus
`oneOf` arms, each with its own `properties` and `required` — so
`ToolSchema` accepts the entry (the bare `z.toJSONSchema` output, which
omits the top-level `type`, does not, but the SDK never emits that form).
A call missing an arm-required field errors over the wire as
`Input validation error: … query: Invalid input: expected string, received undefined`
— actionable, names the parameter. Adopted `z.discriminatedUnion` on
`action` for the four merged tools. Fields that must stay schema-optional
(the `PATH_DEFAULTABLE` paths, filled by a share URL's `file=` after
validation) keep runtime enforcement, now keyed per `(tool, action)`. The
throwaway probe test is superseded by per-arm `required`-list assertions
on the real registrations in `conformance.test.ts` (Phase 1).

## Phase 1 — Test specification (before touching registration)

- [x] `conformance.test.ts`: `EXPECTED_RW_TOOLS` becomes the 7 data tools
  (`project`, `read`, `wait_for_change`, `get_errors`, `docs`, `edit`,
  `create_project`), `EXPECTED_AUTH_TOOLS` becomes `` `['auth']` ``,
  `TOOL_BUDGET` drops from 24 to 10; read-only expectations updated to 6
  tools
- [x] Listing/count assertions elsewhere: `EXPECTED_RW_TOOLS` is
  duplicated in `inspector-smoke.test.ts`, and the `render-gating`,
  `bundle`, `npm-pack`, `hub-mcp`, `conformance-official`, and
  `protocol-hygiene` suites also drive `tools/list` — audit and update
  all of them in the same pass
- [x] Dispatch-equivalence tests: each absorbed verb called through the
  new `(tool, action)` pair reaches the same `handleTool` case and
  returns the same structured payload the pre-rename suites asserted
  under the old name (capture the golden payloads before the flip)
  — landed as the golden cases in `(tool, action)` form plus a
  wire-driven per-arm coverage test (`advertisedArms`: every `oneOf`
  branch's `action` const needs a golden call)
- [x] Per-arm validation tests: a missing required arm input errors
  actionably, naming the parameter and the arm (e.g. `read` with
  `action: "search"` and no `query`; `edit` with `action: "patch"` and
  neither `old_string` nor `section`) — same style as the current
  `PATH_DEFAULTABLE` error
- [x] ERG-4 string tests: not-found and dangling-entry errors name the new
  call form (e.g. call `read` with `action: "list"`)
- [x] Annotation tests: `read` keeps `readOnlyHint: true`; `edit` is
  `destructiveHint: true`, `idempotentHint: false`; read-only mode
  registers no write tool

## Phase 2 — Registration and dispatch

- [x] `auth/auth-tools.ts`: collapse `AUTH_TOOL_DEFINITIONS` to one `auth`
  tool with an `action` enum; `AuthToolsState.handle` dispatches on it
- [x] `tools.ts`: replace the 17 absorbed registrations (5 project + 5
  read + 7 edit; the 2 auth tools live in `auth/auth-tools.ts`) with
  `project`, `read`, `edit`; keep the `wait_for_change` (its
  cancellation/progress wiring stays put), `get_errors`, `docs`,
  `create_project`, and `render` registrations untouched
- [x] `handleTool` maps `(tool, action)` to the existing handlers;
  `normalizeArgs` and `PATH_DEFAULTABLE` keep working per arm
- [x] `syncField`/`wait_for_sync` threading works unchanged per write arm
- [x] Land the Phase 3 codemod in the **same commit** — without it the
  ~25 test files calling old tool names go red the moment the
  registrations flip

## Phase 3 — Test call-site codemod

- [x] Rewrite the ~195 `callTool(fixture, '<old-name>', args)` sites
  across 25 test files to the new surface — e.g. `callTool(f, 'read', {
  action: 'read', ...args })`, and the `project`/`edit`/`auth`
  equivalents. Mechanical sed-style transform: name becomes the
  `(tool, action)` pair, args pass through unchanged; script it, then
  eyeball the handful of multi-assertion cases
- [x] Update helpers that hardcode names the same way
  (`mcp-test-client.ts`, share-URL and render-gating fixtures)
- [x] Full package suite green before moving on

## Phase 4 — Strings, prompts, instructions, docs

- [x] Error messages that name tools: `fileNotFoundError` ("call
  list_files"), `unavailableFileError` ("Use delete_file"), the auth error
  pointing at `authenticate`, share-URL guidance in descriptions
- [x] `prompts.ts` (~28 tool-name occurrences), the server instructions
  in `index.ts`, and the `resources.ts` mentions
- [x] `docs/tools/q2-mcp.qmd`: rewrite the Tool reference (~36 tool-name
  mentions across the per-category sections) and the Read-only mode
  section for the new surface
- [x] Keep the shared descriptions (`PROJECT_PARAM_DESC`, the `path`
  parameter, `waitForSyncParam`) as-is — riding ~4 schemas instead of ~19
  is the savings

## Phase 5 — Verification

- [x] Full package suite plus `tsc --noEmit` plus `npm run build`; dist
  smoke (`node dist/index.js --help`)
- [x] End-to-end through the real binary over stdio: `tools/list` shows 8;
  exercise `project` connect/info, every `read` mode, every `edit` arm,
  `get_errors`; paste the transcript into an Outcome section here
- [x] `cargo xtask build-hub-mcp-bundle` and `cargo build --bin q2` so
  `q2 mcp` serves the new surface; sanity-check `q2 mcp --launcher-info`
- [x] Refresh the embedded docs corpus for the rewritten
  `docs/tools/q2-mcp.qmd` — `cargo build --bin q2`, then `cargo xtask
  build-agents-docs`, then `cargo build --bin q2` again (the staging step
  renders docs/ with q2 itself) — so the MCP `docs` tool and `q2 docs
  llms` serve the new page
- [x] Render this note with `q2 render --strict`; comment on the strand

## Outcome

Landed 2026-10-13 (bd-lj6f7z9s). The production listing is 8 tools
(`auth`, `project`, `read`, `wait_for_change`, `get_errors`, `docs`,
`edit`, `create_project`); read-only mode lists 6 (no `edit`, no
`create_project`). Every existing handler is unchanged — the merge is
registration + dispatch only, exactly as scoped.

Verification record:

- `npx tsc --noEmit` clean; `npm run build` clean;
  `node dist/index.js --help` prints usage.
- Full package suite: 69 files, 621 passed / 3 skipped (`npx vitest
  run`). Includes the new conformance nets: per-(tool, action)-arm
  golden coverage read off the wire, per-arm `required`-list assertions
  on the advertised `oneOf` schemas, per-arm validation errors, merged
  annotations, and the auth dispatch through the wire.
- `cargo xtask build-hub-mcp-bundle` + `cargo build --bin q2` done;
  `q2 mcp --launcher-info` reports the fresh bundle
  (`bundle-hash: ecde99c6f19ffa90`, 19 files, shared render-wasm).
- `docs/tools/q2-mcp.qmd` rewritten; `cargo run --bin q2 -- render
  --strict docs/tools/q2-mcp.qmd` clean; `cargo xtask
  build-agents-docs` + rebuild done — `q2 docs llms tools/q2-mcp.md`
  serves the new page.
- This note renders clean with `q2 render --strict`.

End-to-end transcript (output inspected): real `q2 mcp` binary over
stdio against a local `hub --data-dir … --port 3987`, driven by a raw
JSON-RPC script — initialize, tools/list, then create → connect → info
→ every `read` mode → every `edit` arm → `get_errors`:

```text
tools/list → 8 tools: auth, project, read, wait_for_change, get_errors, docs, edit, create_project
create_project → indexDocId 2xz9d4EbrgZ7…
project/connect → 2 files: index.qmd, notes.txt
project/info → counts: {"files":2,"binary":0,"folders":0,"unavailable":0}
read/list → 2 entries
read/read → 11 lines, hash sha256:9a666…
read/outline → headings: Intro, Details
read/search → 1 match(es) in 2 file(s)
edit/write → synced: true
edit/patch → hash sha256:0feda…
edit/rename → renamed: true
read/history → 3 change(s)
edit/restore → restored_from 33cd005066b2…
edit/create_folder → created: true
edit/delete_folder → deleted: true
edit/delete → deleted: true
get_errors → files checked: 1, errors on index.qmd: 0
E2E-OK
```

Notes for reviewers:

- **Breaking rename, no aliases** (as designed): the 19 absorbed tool
  names are gone from the surface; error strings, server instructions,
  prompts, and the docs page all name the new call form
  (`` `tool` with `action: "arm"` ``).
- Annotation homogenization landed as specified: `read` keeps
  `readOnlyHint: true`; `edit` is `destructiveHint: true`,
  `idempotentHint: false`; `project` merges to
  `{ readOnlyHint: false, destructiveHint: false, idempotentHint: true }`
  (disconnect/set_default_set mutate local state; every arm is
  idempotent); `auth` merges to
  `{ readOnlyHint: false, destructiveHint: true, idempotentHint: false }`
  (the clear arm revokes).
- The e2e-auth suite drives the real `q2 mcp` binary, so its freshness
  gate required the bundle rebuild + `cargo build --bin q2` before it
  would exercise the new surface; both done, suite green.

## Details

- Current production listing (23): 2 auth, 5 project/session
  (`connect_project`, `disconnect_project`, `get_project_info`,
  `list_projects`, `set_default_project_set`), 5 file reads, 3
  watches/checks, 7 file writes, `create_project`.
- Test-surface fact (verified 2026-10-13): the suites exercise tools by
  name over the wire, not at handler level — `callTool` is
  `client.callTool({ name, arguments })`; ~195 call sites across 25 files
  (`read_file` ×46, `write_file` ×43, `patch_file` ×15, ...). Only
  `conformance.test.ts` touches `handleTool` directly. The rename breaks
  our own tests before any external host feels it.
- Token savings estimate: `PROJECT_PARAM_DESC` copies drop from ~19 to
  ~4; tool names, titles, and per-tool schema boilerplate shrink by about
  two-thirds; description text roughly halves even after the surviving
  descriptions grow to explain their arms.
- After this lands, treat a new arm on an existing tool as the default
  answer to capability requests — that is what ERG-5 was pointing at all
  along.
