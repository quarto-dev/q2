# Elevate `quarto-hub-mcp` to a first-class Quarto Hub interface

**Date:** 2026-10-05
**Status:** Ready for implementation
**Scope:** `ts-packages/quarto-hub-mcp`, `crates/quarto-mcp-launcher`, `ts-packages/quarto-sync-client` (small additive exports only: per-document delivery wait and release, public getters for identities/captures, re-export of the project-set helpers for CAP-3 — no behavior changes), `crates/quarto-hub` (Phase 6 only)

> **Bookkeeping note (repo convention):** this plan was authored in Plan mode at
> `.posit/assistant/plans/` and approved 2026-10-06. Filed in braid as parent
> epic **bd-tbuerrg7** with phase epics: Phase 0 **bd-f1dr7gs1**, Phase 1
> **bd-zv8u2sxi**, Phase 2 **bd-oqp2kva8**, Phase 3 **bd-3qe7unp7**, Phase 4
> **bd-738mbhpj**, Phase 5 **bd-rpra6kpq**, Phase 6 **bd-8iv9jty5** (sequential
> `blocks` chain; existing strands linked per §4).
> `claude-notes/plans/CURRENT.md` still points at the author-ID
> transition plan, which is in execution (bd-r62zad5b); this plan is listed
> alongside it rather than displacing it — repoint once that plan closes.

---

## 1. Executive summary

The MCP server is **mature where it matters most and thin where the protocol has
moved on**. The hard parts — OAuth loopback+PKCE, OS-keyring credential storage,
token refresh with typed failure modes, token redaction at every boundary, stdio
hygiene (stdout protection, stdin-EOF exit, outbound sync drain), and a
~5,500-line test suite including e2e auth against a mock IdP — are done well and
in some cases better than most published MCP servers.

The review finds four gaps:

1. **Protocol surface:** the server implements exactly one MCP capability
   (`tools`) and uses none of the features the spec has added since — no
   structured output, no runtime input validation, no cancellation on data
   tools, no resources, prompts, completion, or elicitation. The installed SDK
   (`@modelcontextprotocol/sdk` 1.29.0, protocol `2025-11-25`) supports all of
   these today — but it is now the **maintenance line**: the current spec is
   `2026-07-28`, and the TypeScript SDK v2 (`@modelcontextprotocol/server`;
   the v2 line is stable since 2026-07-27 (2.0.0), now at 2.3.1) implements it
   while still serving `2025-11-25` clients. Building new surfaces on 1.x would target a revision
   that has already removed or deprecated several of them (BP-16).
2. **Capability surface:** 12 tools cover basic text-file CRUD and long-poll
   watching. Nothing for binary files, folders, search, project discovery,
   share URLs, presence, history, render/diagnostics, AST-aware editing, or the
   embedded docs — most of which already have client-side APIs in
   `quarto-sync-client` waiting to be wrapped.
3. **Agent-ergonomics surface:** the tools work, but not the way a human
   works. Writes are fire-and-forget (the agent says "done" before the hub has
   the change); reads carry no content hash, so an agent cannot tell its view
   is stale before it overwrites a collaborator's edit; large files arrive
   whole with no range or truncation; error messages rarely say what to call
   next; the server `instructions` cover only share-URL parsing; and nothing
   measures how many turns an agent needs for a real task (§2.5).
4. **Strategic surface:** stdio-only distribution means browser-hosted agents
   (Claude.ai, ChatGPT connectors) can never reach Quarto Hub; the server is
   not listed in the MCP Registry and has no one-click bundle; and there is no
   user-facing docs page.

**North star:** an agent should be able to *discover, read, write, watch,
render, debug, and share* a Quarto Hub project entirely through MCP, with
protocol-native ergonomics (structured results, cancellable long operations,
subscribable resources, guided prompts) and the situational awareness a human
editor takes for granted (knowing when its view is stale, knowing its write
landed, knowing who else is in the document), while a human collaborator
watches the agent's edits live in the web client.

---

## 2. Review findings: best-practice deviations

IDs (BP-n, HY-n, ERG-n) are referenced by the phase checklists in §5.
Evidence cites `ts-packages/quarto-hub-mcp/src/` as `$SRC`; line numbers are
as of 2026-10-05 on `main`. Spec references name the revision that introduced
a feature; `2026-07-28` is the current revision, and BP-16 covers adopting it.

### 2.1 Protocol compliance and ergonomics

| ID | Finding | Evidence | Spec reference | Impact |
|----|---------|----------|----------------|--------|
| BP-1 | **No structured tool output.** Every result is a plain text content block — some carry JSON text, others bare strings (`Updated …`, raw file contents); no `outputSchema`, no `structuredContent`. | `$SRC/tools.ts:28-30` (`text()` helper used for every result) | `structuredContent`/`outputSchema`, spec 2025-06-18; supported by installed SDK 1.29.0 | Hosts cannot machine-validate or render results; models parse JSON out of prose; newer clients increasingly prefer structured results |
| BP-2 | **No runtime input validation.** Handlers blind-cast (`args.path as string`); a wrong-typed argument becomes a raw `TypeError` wrapped in `isError`. Hand-written JSON schemas duplicate handler expectations and can drift. | `$SRC/tools.ts:346,358-359,417-419`; the "avoids Zod v4" comment at `tools.ts:7-8` predates SDK 1.29's zod handling | SEP-1303 (2025-11-25): input-validation errors should be *tool execution errors* with useful messages | Confusing failures for agents; schema/handler drift is a silent regression class |
| BP-3 | **Data tools ignore cancellation.** `extra.signal` is threaded only into the auth tools. `wait_for_change` — the one tool designed to block for up to 55 s — cannot be cancelled; the promise and its doc-handle listener outlive the client's cancel. | `$SRC/tools.ts:566-588` (the `CallToolRequestSchema` dispatcher forwards `extra` only to the auth tools; `handleTool` at `:309` and `handleWaitForChange` at `:376-414` never see it); `ConnectionManager.waitForChange` (`$SRC/connection-manager.ts:333-338`) takes no signal | `notifications/cancelled`, base spec | Hung server-side state per cancelled call; listener leaks on the automerge doc handle |
| BP-4 | **No progress reporting on long operations.** Only `authenticate` emits `notifications/progress` (to surface the sign-in URL). `wait_for_change` could report elapsed/total when the client supplies a `progressToken`. | `$SRC/auth/auth-tools.ts:426-438` (the one good example); absent in `tools.ts` | Progress, base spec | Host UIs show a dead spinner during a 55 s poll. More importantly, SDK clients can reset their request timeout on each progress notification (`resetTimeoutOnProgress`), so progress is what lets a long poll outlive a host's default per-request timeout — the 55 s clamp exists precisely because there is no progress today |
| BP-5 | **No resources.** The original design deferred resources explicitly ("Q6 Resources: Deferred"). Files are exactly the kind of read-only, subscribable context resources exist for; binary files could be exposed as blob resources (base64), which the tools API cannot cleanly do. | `capabilities: { tools: {} }` at `$SRC/index.ts:269-270`; deferred at `claude-notes/plans/2026-03-13-hub-mcp-server-design.md` L130 | Resources, resource templates, `notifications/resources/updated` (base spec). `resources/subscribe` was **removed** in 2026-07-28 in favor of `subscriptions/listen`; the SDK maps eras (BP-16) | Agents can only pull content tool-by-tool; humans using MCP-aware hosts cannot attach project files as context |
| BP-6 | **No prompts.** No guided workflow templates (review-a-draft, fix-render-errors, collaborate-with-a-human). The repo has already learned that steering text matters — `wait_for_change`'s description is a mini-protocol-spec. | absent | Prompts, base spec; the 2026-07-28 **Skills over MCP** extension (`ext-skills`) is the newer vehicle for the same guidance (§6) | Every host/user rediscovers the correct call patterns |
| BP-7 | **No argument completion.** `project` (known/connected ids) and `path` (files in the project) are the two highest-value completion domains in any MCP server. | absent | Completion capability | Manual id/path entry in hosts that support completion |
| BP-8 | **No elicitation, and the auth URL hack now has a standard replacement.** `authenticate` smuggles the sign-in URL to the user via a progress notification message. URL-mode elicitation (SEP-1036, 2025-11-25) exists precisely for secure out-of-band interactions like OAuth. | `$SRC/auth/auth-tools.ts:426-438` | Elicitation (2025-06-18); URL mode + enums (SEP-1036, SEP-1330, 2025-11-25). In 2026-07-28 server-initiated `elicitation/create` is replaced by **MRTR** (Multi Round-Trip Requests, SEP-2322): the tool returns `resultType: "input_required"` and the client retries the call with the answer | Non-standard UX that many clients render poorly; falls back silently when no `progressToken` |
| BP-9 | **No `title` fields on tools; icons unadopted.** Names are snake_case and mostly verb-led — close to the `verb_noun` convention SEP-986 standardized (exceptions: `authenticate`, `authenticate_clear`, `wait_for_change`) — good — but host UIs get no human-friendly titles. Icons (SEP-973, 2025-11-25) are cheap chrome. | tool defs at `$SRC/tools.ts:51-225` and `$SRC/auth/auth-tools.ts:130-167` | SEP-986, SEP-973 (2025-11-25) | Minor presentation |
| BP-10 | **Server reports version `0.0.1`, hardcoded.** The launcher already knows the embedded bundle's git commit and build time (`q2 mcp --launcher-info`); none of it reaches the MCP `Implementation` record. | `$SRC/index.ts:263-266` | `Implementation` version, base spec; 2025-11-25 also added `description`, `websiteUrl`, `icons` on `Implementation`, aligned with the registry `server.json` (CAP-15) | Support and debugging can't tell which build a client is running; version-gated behavior impossible |
| BP-11 | **JSON Schema dialect unstated.** Schemas are plain draft-07-ish objects; 2025-11-25 makes JSON Schema 2020-12 the default dialect (SEP-1613). Harmless today (no 2020-12-only keywords), but the conformance harness (Phase 0) should pin this. | `$SRC/tools.ts` passim | SEP-1613 (2025-11-25); 2026-07-28 (SEP-2106) allows any 2020-12 keyword in `inputSchema`/`outputSchema` | Latent ambiguity for strict validators |
| BP-15 | **Unknown tool name is reported as a tool-execution error, not a protocol error.** The dispatcher returns `isError: true` text for an unknown `name`; the spec reserves JSON-RPC `-32602` for unknown tools and keeps `isError` for failures *inside* a known tool. Hosts that distinguish the two (retry policy, telemetry) see a misclassified failure. | `$SRC/tools.ts:576-580` (and the `handleTool` default arm at `:341`) | Tools spec, "Error handling" (2025-06-18 onward) | Minor conformance; falls out of the BP-2/BP-16 work |
| BP-16 | **Built on the SDK's maintenance line.** `@modelcontextprotocol/sdk` 1.x tops out at protocol `2025-11-25` (1.32.1 is the latest 1.x; bug-fix and security only, guaranteed to at least 2027-01). SDK v2 (`@modelcontextprotocol/server` / `client` / `core` 2.3.1; Node ≥ 20 — the repo pins 24) implements the current `2026-07-28` revision as a **dual-era** server: stateless per-request `_meta`, mandatory `server/discover` (which now carries `instructions`), `subscriptions/listen` replacing `resources/subscribe`, MRTR replacing server-initiated elicitation, the extensions framework (Tasks, Apps, Auth, Skills), deterministic `tools/list` order with `ttlMs`/`cacheScope` cache hints, and legacy `initialize` for `2025-11-25` clients. v2 validates tool input/output with Standard Schema (zod v4), which retires the `tools.ts:7-8` objection (Q-1) and gives BP-2 for free. | `ts-packages/quarto-hub-mcp/package.json` (`^1.12.1`, resolves 1.29.0); `$SRC/tools.ts:7-8` | [2026-07-28 changelog](https://modelcontextprotocol.io/specification/2026-07-28/changelog); SDK v2 migration docs (`docs/migration/upgrade-to-v2.md`, `support-2026-07-28.md`) | Every new surface in this plan (resources now; elicitation or tasks if §6 later adopts them) would otherwise be built against an API the current spec has removed or deprecated; the migration is the enabling step for Phases 1–5 |
| BP-17 | **No cache hints; tool order undeclared.** 2026-07-28 asks servers to return `tools/list` in deterministic order (prompt-cache hits) and lets list/read results carry `ttlMs`/`cacheScope`. Our list is deterministic by construction (static arrays) but nothing says so and nothing carries hints. | `$SRC/tools.ts:557-564` | SEP-2549 (2026-07-28) | Minor; falls out of BP-16 |

### 2.2 Security posture (mostly strong; three refinements)

Done well, and worth preserving as invariants in all new work:

- Loopback+PKCE (RFC 8252) with constant-time state check, RFC 9207 `iss`
  validation, Host-header allowlist on the loopback listener (DNS-rebinding
  defense), keyring-only storage, proactive refresh with in-flight mutex,
  `invalid_grant` → store wipe + `ReauthRequired`, 403 keeps the keyring.
- Token redaction on every log/error path including process-level
  `uncaughtException`/`unhandledRejection` scrubbers
  (`$SRC/index.ts:170-181`).
- Insecure-transport gate (Bearer over `ws://` to non-loopback rejected).
- Annotations present and honest on all 12 tools (destructive/idempotent/
  readOnly/openWorld) — better than most servers.
- Static tool descriptions: no tool-poisoning surface.
- MCP-level authorization is correctly *absent*: the spec says stdio servers
  SHOULD NOT implement MCP authorization and should take credentials from the
  environment. The loopback+PKCE flow is the server acting as an OAuth
  *client* to the hub (a downstream credential), which is the sanctioned
  pattern. Keep that distinction crisp in CAP-16, where the hub becomes an
  OAuth *resource server* and the rules invert.
- The browser launcher spawns without a shell on every platform
  (`$SRC/auth/browser.ts`), so a hostile URL cannot inject a command.

Refinements:

| ID | Finding | Evidence |
|----|---------|----------|
| BP-12 | **`authenticate` has no concurrency guard.** The doc comment argues hosts serialize `tools/call` and concurrent calls are "undefined-but-non-corrupting" (worst case two browser tabs) — a deliberate decision, not an oversight. Still worth a cheap mutex to make the behavior defined. | `$SRC/auth/auth-tools.ts:204-212` |
| BP-13 | **No `authenticate_status` tool.** Deferred since the device-flow plan's "future work" list. Agents cannot ask "am I signed in, as whom, until when?" without risking a browser flow. | `claude-notes/plans/2026-05-05-hub-mcp-device-flow-implementation.md` L1426-1436 |
| BP-18 | **Authorization URL is opened without validation.** The URL handed to the browser (and surfaced to the user) is built from `as.authorization_endpoint`, i.e. *fetched* authorization-server metadata; `openBrowser` applies no scheme or host check. The Security Best Practices page asks servers to accept only `http(s)`, reject `javascript:`/`data:`/`file:`, and treat metadata-derived URLs as SSRF input (HTTPS only, no private/link-local hosts). Low risk today (no shell, trusted IdP), but cheap to pin as an invariant — with BP-8 deferred (§6), the progress-notification hand-off it guards is the long-term path. | `$SRC/auth/auth-tools.ts:403`; `$SRC/auth/browser.ts` (`openBrowser`) |

### 2.3 Code hygiene

| ID | Finding | Evidence |
|----|---------|----------|
| HY-1 | **`read_file`'s binary error names a tool that doesn't exist** (`read_binary_file_metadata`). Actively misleads agents into a doomed retry. | `$SRC/tools.ts:371` |
| HY-2 | **Dead `registerAuthTools`** — an abandoned wiring whose standalone `CallToolRequestSchema` handler would conflict with `registerTools`' if ever called (the SDK allows one handler per method). Never called from `index.ts`. | `$SRC/auth/auth-tools.ts:628-641` |
| HY-3 | **Stale compiled device-flow artifacts linger in the local `dist/` build output** (`dist/auth/device-flow.*`) — the source was deleted when loopback+PKCE replaced it. `dist/` is **gitignored** (`.gitignore:48`), so nothing stale is committed; the risk is a test or packaging step that consumes `dist/` (the `McpTestClient` spawns `node dist/index.js`) picking up orphaned modules. Fix: `rm -rf dist` before every `tsc` build in the package scripts, and have the npm strand (bd-3tak0lyy) package from a clean build. | `ts-packages/quarto-hub-mcp/dist/` (local), `.gitignore:48`, `$SRC/mcp-test-client.ts` |
| HY-4 | **`getWriteTools` omits `delete_folder`-class operations entirely** — not a bug, but the write-surface asymmetry (files yes, folders no) has no documented reason. | `$SRC/tools.ts:129-225` |
| HY-5 | **No per-project disconnect.** `ConnectionManager` exposes `connect`/`disconnectAll` but nothing to drop one project; the sync client likewise only has whole-client `disconnect`. A long agent session touching many projects accumulates websocket + in-memory-doc state with no release valve. | `$SRC/connection-manager.ts:261,463`; `quarto-sync-client/src/client.ts:1449` |
| HY-6 | **Startup stderr noise with a cached credential** — Node's `TimeoutNegativeWarning: -1` from the refresh timer, plus deprecated automerge `initSync()` parameter warnings. Harmless, but every host surfaces stderr to the user as "server errors", so it is the first thing a new user sees. Already tracked (bd-rgt8rglx open; bd-2qnnrwbd, un-deferred 2026-10-06); fold into Phase 1. | `braid show bd-rgt8rglx`, `braid show bd-2qnnrwbd` |

### 2.4 Testing gaps

| ID | Finding |
|----|---------|
| BP-14 | **No protocol-level conformance tests.** The suite is strong on behavior (e2e against in-process hub and mock IdP) but nothing validates the *wire contract*: that every `tools/list` entry validates against the SDK's `ToolSchema`, that every `inputSchema` is valid JSON Schema (2020-12 per SEP-1613), that results match declared `outputSchema`s, or that a cancelled request actually frees its listener. A schema-conformance harness would have caught BP-2's blind casts. `mcp-test-client.ts` is a hand-rolled raw JSON-RPC-over-stdio client that never schema-validates responses — right for the stdio-hygiene tests, wrong as a conformance foundation. The **official suite exists**: `npx @modelcontextprotocol/conformance server --url … --spec-version 2025-11-25\|2026-07-28` (0.1.x, with a GitHub Action and an `--expected-failures` baseline), plus `@modelcontextprotocol/inspector --cli` for smoke tests. The official suite drives servers over Streamable HTTP, so running it needs a loopback HTTP listener (test-only, or a dev `--http <port>` flag) — which Phase 6 wants anyway. |

### 2.5 Agent ergonomics (the "as smooth as by hand" gap)

A human editing in the web client sees collaborators' text arrive live, sees
their own keystrokes land, can scroll a long file, can undo, and gets an
inline hint when something is wrong. Each row is a place where an agent
driving the same project through MCP lacks the equivalent signal. Guidance
references: Anthropic, ["Writing effective tools for AI
agents"](https://www.anthropic.com/engineering/writing-tools-for-agents)
(2025-09-11), and the MCP tools spec.

| ID | Finding | Evidence | Fix / impact |
|----|---------|----------|--------------|
| ERG-1 | **No stale-view protection.** `read_file` returns bare text with no content hash; `write_file` takes whole-file content and applies it with automerge `updateText`, a diff against the *current* doc. If a collaborator edited between the agent's read and its write, the diff reverts their edit — silently, attributed to the agent's user. `patch_file` is safer (it splices against the latest merged text and fails if `old_string` is gone) but returns no post-write hash either. `wait_for_change` already computes `sha256:` hashes (`hashPayload`). | `$SRC/tools.ts:357-374` (no hash), `:416-440`; `quarto-sync-client/src/client.ts:1564-1572`; `$SRC/connection-manager.ts:136-143` | The one failure a human would never produce: overwriting what a collaborator just typed. `read_file`/`write_file`/`patch_file` return `hash`; write tools accept optional `expected_hash` and refuse on mismatch, returning the current content + hash — compare-and-swap, the CRDT equivalent of looking at the screen before typing |
| ERG-2 | **Writes are fire-and-forget.** Write tools return `Updated …` the moment the local automerge change is made; delivery to the hub is asynchronous and only the process-exit drain (`drainOutbound`, bd-10deu8h4) ever waits for it. An agent that says "done, refresh your browser" before the hub has the bytes is the most visible "not like by hand" moment, and a crash or network blip in that window loses the edit with no signal to the agent. `isDelivered`/`remote-heads` already exist in the sync client but are not exported. | `$SRC/tools.ts:438-439`, `:477-478`; `quarto-sync-client/src/client.ts:1386`, `:1405-1439` | Bounded delivery wait on every write (default ≈2 s; `wait_for_sync: false` to opt out); result carries `synced: true\|false` and the new `hash`. Reverses the §6 deferral of "per-write delivery confirmation" — the substrate landed with the exit drain |
| ERG-3 | **No range, size, or truncation handling on reads.** `read_file` returns the entire file; `list_files` entries carry `{path, type}` (plus `status`/`docId` on unavailable entries) — no size, mime type, or line count to help an agent decide what to read. A large `.qmd`, a notebook, or a data file fills the context window with no warning and no way to ask for less. | `$SRC/tools.ts:351-374`, `:284-294` | `read_file` gains `offset`/`limit` (lines) and `max_bytes`, returning `truncated: true` plus a continuation hint; `list_files` entries carry `size`, `mimeType`, and `lines` for text. Standard guidance: paginate or truncate with sensible defaults and steer the model toward targeted reads |
| ERG-4 | **Error messages stop at the diagnosis.** `patch_file`'s multi-match message is the model ("Provide a longer, unique string"); most others are terminal. `File not found: <path>` suggests neither `list_files` nor a near match; the share-URL server mismatch tells the user to restart the server (bd-qt7h8h5g removes that); wrong-type and unknown-tool errors (BP-2, BP-15) are raw. | `$SRC/tools.ts:368,455,510,525,528`; `:253-259` | Convention for Phase 1: every error names the failing parameter, the current state, and the next tool to call; path errors list up to three closest existing paths. Spec rationale: `isError` results exist "to enable model self-correction" |
| ERG-5 | **No tool-surface budget.** §3 proposes roughly twenty new tools on top of twelve. Selection accuracy degrades as the list grows; the guidance is fewer, consolidated, high-impact tools with parameters for variants. | §3 | Rules applied to every CAP below: (a) same verb → same tool, varied by parameter (`read_file` returns an `image`/blob content block for binaries rather than a `read_binary_file` sibling; `write_file` takes `encoding: "base64"`); (b) read-only, browsable content moves to resources where the host supports them; (c) hard ceiling of **24 tools** in the default listing (opt-in render tools excluded); (d) one `get_project_info` instead of separate info tools |
| ERG-6 | **Server `instructions` cover only share-URL parsing.** `instructions` is the one text every host injects into the model's context before any tool is called. Ours explains the `project` parameter and nothing else: no recommended workflow, no collaboration etiquette, no auth summary, no read-only notice. | `$SRC/index.ts:272-280` | Rewrite as a ~15-line operating guide: connect → read (keep the `hash`) → `patch_file` with `expected_hash` → check `synced` → `wait_for_change` before large rewrites; "prefer `patch_file`; never `write_file` a file a human is editing"; when to call `authenticate`; the untrusted-content note (ERG-10). Delivered via `initialize` to legacy clients and `server/discover` under 2026-07-28 — the SDK handles both |
| ERG-7 | **Nothing measures agent experience.** §2.4 covers wire conformance; nothing in the repo runs a model against the server on a realistic task and records turns, tokens, wrong-tool calls, or validation retries. §9's criteria were qualitative. | — | A small **agent-task eval suite** (Phase 0): 5–8 scripted tasks (e.g. "from this share link, add a figure and fix the render error"; "a collaborator edited `intro.qmd`; summarize the change") run headless (`claude -p` or the Claude Agent SDK) against the in-process `test-hub`, scored on success, turns, tokens, and `isError` count. Run at phase gates by hand, not in CI (cost, nondeterminism); commit the transcripts. Evaluation-driven iteration is the most-repeated recommendation in current tool-design guidance |
| ERG-8 | **Watching is per-file only.** `wait_for_change` requires `path`; a collaborating agent usually wants "did anything change in the project" (file added, removed, or edited by anyone), and it cannot tell its own writes from a collaborator's. | `$SRC/tools.ts:97-126`, `:376-414` | CAP-18: `path` becomes optional (project-wide watch returning changed paths + hashes); with ERG-1's write-side `hash` the agent passes its own post-write hash as `since_hash`, so its own edit is not reported back |
| ERG-9 | **No undo.** Humans have Ctrl+Z and the web client's history view; an agent that made a bad `write_file` has no way back short of reconstructing the text. | — | CAP-19: `restore_file_version(path, to_hash)` as a new attributed change (CRDT-safe, never rewrites history); rides on CAP-9's history plumbing. Agent-appropriateness is Q-6 |
| ERG-10 | **Untrusted-content posture is undocumented.** Hub documents are multi-author; text returned by `read_file`/`wait_for_change` can carry instructions aimed at the agent. The server cannot and should not filter it, but neither `instructions`, the README, nor the planned docs page says so. | — | One sentence in `instructions` (ERG-6) and a section in the docs page (CAP-17). Documentation only; no code change |

---

## 3. Review findings: missing capabilities

IDs (CAP-n) referenced by §5. "Existing substrate" = the API already available
today without hub-server changes.

### 3.1 Discovery and sharing (today: impossible without a raw automerge id)

| ID | Capability | Existing substrate | Notes |
|----|-----------|--------------------|-------|
| CAP-1 | Share URLs — embed `shareUrl` (the `https://quarto-hub.com/#/share/<id>?name=…&file=…` link for a connected project) in the `create_project`, `connect_project`, and `get_project_info` results, so an agent can hand a human a clickable link. | `share-url.ts` already parses this exact format (inverse operation is trivial) | Highest value-per-line in the whole review. **No standalone `get_share_url` tool** (ERG-5 budget): with `shareUrl` embedded in `get_project_info`, a separate tool would spend a listing slot on a redundant path. Also add an optional `name` to `create_project` — it has none today (`connection-manager.ts:374-376`), so the `name=` parameter cannot be filled |
| CAP-2 | `get_project_info` — file/folder counts, contributors (identities), engine-capture summary, index doc id, server URL, auth mode, sync diagnostics. The "doctor" tool deferred from the exit-drain plan ("parent plan Phase 1.5"). | `getSyncDiagnostics`, `getDocInventory` (public); `getIdentitiesFromIndex`, `getCapturesFromIndex` are **internal** helpers (`client.ts:478,483`) and need a small public export; `ConnectionManager.lastObservedAuthMode()` | One tool; answers "what is this project and is my connection healthy" |
| CAP-3 | `list_projects` — enumerate the caller's projects/collections. | `ProjectSetDocument` + `addProjectToSet` exist in `@quarto/quarto-automerge-schema` (`src/index.ts:192,237`; not re-exported by the sync client); **but** the project-set doc id is stored client-side only (IndexedDB singleton pointer, read at `hub-client/src/services/projectSetStorage.ts:20`, written at `:29-40`). There is no server-side per-user registry. | MVP: accept a project-set doc id (or share URL to one) and enumerate it. Full "list my projects" needs a hub-side, auth-keyed registry — a **hub feature**, scoped as a Phase 6 design item. The MVP lands in Phase 2 |

### 3.2 Completing the file surface

| ID | Capability | Existing substrate |
|----|-----------|--------------------|
| CAP-4 | Binary read **through `read_file`** (ERG-5): image mime types return an `image` content block, other binaries an embedded blob resource (base64), both with `structuredContent` `{mimeType, size, sha256}`; `metadata_only: true` returns just the metadata. The SDK already ships `ImageContentSchema`/`EmbeddedResourceSchema`. Closes HY-1 for real without a sibling tool. | `getBinaryFileContent`, `isFileBinary`, `BinaryDocumentContent` |
| CAP-5 | Binary write **through `write_file`/`create_file`** with `encoding: "base64"` (ERG-5). Needed so agents can add images to projects — a render-blocking gap for real documents. | `createBinaryFile` |
| CAP-6 | Folder CRUD: `create_folder`, `delete_folder`; `list_files` gains folder entries. (Folder *rename* is iterative — rename each contained file plus the folder marker — and can wait.) Resolves HY-4. | `createFolder`, `deleteFolder`, `getFolderPaths` (IndexDocument v3) |
| CAP-7 | `search_files` — substring/regex content search across project text files with context snippets and a result cap. Client-side scan is fine at project scale (the web client's in-memory provider does the same). | Deferred Phase 2 item from the original design; `hub-client/src/services/search/inMemorySearchProvider.ts` as reference |

### 3.3 Collaboration awareness

| ID | Capability | Existing substrate | Notes |
|----|-----------|--------------------|-------|
| CAP-8 | `list_presence` — who is connected, on which file, cursor/selection, last-seen. | Presence rides automerge ephemeral messaging; `getIndexHandle()` exposes the project-scoped channel; message schema lives in `hub-client/src/services/presenceService.ts` | Read-only observation only — the MCP server must **not** inject fake cursor presence. See Q-3 |
| CAP-9 | `get_file_history` — ordered change summaries per file using automerge heads (`A.getHeads`/`A.view`/`A.diff`): "what changed since yesterday", "show the last 5 changes and their authors"; optional `from_hash`/`to_hash` returns a diff between two heads (one tool, ERG-5 rule (a)). | Pure client-side automerge; `branchService.ts` demonstrates clone/merge viability, and `getHeads`/`view`/`diff` are already used in `attribution-runs.ts`/`debugApi.ts` | No hub changes needed; attribution via the index `identities` map |
| CAP-10 | Engine-capture visibility: fold capture state (`idle/running/error`, `lastError`, staleness) into `get_project_info`; optionally `clear_capture` for stale entries. | `onCapturesChange`, `clearCapture` | Read surface in CAP-2; `clear_capture` is a maybe (Q-4) |
| CAP-18 | Project-wide watch: `wait_for_change` with `path` omitted blocks until *any* file is added, removed, or edited, returning `[{path, hash, kind}]`; the agent's own writes are excluded via `since_hash` (ERG-8). | `onFileAdded`/`onFileChanged`/`onFileRemoved` already fan into per-path `fireWaiters` (the `SyncClientCallbacks` in `connect()`, `connection-manager.ts:278-296`) — a project-level waiter set is a few lines | Lands in Phase 3 with BP-4; the Phase 1 BP-3 rework should leave `path` ready to become optional |
| CAP-19 | `restore_file_version` — revert a file to a prior `hash` as a new, attributed change (ERG-9). | Same automerge heads/`view` plumbing as CAP-9 | Q-6 decides agent-appropriateness; default: allowed, `destructiveHint: true`, result carries the pre-restore `hash` so the restore is itself reversible |

### 3.4 Quarto-specific intelligence (the differentiators)

These are what make this a *Quarto* MCP server rather than a generic CRDT file
API.

| ID | Capability | Existing substrate | Notes |
|----|-----------|--------------------|-------|
| CAP-11 | **AST-aware qmd editing**: `get_outline` (headings/sections) plus a `section` selector on `read_file`/`patch_file` — structural reads and edits instead of fragile string surgery (ERG-5 rule (a): same verbs, varied by parameter). | `getFileAst`/`updateFileAst` via `ASTOptions` — **requires a caller-supplied parser in Node** | Gated on spike S-1 (parser in Node: `wasm-qmd-parser` nodejs target staged into the bundle like the keyring addons, vs. a pampa napi-rs module) |
| CAP-12 | **Render loop**: `render_project` / `render_file` — materialize the project to a temp dir, run `q2 render --json-errors`, return structured diagnostics (`quarto-error-reporting` JSON wire: codes, messages, source locations) plus output paths. Closes the agent loop: edit → render → see errors → fix. | `exportProjectAsZip` (fflate — pure JS, Node-safe, verified); `q2 render` already has `--json-errors` (`crates/quarto/src/commands/render.rs:84,801-802`); launcher can inject the `q2` binary path (`current_exe`) as an env var | **Security:** rendering executes user code (knitr/jupyter). Must be opt-in (`--allow-render` flag), documented, annotated |
| CAP-13 | **Docs tool**: `docs` — `query` searches the embedded `agents-docs-dist` tree, `page` fetches one page's markdown (one tool, ERG-5 rule (a)) — the same corpus `q2 docs llms` serves, reachable without shelling out. | Tracked as **bd-dn81ol95**; "shares the embedded tree; no new artifact" — needs a small launcher/`q2 docs` seam to expose pages to the Node child; **bd-b6cocsxw** (`q2 docs llms --json` for `--list`/`--embed-info`) is that seam's natural first half | Absorb both strands into Phase 4 |
| CAP-14 | YAML/front-matter validation surfaced as diagnostics — *covered by CAP-12* (render reports real diagnostics via `--json-errors`); a standalone `validate_file` is redundant unless Phase 4 experience says otherwise | — | Deliberately folded into CAP-12 |

### 3.5 Distribution and reach

| ID | Capability | Status |
|----|-----------|--------|
| CAP-15 | npm/npx distribution (`npx @quarto/hub-mcp`), **plus** a listing in the official MCP Registry (`server.json` + `mcpName` in `package.json`, published with `mcp-publisher`; the registry is in preview with the v0.1 API frozen since 2025-10-24) and a one-click Claude Desktop bundle (`.mcpb`, also a registry package type). Discovery and install are part of "first-class". | Tracked: **bd-3tak0lyy** (P4, gated on public-release readiness); the standalone tarball (bd-sca6g1tu, closed) was explicitly a stopgap. No `server.json` exists today |
| CAP-16 | **Remote MCP: the hub serves MCP over Streamable HTTP** so browser-hosted agents (Claude.ai, ChatGPT connectors) connect directly. | Explicitly out of scope in the `q2 mcp` plan (2026-06-11, L226-227) — revisit now. The tool handlers are already transport-agnostic (`handleTool` knows nothing about stdio) — the real work is auth (hub as OAuth resource server: RFC 9728 Protected Resource Metadata, introduced in 2025-06-18 and given the `.well-known` fallback by SEP-985 in 2025-11-25; RFC 8707 resource indicators; client registration via CIMD per SEP-991 — Dynamic Client Registration was **deprecated** in 2026-07-28) and connection management under the 2026-07-28 **stateless** model (no sessions, no `Mcp-Session-Id`; SDK v2.3 is one-server-per-request). Phase 6 design doc gates implementation |
| CAP-17 | User-facing docs page under `docs/` (setup for Claude Code/Desktop/Cursor, tool reference, auth walkthrough, the collaboration model — hashes, `synced` — and the untrusted-content note, ERG-10) | Unchecked item from the `q2 mcp` plan (L461); today only the 13.5 KB README; `docs/guides/projects/create.qmd` mentions MCP servers only generically |

---

## 4. Existing braid strands (link, don't duplicate)

| Strand | Relation to this plan |
|--------|----------------------|
| **bd-qt7h8h5g** (P2 bug) — can't reach projects on non-default sync servers | **Fix in Phase 1.** Design: key `ConnectionManager` by `(serverUrl, indexDocId)`; accept an optional per-call `server` and honor `server=` in share URLs by connecting to that server (no-auth attempt first; Bearer tokens are audience-bound and must not be replayed to a foreign origin) |
| **bd-dn81ol95** — expose embedded docs as `q2 mcp` tools | Absorbed as CAP-13 / Phase 4 |
| **bd-b6cocsxw** — `q2 docs llms --json` for `--list`/`--embed-info` | First half of the CAP-13 seam; absorbed into Phase 4 |
| **bd-rgt8rglx** (P3 bug) — `TimeoutNegativeWarning` at startup with a cached credential; **bd-2qnnrwbd** — stderr warts (un-deferred 2026-10-06; re-scoped to the `initSync()` params half — the timer warning stays with bd-rgt8rglx) | HY-6; fix in Phase 1; both are Phase 1 children in braid and gate its close |
| **bd-3tak0lyy** — npm publish | Absorbed as CAP-15 / Phase 6 (with the registry listing and `.mcpb`); package from a clean build (HY-3) |
| **bd-ra5ypj3s** — Google 400 for gmail accounts on the Desktop OAuth client | Auth-polish; Phase 6 operator work, investigated alongside CAP-16 |
| **bd-qxgoti2b** — unify hub-client and hub-mcp auth on Auth Code + PKCE | Adjacent epic; CIMD/PRM work in Phase 6 should be coordinated, not duplicated |
| **bd-8oyidg7h** — remove deprecated `/auth/actor` + hub-mcp fallback | Independent cleanup; touch-point in Phase 1 if `fetchAuthorId` is refactored |
| **bd-r62zad5b** (in progress) — author-id transition Phase 5 (hub-mcp fetches author via Bearer) | **Coordinate**: Phase 1 edits the same connection/auth code; land ordering with its owner. Also the forum for Q-3: the MCP server fetches the *human's* per-project author id (`fetchAuthorId`), so a "Claude (via MCP)" identity would rename the human's own entry — agent attribution needs a distinct, user-linked author id minted by the hub |
| Test-hygiene: bd-n6dhrz4j, bd-4qi4j3sa, bd-yw3mcdkg, bd-l0oc6se2, bd-c72wsugj, bd-fuw5gcni, bd-9itqqqe6, bd-8e96g942 | Not duplicated; Phase 0 harness must not add new flake modes (network-gated like existing live tests) |

---

## 5. The plan

Process requirements for **every** phase (repo mandates, non-negotiable):

- **TDD**: failing test first, then implementation (`AGENTS.md` TDD section).
  Each phase below lists its test specifications *before* its work items.
- **Build chain after any TS change**: `cargo xtask build-hub-mcp-bundle &&
  cargo build --bin q2`, then confirm freshness with `q2 mcp --launcher-info`
  (embed commit/dirty marker). A plain `cargo build` embeds a stale bundle.
- **Phase-close gate**: `cargo xtask verify` (full — the hub-build leg is
  affected by definition here) green, plus `npm run test -w
  ts-packages/quarto-hub-mcp`.
- **E2E per phase**: drive the new surface through a real MCP client
  (the Phase 0 SDK-client fixture for CI; a live Claude Code/Desktop session
  against the in-process `test-hub` and, for final validation,
  quarto-hub.com). Record the invocation and observed output in the plan file.
- Commit at clean phase boundaries per the approved-plan execution policy.
- **Each phase is a separate PR in a stack of PRs.** Phase *n*'s branch is
  based on phase *n*−1's branch (not on `main`), so the stack lands in order
  and each PR's diff shows only that phase's work. Rebase the rest of the
  stack as each PR merges; keep every PR green on its own (the phase-close
  gate runs against the branch tip, not the eventual merge base).
- **Tool budget** (ERG-5): ≤ 24 tools in the default listing; a new tool must
  say why a parameter on an existing tool, or a resource, does not serve.
  Budget tally for this plan: 12 existing + 1 (Phase 1: `authenticate_status`)
  + 6 (Phase 2: `create_folder`, `delete_folder`, `search_files`,
  `get_project_info`, `list_projects`, `disconnect_project`) + 3 (Phase 3:
  `list_presence`, `get_file_history`, `restore_file_version`) + 2 (Phase 4:
  `get_outline`, `docs`) = **24, at ceiling**. Consolidations already applied
  to stay inside: binary I/O rides `read_file`/`write_file` (CAP-4/5); no
  standalone `get_share_url` (CAP-1 embeds `shareUrl`); `diff_file` is a mode
  of `get_file_history` (CAP-9); section reads/edits are a `section`
  parameter on `read_file`/`patch_file` (CAP-11); `docs_search`/`docs_fetch`
  are one `docs` tool (CAP-13). Q-gated maybes (`clear_capture`, Q-4) and any
  future tool enter the default listing only by freeing a slot.
- **Every result structured** (BP-1) and **every error actionable** (ERG-4):
  conventions enforced by the Phase 0 harness and applied to every tool a
  phase touches.
- **Agent-task evals at each phase gate** (ERG-7): run the suite; record
  success, turns, and tokens in this plan next to the e2e transcript.
- **Living steering text**: the server `instructions` (ERG-6) and the docs
  page (CAP-17) describe only tools that exist as of their phase; both are
  revised in every phase that adds a surface.
- **Correctness before elegance**: a correctness fix lands in the earliest
  phase it fits, even when a later phase reworks the same code — BP-3 threads
  cancellation through `wait_for_change` in Phase 1, and BP-4/CAP-18 rework
  it again in Phase 3. The second pass is deliberate, not an oversight.
- **Differentiation before chrome**: Phase 4 (Quarto-specific tools)
  precedes Phase 5 (protocol surfaces) because CAP-12's render loop and
  CAP-11's AST editing are the differentiators and depend on nothing in
  Phase 5; shipping them first closes the agent loop earlier. The prompts
  surface follows so the render-flavored `fix-render-errors` prompt ships
  alongside the other three templates. The official conformance suite backing
  the registry listing's `2026-07-28` claim (CAP-15) is deferred to Phase 6,
  where it gates the listing and can ride the CAP-16 HTTP transport if one
  lands; until then the Phase 0 in-memory harness is the wire-contract net.

### Phase 0 — Protocol conformance harness (test infrastructure first)

The regression net everything else TDDs against. Pure test code and fixtures —
no runtime additions, and nothing changes for stdio users.

- [x] In-memory linked-transport fixture: SDK `Client` ↔ our `Server` over
  `InMemoryTransport.createLinkedPair()`, booted against the in-process
  `test-hub`. A *new* fixture, not an extension of `mcp-test-client.ts` —
  that client is raw JSON-RPC over a spawned stdio process with no schema
  validation; keep it for the stdio-hygiene tests. Write against the SDK
  `Client` API so the fixture survives BP-16 (`@modelcontextprotocol/client`
  in v2).
- [x] `@modelcontextprotocol/inspector --cli … --method tools/list` smoke test
  over stdio (no listener needed). The official conformance suite
  (`@modelcontextprotocol/conformance`) is deferred to Phase 6: at 0.1.x its
  marginal value over the in-memory fixture plus the schema-conformance tests
  below does not justify a test-only HTTP transport and a maintained
  expected-failures baseline.
- [x] **Agent-task eval suite** (ERG-7): 5–8 scripted tasks + a runner
  (headless `claude -p` or the Agent SDK, Q-7) against `test-hub`; scoring
  (success, turns, tokens, `isError` count, validation retries); baseline
  recorded in this plan before Phase 1 changes anything. Not in CI.
- [x] **Test spec — security invariants**: the auth URL handed to the browser
  is `http(s)` with a non-private host (BP-18); Bearer is never sent over
  `ws://` to a non-loopback peer (exists; pin as a named conformance test).
- [x] **Test spec — tool budget**: default `tools/list` count ≤ 24 (ERG-5).
- [x] **Test spec — schema conformance**: every `tools/list` entry validates
  against the SDK's `ToolSchema`; every `inputSchema` compiles as JSON Schema
  2020-12 (BP-11, BP-14).
- [x] **Test spec — result conformance**: for each tool with an `outputSchema`
  (none yet — harness ships with the assertion wired and a placeholder schema
  list), a golden call's `structuredContent` validates (BP-1's future net).
- [x] **Test spec — cancellation hygiene**: cancelling `wait_for_change`
  mid-poll resolves the request as cancelled and leaves zero listeners on the
  doc handle. Land it as vitest `test.fails` in Phase 0 (green-by-construction
  against the unfixed code, so the Phase 0 gate stays green); flip to a normal
  test when the BP-3 fix lands in Phase 1.
- [x] Wire into `vitest` + confirm no new network access (CI-flake policy:
  offline by default, like the existing suite modulo its gated live tests).

#### Phase 0 completion record (landed 2026-10-06, bd-f1dr7gs1)

**Files.** `src/in-memory-fixture.ts` (fixture + `seedProject` + typed
`callTool`), `src/conformance.test.ts` (14 tests: fixture smoke, tool
budget, schema conformance, result-conformance net, security invariants,
cancellation hygiene), `src/inspector-smoke.test.ts`,
`eval/{run.mjs,tasks.mjs,README.md}` + `eval/results/2026-10-06T09-17-33/`
(baseline transcripts). Runtime deltas, both behavior-preserving:
`createServer` extracted from `main()` in `index.ts` (fixture and stdio
entrypoint now share one construction, so the harness can't drift from
what real clients see), and `ConnectionManager.pendingWaiterCount()` — a
read-only conformance seam, the leak gauge for the BP-3 net. New devDeps:
`ajv` (JSON Schema 2020-12 compilation), `@modelcontextprotocol/inspector`
(smoke test).

**Red-by-construction nets, verified failing for the right reason** (then
landed as `it.fails`): BP-3 — client call rejects `AbortError` on cancel
but the waiter count stays 1 (leak); BP-18 — the browser is handed
`http://169.254.169.254/...?response_type=code&…` built straight from
malicious IdP metadata. BP-18 ships as `it.fails` too (same flip rule as
BP-3 when its Phase 1 fix lands) — a small extension of the plan's
`test.fails` mechanism beyond the cancellation case.

**E2E (per the phase-gate requirement).** The fixture smoke test is the
CI e2e: real SDK `Client` → `InMemoryTransport` → real `Server` →
test-hub; create → list → read round-trip. Plus the official-client leg:

```
node node_modules/.bin/mcp-inspector --cli node dist/index.js \
  -e QUARTO_HUB_SERVER=ws://127.0.0.1:<port>/ws --method tools/list
→ exit 0; stdout JSON with the 10 expected tools, each ToolSchema-valid;
  stderr empty. (Invocation recorded in src/inspector-smoke.test.ts;
  `-e` is required because the CLI parses `--server` as its own option.)
```

Note for Phase 6 (CAP-16): the inspector CLI parses `--server <name>` as
*its* catalog option; server flags must reach the spawned process via
`-e KEY=VALUE`.

**Q-7 resolved: headless `claude -p`.** `--output-format stream-json
--verbose` gives machine-readable transcripts (turns, tokens, cost,
per-tool results) with zero new dependencies and exercises the Claude
Code host; the Agent SDK would add a dependency for no scoring gain.
7 tasks (the §5 "5–8"), scored on hub state + final answer, never on
tool sequence. Suite: `npm run eval -w ts-packages/quarto-hub-mcp`.

**Eval baseline (2026-10-06, claude-fable-5-1 via Claude Code 2.1.289,
commit of branch tip; transcripts committed at
`ts-packages/quarto-hub-mcp/eval/results/2026-10-06T09-17-33/`):**

| task | success | turns | tokens (in/out) | cost | isError | retries | nonMcp | duration |
|------|---------|-------|-----------------|------|---------|---------|--------|----------|
| create-project | PASS | 3 | 66/528 | $0.0775 | 0 | 0 | 1 | 12s |
| read-and-report | PASS | 4 | 98/645 | $0.4918 | 0 | 0 | 1 | 13s |
| patch-typo | PASS | 5 | 130/1358 | $0.5463 | 0 | 0 | 1 | 25s |
| write-new-file | PASS | 4 | 98/627 | $0.4850 | 0 | 0 | 1 | 12s |
| rename-file | PASS | 7 | 130/1425 | $0.5568 | 0 | 0 | 1 | 25s |
| collaborator-edit | PASS | 3 | 66/353 | $0.4524 | 0 | 0 | 1 | 11s |
| watch-live-edit | PASS | 4 | 98/1722 | $0.5673 | 0 | 0 | 1 | 64s |

7/7 PASS, median 4 turns, zero `isError`, zero validation retries. The
`nonMcp=1` everywhere is Claude Code's ToolSearch discovery (host-side,
not a server ergonomics gap). Baseline takeaways for Phase 1: the server
is already adequate for one-file tasks; the watch task took 64 s
(baseline for BP-4's progress reporting); nothing yet exercises a
*failed* call path (wrong args, stale state) — that is where ERG-4's
error-quality work will show up, and Phase 1 should add an
error-recovery task to the suite.

**Phase-close gate:** `cargo xtask verify` green (14/14; required fixing
a pre-existing CI↔verify drift first — xtask's `lint:css` args lacked
CI's `--if-present`, broken by npm 11 matching the nested
`vscode-sync-experiment` workspace; landed as its own commit),
`npm run test -w ts-packages/quarto-hub-mcp` green (24 files, 269
passed + 2 expected-fail + 3 pre-existing skips). Bundle rebuilt and
freshness confirmed via `q2 mcp --launcher-info`.

### Phase 1 — Correctness and protocol hygiene

Opens with the SDK v2 migration (BP-16), the enabling step for everything
below; the remaining items are small and independently shippable once it
lands. **Coordinate landing order with bd-r62zad5b (in progress, same
code).**

Test specifications (all red before implementation):

- [x] Wrong-typed argument (`path: 42`, missing `project`) → `isError` result
  naming the offending parameter and expected type (BP-2; SEP-1303 — tool
  execution error, not protocol error).
- [x] Client-cancelled `wait_for_change` → prompt cancellation, no listener
  leak (BP-3; Phase 0 harness asserts).
- [x] `list_files` result carries `structuredContent` matching its
  `outputSchema`, with the JSON text fallback retained (BP-1).
- [x] Dual-era handshake: a legacy client (`initialize`, `2025-11-25`) and a
  modern client (`server/discover`, per-request `_meta`) both list and call
  tools against the same process, and `instructions` arrives on both paths
  (BP-16).
- [x] `initialize`/`server/discover` reports the real embedded version plus
  `description` and `websiteUrl` (BP-10).
- [x] `read_file` returns `hash`; `write_file` with a stale `expected_hash`
  is refused with the current content + hash and changes nothing;
  `patch_file` with a matching `expected_hash` succeeds and returns the new
  `hash` (ERG-1).
- [x] `write_file` result carries `synced: true` once the in-process hub has
  acknowledged the change; with the hub stalled, `synced: false` within the
  bound, and the write still lands when the hub returns (ERG-2).
- [x] `read_file` on a missing path names `list_files` and the closest
  existing paths (ERG-4); an unknown tool name is a JSON-RPC `-32602` error,
  not an `isError` result (BP-15).
- [x] `tools/list` order is identical across calls and carries `ttlMs`
  (BP-17).
- [ ] Startup against a cached credential emits no `TimeoutNegativeWarning`
  and no automerge deprecation warning on stderr (HY-6).
- [x] Concurrent `authenticate` calls serialize (BP-12).
- [x] `authenticate_status` reports `{authenticated, hub, identity?, expiry?}`
  without triggering a flow (BP-13).
- [ ] Share URL naming a *different* `server=` now connects to that server
  instead of erroring (bd-qt7h8h5g): unit tests for multi-server manager
  keying + e2e against two in-process hubs; regression test that Bearer is
  never replayed cross-origin.

Work items:

- [x] BP-16: migrate to SDK v2 (`@modelcontextprotocol/server` for the
  server, `@modelcontextprotocol/client` in tests): `McpServer.registerTool`
  with zod v4 input/output schemas, stdio transport from
  `@modelcontextprotocol/server/stdio`, dual-era enabled. Keep `handleTool`'s
  shape (transport-agnostic): the migration touches registration and types,
  not handler logic. Re-run `bundle.test.ts` and `q2 mcp --launcher-info`;
  Node floor is ≥ 20 (repo pins 24). Record Q-1's outcome.
- [x] BP-2: input validation comes from the v2 zod schemas (SEP-1303 error
  shape); the hand-written JSON schemas are deleted, not duplicated.
- [x] BP-15: unknown tool → protocol error. BP-17: `ttlMs`/`cacheScope` on
  `tools/list` (static list → long TTL).
- [x] **Checkpoint (intra-phase gate):** once the BP-16 migration and its
  free fallout (BP-2, BP-15, BP-17) land, run the full phase-close gate
  (`cargo xtask verify`, `npm run test -w ts-packages/quarto-hub-mcp`, eval
  suite) and commit before starting the ergonomics batch (ERG-1 onward). The
  dual-era handshake and wrong-typed-argument test specs must be green at
  this point. The migration is the riskiest single change in this plan;
  everything after it is small and independently revertable.

#### Phase 1 checkpoint record (landed 2026-10-06, bd-zv8u2sxi)

**BP-16 migration.** `@modelcontextprotocol/sdk` 1.x →
`@modelcontextprotocol/server` 2.3.1 (pinned `~2.3.1`), with
`@modelcontextprotocol/client` + `/core` 2.3.1 in devDeps for the harness.
`registerTools` is now per-tool `McpServer.registerTool` calls with zod v4
schemas (zod 4.4.3 dedupes workspace-wide); `handleTool` and every handler
kept verbatim (transport-agnostic, revert = registration-only). The stdio
entrypoint is `serveStdio(factory)` — the v2 entry that owns era
classification and instance pinning — replacing hand-wired
`server.connect(new StdioServerTransport())`, which in v2 serves **legacy
only**: `supportedProtocolVersions` merely installs the discover handler;
era pinning happens inside `serveStdio`'s `connectInstance` (SDK-internal
`setNegotiatedProtocolVersion`). One migration-found wire detail: the
2026-07-28 `_meta` envelope keys are camelCase
(`io.modelcontextprotocol/protocolVersion`), not kebab-case. `path` became
schema-optional on the six share-URL-`file=`-defaultable tools (zod
validation runs before `normalizeArgs` can default it); the requirement is
now enforced post-normalization with a message naming the share-URL
affordance. HY-2's dead `registerAuthTools` went away with the migration.

**Q-1 outcome (recorded).** The zod objection (`tools.ts:7-8`) is fully
retired: zod v4 schemas are the single source for the advertised JSON
Schema and runtime validation; zod 4.4.3's `~standard.jsonSchema`
conversion keeps `.describe()` text and emits draft 2020-12 with a
`$schema` declaration (BP-11 closed as fallout). Friction was all in the
dual-era enablement described above — the migration guide does not
surface that `server.connect()` + `StdioServerTransport` is legacy-only,
nor that `InMemoryTransport` carries no era classification (the dual-era
tests therefore drive `serveStdio` over a stdio-shaped in-memory wire —
the production path).

**E2E (real binary, recorded).** `cargo xtask build-hub-mcp-bundle &&
cargo build --bin q2`; freshness confirmed by `q2 mcp --launcher-info`.
Legacy: `initialize` → `2025-11-25`, `tools/list` → 12 tools, clean exit
on stdin EOF. Modern (camelCase envelope): `server/discover` →
`supportedVersions: ["2026-07-28"]` + instructions; `tools/list` → 12
tools, `ttlMs: 3600000, cacheScope: "private"`. Both invocations against
`./target/debug/q2 mcp --server ws://127.0.0.1:1/ws`, output inspected.

**Checkpoint gate.** `cargo xtask verify` green (14/14);
`npm run test -w ts-packages/quarto-hub-mcp` green (25 files, 274 passed
+ 2 expected-fail + 3 skips — the BP-3/BP-18 `it.fails` nets still red by
construction). Eval suite 7/7 PASS, median 4 turns, zero `isError`, zero
retries (transcripts `eval/results/2026-10-06T10-32-53/`): turns/tokens
in line with the Phase 0 baseline — the migration is agent-invisible, as
intended.
- [x] ERG-1: `hash` on `read_file`/`write_file`/`patch_file` results;
  `expected_hash` on `write_file`/`patch_file` (compare-and-swap, reusing
  `hashPayload`).
- [x] ERG-2: bounded delivery wait on all write tools (`synced`, default
  ≈2 s, `wait_for_sync: false` opt-out); needs one sync-client export
  (`awaitDelivery(path, ms)` over the existing `isDelivered`/`remote-heads`
  plumbing).
- [x] ERG-4: error-message convention (parameter, state, next tool;
  near-match paths) applied to every data tool.
- [x] ERG-6: rewrite `instructions` as the operating guide (workflow,
  etiquette, auth, read-only, untrusted content).
- [x] BP-18: validate the authorization URL (`https`, non-private host)
  before surfacing or opening it.
- [x] HY-6: fix bd-rgt8rglx (`TimeoutNegativeWarning`) and bd-2qnnrwbd
  (deprecated `initSync()` params — un-deferred and re-scoped to the
  initSync wart only; both are Phase 1 children in braid, so both gate
  this phase's close).
- [x] BP-3: thread `extra.signal` through `handleTool` →
  `ConnectionManager.waitForChange(..., { signal })` and connect paths where
  feasible; abort unregisters listeners.
- [x] BP-1: `outputSchema` + `structuredContent` for `connect_project`,
  `list_files`, `wait_for_change`, `create_project`, and every write tool
  (`{path, hash, synced}`) — and each tool added later; convention enforced
  by the Phase 0 harness. (Extended to `read_file` too — Phase 1 reshaped
  it for ERG-1, and the ERG-1 hash → `expected_hash` loop is exactly the
  machine-read structuredContent is for. Auth tools stay prose-text: they
  are interactive flows, not data.)
- [x] BP-10: launcher injects `QUARTO_MCP_SERVER_VERSION` (embed commit +
  workspace version); server reports it; fallback to a bundle-build-time
  stamp when run standalone. Also set `Implementation.description` and
  `websiteUrl` (shared with `server.json`, CAP-15).
- [x] BP-9: add `title` to all tools (and a Quarto icon if trivial — else
  defer icons to Phase 5). (Titles landed; icons deferred to Phase 5.)
- [x] BP-12: `authenticate` mutex.
- [x] BP-13: `authenticate_status` tool.
- [x] HY-2: delete dead `registerAuthTools`. (Deleted with the BP-16
  migration — it was v1-only wiring; see the checkpoint record.)
- [x] HY-1: interim message fix — drop the phantom `read_binary_file_metadata`
  reference (binary reads land in `read_file` in Phase 2, CAP-4).
- [ ] bd-qt7h8h5g: multi-server `ConnectionManager` (per-call `server`
  override + share-URL `server=` honored; origin-scoped auth).
- [x] HY-3: `rm -rf dist` before `tsc` in the package scripts; confirm no test
  or packaging step consumes orphaned `dist/` modules.

### Phase 2 — Complete the file and project surface

No new protocol surfaces; pure tool additions over existing sync-client APIs.

Test specifications (red first):

- [ ] Binary round-trip: `write_file` (`encoding: "base64"`) → `read_file`
  returns an `image` block for a PNG and an embedded blob for a PDF, identical
  bytes, correct `mimeType`/`size`/`sha256`; `metadata_only` returns no bytes;
  the default tool count stays ≤ 24 (CAP-4/5, ERG-5, HY-1 closed for real).
- [ ] `read_file` with `offset`/`limit` returns the requested lines; a file
  over `max_bytes` returns `truncated: true` and a continuation hint;
  `list_files` entries carry `size`/`mimeType`/`lines` (ERG-3).
- [ ] Folder lifecycle: create/list/delete; `list_files` includes folders
  (CAP-6).
- [ ] `search_files` returns ranked matches with snippets, honors a result
  cap, skips binaries (CAP-7).
- [ ] `get_project_info` shape: counts, identities, captures, ids, server,
  auth mode (CAP-2; golden-shape test through the Phase 0 harness).
- [ ] `create_project` (with `name`), `connect_project`, and
  `get_project_info` results embed a `shareUrl` that round-trips through
  `parseProjectRef` (CAP-1).
- [ ] `list_projects` enumerates a project-set document passed by doc id or
  share URL (CAP-3 MVP).

Work items:

- [ ] CAP-4, CAP-5, CAP-6, CAP-7, CAP-2, CAP-1, CAP-3 (MVP), ERG-3 (each with
  `outputSchema` from day one; CAP-2 needs the
  `getIdentitiesFromIndex`/`getCapturesFromIndex` exports from the sync
  client; CAP-3 MVP uses the re-exported project-set helpers).
- [ ] HY-5: `disconnect_project` tool (per-project teardown in
  `ConnectionManager`; may need a small per-document release in the sync
  client).
- [ ] CAP-17: user-facing docs page `docs/tools/q2-mcp.qmd` (or similar):
  install via `q2 mcp --print-config`, auth walkthrough, tool reference,
  `--read-only`, share-URL semantics, the collaboration model (hashes,
  `synced`), and the untrusted-content note (ERG-10). (Sidebar
  entry per the docs lint rules.)

### Phase 3 — Collaboration awareness

Test specifications:

- [ ] `list_presence` reflects a fake peer's ephemeral presence message on the
  index channel (test-hub + hand-crafted message per `presenceService`'s
  schema); the MCP server itself emits **no** presence (CAP-8, Q-3).
- [ ] `get_file_history` returns ordered change summaries with author
  attribution; called with `from_hash`/`to_hash` it returns a diff between
  the two heads matching an expected patch (CAP-9).
- [ ] `wait_for_change` emits progress notifications when given a
  `progressToken` (BP-4).
- [ ] `wait_for_change` without `path` returns the set of changed paths with
  hashes; the agent's own write (its post-write `hash` passed as
  `since_hash`) is not reported (CAP-18).
- [ ] `restore_file_version` to a prior hash produces a new change whose
  content equals the historical text and whose result carries the
  pre-restore `hash` (CAP-19, if Q-6 says yes).

Work items:

- [ ] CAP-8 `list_presence` (passive only).
- [ ] CAP-18 project-wide watch; CAP-19 `restore_file_version` (Q-6).
- [ ] CAP-9 `get_file_history` (automerge `getHeads`/`view`/`diff`; bounded
  `limit`; `from_hash`/`to_hash` diff mode).
- [ ] BP-4 progress on `wait_for_change`.
- [ ] Q-3 decision recorded (agent self-announcement). **Constraint found in
  review:** the MCP server authenticates as the human and fetches *their*
  per-project author id (`fetchAuthorId`), so writing "Claude (via MCP)" into
  the `identities` map would rename the human's own entry. Recommendation:
  the hub mints a distinct, user-linked author id for agent sessions (e.g. a
  flag on the author-id endpoint) so the web client can show "Charlie (via
  Claude)" without touching the human's identity; never fake cursor presence.
  Design it with the bd-r62zad5b owner.

### Phase 4 — Quarto-specific intelligence

- [ ] **Spike S-1 (gates CAP-11):** qmd parser in Node. Compare (a)
  `wasm-qmd-parser` built with a `nodejs` target and staged into the esbuild
  bundle (precedent: keyring `.node` staging), vs. (b) napi-rs module for
  `pampa`. Output: decision + measured bundle-size/startup cost. Time-boxed;
  failure mode = CAP-11 slips, rest of phase proceeds.

Test specifications:

- [ ] `get_outline` on a fixture qmd returns the expected heading tree;
  `read_file` with a `section` selector returns exactly that section's
  content (CAP-11).
- [ ] `patch_file` with a `section` selector replaces exactly one section's
  content; concurrent outside edits are preserved (CRDT merge) (CAP-11).
- [ ] `render_project` on a fixture project with a deliberate error returns
  `structuredContent.diagnostics[]` with the expected `Q-` code and source
  location; absent `--allow-render`, the tool is not listed (CAP-12).
- [ ] `docs` with a `query` returns ranked pages; `docs` with a `page`
  returns one page's markdown (CAP-13 / bd-dn81ol95).

Work items:

- [ ] CAP-11 AST surface (post-S-1): `get_outline` + `section` selectors on
  `read_file`/`patch_file`.
- [ ] CAP-12 `render_project` / `render_file`: launcher injects
  `QUARTO_Q2_PATH` (`current_exe`); server materializes the project via
  `exportProjectAsZip` → temp dir → `q2 render --json-errors`; parse the
  JSON diagnostics wire into `structuredContent`; **opt-in via
  `--allow-render`** (code-execution gate), documented with the security
  model.
- [ ] CAP-13 `docs` tool (bd-dn81ol95 + bd-b6cocsxw; design the
  `q2 docs llms --json` ↔ MCP seam).
- [ ] CAP-10 `clear_capture` decision (Q-4).

### Phase 5 — MCP-native surfaces

Test specifications:

- [ ] `resources/list` on a connected project enumerates files (paginated with
  `nextCursor` on a 500-file fixture); `resources/read` returns text contents
  and base64 blob contents for binaries; a modern client's
  `subscriptions/listen` and a legacy client's `resources/subscribe` both
  receive `notifications/resources/updated` on `onFileChanged` (BP-5; the
  SDK maps eras).
- [ ] Write-tool results include a `resource_link` to the file's `hub://` URI
  (BP-5).
- [ ] `prompts/list` exposes the workflow templates; `prompts/get` renders
  with arguments (BP-6).

Work items:

- [ ] BP-5 resources + resource template (URI scheme e.g.
  `hub://{server}/{indexDocId}/{path}` — finalize in implementation, Q-2) +
  subscribe bridge over the sync-client callbacks;
  `notifications/resources/list_changed` on file add/remove; `resource_link`
  blocks in write results; `ttlMs`/`cacheScope` on list results (BP-17).
- [ ] BP-6 prompts: `review-draft`, `collaborate-with-human`,
  `safe-edit-workflow` (read → patch → confirm), plus the render-flavored
  `fix-render-errors` (render → read diagnostics → patch → re-render) — all
  four land here, now that CAP-12 has shipped in Phase 4.
- [ ] BP-9 icons, if deferred from Phase 1.

### Phase 6 — Distribution and remote access

Test specifications:

- [ ] Packaging smoke test: `npm pack` from a clean build, install the
  tarball into a temp prefix, `npx @quarto/hub-mcp --help` runs, and the
  installed server connects to the in-process test-hub (CAP-15).
- [ ] `server.json` validates against the MCP Registry schema; the `.mcpb`
  bundle's one-click config shows the exact command and requires consent
  (CAP-15).
- [ ] Official conformance suite green at `--spec-version 2026-07-28` with an
  **empty** expected-failures baseline, run against the CAP-16 Streamable HTTP
  transport if it has landed, else a test-only loopback listener; gates the
  registry listing's conformance claim (satisfies §9).

Work items:

- [ ] CAP-15 npm publish (bd-3tak0lyy): package layout from a clean build
  (HY-3), license notices, install docs; `server.json` + `mcpName` and
  `mcp-publisher` publication to the MCP Registry (a GitHub Actions flow
  exists); `.mcpb` bundle for Claude Desktop one-click install (the one-click
  config MUST show the exact command and require consent — Security Best
  Practices). Retire the standalone tarball (bd-sca6g1tu) once npm ships.
- [ ] Official conformance suite (deferred from Phase 0): run
  `npx @modelcontextprotocol/conformance server --spec-version 2026-07-28`
  before the registry listing claims conformance — by now either the CAP-16
  transport exists to run it against, or the listing itself justifies the
  test-only loopback listener, and the suite will have matured past 0.1.x.
  Empty expected-failures baseline is the bar.
- [ ] **Design doc first (gates implementation):** CAP-16 hub-served MCP over
  Streamable HTTP. Must resolve: hub as OAuth resource server (RFC 9728 PRM
  with `WWW-Authenticate` + `.well-known` fallback per SEP-985; RFC 8707
  resource indicators; scope hints and `insufficient_scope` step-up per
  SEP-835; CIMD per SEP-991 — DCR is deprecated; the 2026-07-28 Auth
  extension for client-credentials/enterprise cases — coordinate with
  bd-qxgoti2b and bd-ra5ypj3s); the Security Best Practices list applied to a
  remote server (token passthrough prohibited — the hub validates tokens
  issued *to the MCP endpoint*, never raw IdP tokens; confused-deputy consent;
  SSRF rules on metadata URLs; `Origin` validation → 403); connection
  lifecycle under the **stateless** model (no sessions; SDK v2.3 is
  one-server-per-request, but the `ConnectionManager` holds per-project
  websocket state — working assumption is a per-user pool keyed by token
  subject with idle eviction, Q-5); multi-tenant fairness (one automerge
  connection per user × N users).
- [ ] **Explore (no commitment):** MCP Apps extension
  (`io.modelcontextprotocol/ui`) for an in-host rendered preview of a file —
  the closest MCP equivalent of the web client's live preview. Gated on host
  support; a one-page feasibility note only.
- [ ] CAP-3 full discovery: hub-side per-user project registry (auth-keyed),
  if the design doc confirms the need; MVP shipped in Phase 2.
- [ ] Update `docs/` MCP page with npm + remote-connection instructions.

---

## 6. Explicitly considered and deferred

| Item | Rationale |
|------|-----------|
| **Tasks** (SEP-1686, experimental in 2025-11-25; moved to the official `io.modelcontextprotocol/tasks` extension in 2026-07-28, SEP-2663, with `tasks/get` polling) | The natural eventual home for `wait_for_change`-class long operations and renders. Client support is nascent; cancellation + progress (Phase 1/3) covers today's need. Revisit once SDK v2 exposes the extension and a mainstream host advertises it |
| **Remaining 2026-07-28 surfaces** after BP-16 (dual-era adopted in Phase 1) | Nothing in this plan builds on surfaces 2026-07-28 **removed** (`ping`, `logging/setLevel`, `resources/subscribe` — served via `subscriptions/listen` — the HTTP GET stream, SSE resumability) or **deprecated** (roots, sampling, logging, DCR, HTTP+SSE). Not adopted yet: `Mcp-Method`/`Mcp-Name` headers and OTel `traceparent` in `_meta` (HTTP-only; Phase 6), the error-code allocation policy (adopt when we define our own codes). Sources: [release post](https://blog.modelcontextprotocol.io/posts/2026-07-28/), [changelog](https://modelcontextprotocol.io/specification/2026-07-28/changelog), [deprecations](https://modelcontextprotocol.io/specification/2026-07-28/deprecated) |
| **MRTR / URL-mode elicitation auth hand-off** (BP-8) | Same deferral logic as Tasks: MRTR (`resultType: "input_required"`, SEP-2322) replaced server-initiated elicitation in 2026-07-28, but no mainstream host advertises support yet. The existing progress-notification hand-off — the ultimate fallback in the original three-tier design — remains the single path, with BP-18's URL validation (Phase 1) as its security invariant. Adopt when a mainstream host ships MRTR; SDK v2's dual-era surface keeps it registration-level work |
| **Argument completion** (BP-7) | Serves manual parameter entry in human-driven host UIs; this server's primary consumer is an agent that already discovers paths via `list_files` and projects via `connect_project`. Revisit if a mainstream host surfaces completion prominently for stdio servers |
| **MCP Apps** (`ext-apps`, 2026-07-28 extension) — interactive HTML UI inside the host | Would give the agent's human partner an in-host preview of a rendered page, the closest thing to the web client's live preview. Host support is early; Phase 6 carries a one-page feasibility note, nothing more |
| **Skills over MCP** (`ext-skills`) | Could ship a Quarto-authoring skill to hosts instead of, or alongside, BP-6 prompts. Decide in Phase 5 once host adoption is visible; prompts are the portable baseline |
| **Logging capability** (`notifications/message`) | Deprecated in 2026-07-28; stderr is the sanctioned stdio log channel (2025-11-25, PR #670) and what hosts surface. Keep stderr clean (HY-6) instead |
| **Comments/annotations tools** | No comments entity exists anywhere (hub server, automerge schema, or web client). That is a hub feature first — file a strand; MCP exposure follows naturally |
| **Publishing tools** (`q2 publish`) | Publishing lives outside the hub (`crates/quarto-publish`); a `publish_project` tool could follow the CAP-12 pattern later — not in this plan |
| **Rust-native MCP port** | Evaluated and rejected in the 2026-06-11 plan (auth maintained in two languages); nothing has changed that calculus |
| ~~Per-write delivery confirmation~~ | **Promoted to Phase 1 as ERG-2.** The exit-drain work landed the substrate (`isDelivered`/`remote-heads`), and the "conflicts with tools.ts ownership" concern is moot now that this plan owns `tools.ts` |

## 7. Open questions (resolve in the noted phase)

- **Q-1 (Phase 1):** Resolved in review, pending confirmation during BP-16:
  the `tools.ts:7-8` objection predates SDK zod-v4 support (1.x accepts
  `^3.25 || ^4`, and the repo already resolves zod 4.4.3); SDK v2 is
  zod-v4/Standard-Schema native. Record migration friction, not the yes/no.
- **Q-2 (Phase 5):** Resource URI scheme (`hub://…`) and its stability as a
  public contract.
- **Q-3 (Phase 3):** Should the agent announce itself in presence/identities
  (honest signaling to human collaborators) and if so, how? Recommendation:
  yes — via a hub-minted agent author id linked to the user (Phase 3), because
  the server currently shares the human's author id and would otherwise
  rename their identity; never fake cursor presence.
- **Q-4 (Phase 4):** Is `clear_capture` agent-appropriate (it mutates shared
  project state) or human-only?
- **Q-5 (Phase 6):** Under the 2026-07-28 stateless model (sessions removed),
  how does the hub keep automerge connections warm across an agent's requests?
  Working assumption: a per-user pool keyed by token subject with idle
  eviction; the design doc must size it.
- **Q-6 (Phase 3):** Is `restore_file_version` agent-appropriate? Working
  answer: yes, with `destructiveHint: true` and a reversible result (it is how
  a human uses Ctrl+Z); confirm with the web-client owners that a restore shows
  up as an ordinary change in the history view.
- **Q-7 (Phase 0):** Eval runner — headless `claude -p` (no new dependency,
  exercises the Claude Code host) or the Claude Agent SDK (programmatic
  scoring)? Pick whichever yields machine-readable transcripts with the least
  ceremony; both are acceptable.

## 8. Risks

| Risk | Mitigation |
|------|-----------|
| Landing conflict with bd-r62zad5b (author-id transition, in progress, same files) | Coordinate before Phase 1; prefer landing Phase 1 after it, or rebase plan |
| Bundle-size growth (parser WASM, zip, docs corpus) erodes cold-start | Measure in S-1; keep CAP-11/CAP-13 lazily loaded where the bundler allows |
| `render_project` normalizes arbitrary code execution through MCP | Opt-in flag, annotations, docs; never enabled by `--print-config` output |
| New flake modes in an already-flaky test neighborhood (8 open hygiene strands) | Phase 0 harness is offline-only; live-hub tests gated like existing ones |
| Remote-MCP design doc expands into a hub epic | Time-box the design doc; implementation is a separate plan if approved |
| SDK v2 migration churn (the v2 line is ten weeks old; 2.3.0 shipped 2026-10-02 with the one-server-per-request change; 2.3.1 is current) | Pin the minor; keep `handleTool` transport- and SDK-agnostic so a revert is registration-only; the Phase 1 dual-era test catches host regressions |
| Write acknowledgement adds latency to every write | Bounded (≈2 s default), per-call opt-out, and a hub ack is tens of milliseconds on a healthy link; measured by the eval suite |
| Agent-task evals are costly and nondeterministic | Phase-gate only, small suite, committed transcripts; a trend instrument, not a pass/fail gate |
| Tool count creeps past the budget as CAPs land | ERG-5 rules plus the Phase 0 count test |

## 9. Success criteria

An agent connected via `q2 mcp` can, with no shell access and starting from
nothing but a share link:

1. Connect to a project, report its shape and health, and enumerate the
   caller's project collections (full "list my projects" without a
   collection id is gated on the Phase 6 hub-side registry).
2. Read, search, create, edit (string- and AST-level), rename, and delete text
   and binary files; create and delete folders (folder rename is deferred,
   CAP-6).
3. Watch for collaborator edits with cancellable, progress-reporting calls —
   or subscribe to resources in hosts that support them.
4. See who else is in the project and what changed while it was away.
5. Render the project and iterate on structured diagnostics until clean.
6. Answer Quarto usage questions from the embedded docs.
7. Hand the human a share URL for what it built.

…with every result structured and schema-validated, every long operation
cancellable, and every capability advertised through protocol-native discovery.

Measured, not asserted (ERG-7 suite, recorded per phase):

- Every write result reports `synced: true` against a healthy hub, and no eval
  transcript contains a lost or clobbered collaborator edit when
  `expected_hash` is supplied.
- Median turns per eval task do not regress between phases; `isError` results
  caused by argument shape or stale state trend to zero.
- Default tool listing ≤ 24. Before the Phase 6 registry listing ships, the
  official conformance suite is green at `--spec-version 2026-07-28` with an
  empty expected-failures baseline.
- A human watching in the web client sees the agent's edits attributed to the
  agent (Q-3), not to themselves.
