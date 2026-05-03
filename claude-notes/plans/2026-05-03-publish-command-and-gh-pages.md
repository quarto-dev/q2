# `quarto publish` scaffolding + `gh-pages` provider

**Date:** 2026-05-03
**Beads:** `bd-t3ny` (epic; phase sub-issues to be filed after design approval)
**Status:** Draft — pending user review. **Do not start implementation until
the user gives the go-ahead.**

## Overview

Build the scaffolding for Quarto 2's `publish` command and ship one
working endpoint — **GitHub Pages (`gh-pages`)** — end-to-end. The shape
of the subsystem mirrors Quarto 1 so that follow-up endpoints
(quarto-pub, netlify, posit-connect, posit-connect-cloud, confluence,
huggingface) can be added one provider at a time without re-wiring the
core.

Out of scope for this issue: every provider other than gh-pages, single-
document publishing, the full account-management UI, and the
multi-target deployment-prompt UX. Each is a follow-up issue.

## Q1 reference architecture

Reviewed before drafting:

- `external-sources/quarto-cli/src/command/publish/cmd.ts` — CLI entry
  point, argument parsing, deployment/account resolution.
- `external-sources/quarto-cli/src/command/publish/{account,deployment,options}.ts`
  — interactive account picker + deployment resolution.
- `external-sources/quarto-cli/src/publish/provider.ts` — provider
  registry (`kPublishProviders`, `findProvider`).
- `external-sources/quarto-cli/src/publish/provider-types.ts` — the
  `PublishProvider` interface and `AccountToken`/`PublishFiles` shapes.
- `external-sources/quarto-cli/src/publish/types.ts` — `PublishOptions`,
  `PublishRecord`, `PublishDeployments`.
- `external-sources/quarto-cli/src/publish/publish.ts` — `publishSite` /
  `publishDocument`: wire render → provider → publish-record write.
- `external-sources/quarto-cli/src/publish/config.ts` — `_publish.yml`
  read/write.
- `external-sources/quarto-cli/src/publish/common/{git,publish,errors}.ts`
  — shared helpers (anonymous account, GitHub context for publish,
  staged document publish, SHA-based site upload handler).
- `external-sources/quarto-cli/src/publish/gh-pages/gh-pages.ts` — the
  provider we're porting.
- `external-sources/quarto-cli/src/core/github.ts` —
  `gitHubContext(dir)`: detect git, repo, origin URL, gh-pages
  remote/local presence, derive default `https://<user>.github.io/<repo>/`
  site URL, honor `CNAME`.

### How Q1's `publish` is layered

```
src/command/publish/        ← CLI: arg parsing, prompts, deployment + account picker
    cmd.ts
    options.ts
    deployment.ts
    account.ts

src/publish/                ← provider registry + render-for-publish
    provider.ts             ← list of providers
    provider-types.ts       ← PublishProvider interface
    types.ts                ← PublishOptions, PublishRecord, ...
    publish.ts              ← publishSite / publishDocument (calls provider.publish)
    config.ts               ← _publish.yml read/write
    account.ts              ← shared "handle unauthorized" flow
    common/
        git.ts              ← anonymous account, gh context for publish, verifyContext
        publish.ts          ← SHA-based handler used by quarto-pub/netlify
        bundle.ts           ← document staging
        data.ts             ← writePublishRecord
        errors.ts           ← throwUnableToPublish

    gh-pages/gh-pages.ts    ← one file per provider
    quarto-pub/...
    netlify/...
    rsconnect/...
    posit-connect-cloud/...
    confluence/...
    huggingface/...
```

### How Q1's `gh-pages` flow works (end-to-end)

1. Read `gitHubContext(dir)` → `{git, repo, originUrl, ghPagesRemote,
   ghPagesLocal, siteUrl, organization, repository}`.
2. `verifyContext`: bail if no git, no repo, or no origin.
3. Verify git user.name/user.email are set (worktree commits will fail
   otherwise).
4. If no gh-pages branch on remote:
   - Confirm with the user (skipped under `--no-prompt`).
   - Stash if working tree dirty; remember current branch.
   - If a *local* gh-pages exists → push it. Else → create orphan
     gh-pages branch, empty commit, push.
   - Restore branch + stash.
5. `git remote set-branches --add origin gh-pages` then
   `git fetch origin gh-pages`.
6. Render via the supplied `render(flags)` callback → `PublishFiles`.
7. Allocate a temp dir under `<project>/.quarto/scratch/quarto-publish-worktree-<rand>/`.
8. Clean up any stale prior worktrees with that prefix
   (`git worktree remove --force`).
9. `git worktree add --track -B gh-pages <tempdir> origin/gh-pages`.
10. `git rm -r --quiet .` inside the worktree (clean slate).
11. Copy render output into the worktree; write `.nojekyll` containing
    a short uuid (`deployId`).
12. `git add -Af . && git commit --allow-empty -m "Built site for gh-pages"
    && git push --force origin HEAD:gh-pages`.
13. Remove worktree.
14. If `--browser` and the inferred site URL is `<user>.github.io/...`,
    poll `<siteUrl>/.nojekyll` until it returns the deployId or 5 minutes
    pass.
15. Print "Published to ...". If first-time deploy *and* the inferred
    URL is the user's default site (`<user>.github.io`), nudge them to
    switch the source branch to `gh-pages` in repo settings.
16. Return `(undefined, verified ? Url : undefined)` — gh-pages doesn't
    persist a publish-record (the branch *is* the record), so it returns
    `undefined` for the record. `_publish.yml` is then a no-op for
    gh-pages.

Key detail: **gh-pages does not write `_publish.yml`** in Q1 because
the `(provider, branch)` pair is sufficient. Re-publish is detected by
`publishRecord(input)` calling `gitHubContext` and seeing a remote
gh-pages branch.

## Q2 mapping

### Crate organization

**Proposed:** new `quarto-publish` library crate under `crates/`,
plus a thin CLI shim in `crates/quarto/src/commands/publish.rs`.

```
crates/quarto-publish/
    src/
        lib.rs                 ← re-exports types + execute()
        provider.rs            ← PublishProvider trait + ProviderRegistry
        types.rs               ← PublishInput, PublishUx, PublishRecord,
                                  PublishOutcome, PublishSummary, PublishError,
                                  AccountToken, PublishFiles
        host.rs                ← PublishHost trait + NativeHost impl
        renderer.rs            ← PublishRenderer trait + ProjectPublishRenderer
        publish.rs             ← top-level publish flow (registry → provider.publish)
        deployment.rs          ← resolve_deployment() from _publish.yml
        config.rs              ← read/write _publish.yml (Q1-compatible reader)
        wait.rs                ← common::wait_for_deploy
        common/
            git.rs             ← git command wrappers
            github.rs          ← GitHub context discovery
            errors.rs          ← unable-to-publish formatting
        gh_pages/
            mod.rs             ← module entry
            provider.rs        ← GhPagesProvider impl

crates/quarto/src/commands/publish.rs
    ← parses CLI args, builds PublishInput/PublishUx, validates flag
      combinations (e.g. --no-wait + --browser), instantiates NativeHost,
      calls quarto_publish::execute
```

This matches Q2's pattern of moving substantive logic out of the binary
crate (compare `quarto-hub`, `quarto-navigation`, `quarto-trace`,
`quarto-doctemplate`).

### Provider trait

See the "Structural-improvement review" section below for the rationale
behind the shape; the canonical sketch lives there. Summary:

- One `publish` method (no site/document branch — Q2 always treats the
  publish target as a project).
- Errors are a structured `PublishError` enum (no `is_unauthorized` /
  `is_not_found` trait methods).
- Per-call inputs split across `PublishInput` (what), `PublishUx`
  (how interactive), `PublishHost` (side-effect surface — browser
  open, HTTP fetch, prompts).
- `dyn`-compatible: no generics on methods, no associated types
  tied to the provider impl, no `impl Trait` returns.

`PublishRenderer` is a separate trait with
`async fn render(&self, flags: RenderFlags) -> Result<PublishFiles, …>`.
For Phase 1 it's implemented by a `ProjectPublishRenderer` that wraps
`ProjectPipeline` and derives `PublishFiles` from the
`ProjectRenderSummary` (not from a filesystem walk). The trait shape is
kept narrow so it can later be implemented across an
extension-host/provider boundary.

### CLI shape

```
quarto publish [provider] [path]
    --id <id>              identifier of content to publish
    --server <server>      server to publish to
    --token <token>        access token
    --no-render            do not render before publishing
    --no-prompt            do not prompt
    --no-browser           do not open browser after publishing
    --no-wait              do not wait for the deployment to be live
                           (incompatible with --browser; pass --no-browser too)
    --dry-run              prepare and stage the deployment but do not
                           push/upload anything; print (or emit, under
                           --json) what would be deployed
    --json                 emit machine-readable output (implies
                           --no-prompt; final PublishOutcome on stdout,
                           NDJSON events on stderr)
```

`provider` and `path` are positional and either may be absent (if
`provider` looks like an existing path, swap them — Q1 behavior).

Argument validation (in `commands/publish.rs`, before dispatching):

- `--no-wait` together with browser-on (the default) → reject with
  "Refusing to open the browser to a deployment that may not yet be
  live. Pass --no-browser if you really want --no-wait."
- `--json` together with `--prompt` (explicit) → reject. (`--json`
  forces `--no-prompt`; specifying `--prompt` alongside is a config
  error.)
- `--dry-run` together with `--browser` → silently downgrade to
  `--no-browser` (we have no URL to open) with a one-line note on
  stderr (or as a `dry-run-no-browser` event under `--json`).

For Phase 1, `provider` is required (`gh-pages`) and `path` defaults to
the cwd. We can defer the interactive provider picker.

The `--wait` toggle can also be set per-project under
`publish.gh-pages.wait` in `_quarto.yml` (CLI flag wins; see the
"`_quarto.yml` schema" section above). The deployment-waiting
machinery lives in `common::wait_for_deploy` so future providers
(Netlify, etc.) plug into the same `--no-wait` semantics without
duplicating the logic.

### Render integration

Q1 calls `render(project.dir, ...)` and walks `projectOutputDir(project)`
for files. Q2 equivalent:

1. `ProjectContext::discover(path, runtime)`
2. Reject with `Q-PUBLISH-NOT-PROJECT` if not a website / book /
   manuscript project (Phase 1 supports websites only).
3. Reuse `ProjectPipeline::run()` (the same path `quarto render` uses).
4. Walk `project.output_dir` after the render, collect every file as
   `PublishFiles { base_dir: output_dir, root_file: "index.html",
   files: [...] }`.

### `_publish.yml`

Phase 1 includes read-side support (so future providers can use it) but
gh-pages doesn't write to it (matches Q1 — gh-pages re-detects from
git state, not from `_publish.yml`).

## Phase 0 — provider trait + CLI scaffolding (TDD)

**Tests written first:**

- `ProviderRegistry::find` returns `None` for unknown names and
  `Some(_)` for `"gh-pages"`.
- `ProviderRegistry::register` allows runtime registration of a new
  provider (proves the registry is open for future extension-loaded
  providers).
- CLI argument validation: `--no-wait` without `--no-browser` → error
  with the expected message.
- `commands::publish::execute` happy path: known provider, project
  path that's a real Q2 project context — dispatches to the provider
  (for Phase 0 this hits an `unimplemented!()` after the lookup,
  proving the wiring).
- Error path: unknown provider name → clear error listing available
  providers.

**Implementation:**

- New crate `quarto-publish` registered in workspace.
- Trait + type definitions: `PublishProvider`, `PublishRenderer`,
  `PublishHost`, `PublishInput`, `PublishUx`, `PublishRecord`,
  `PublishOutcome`, `PublishSummary`, `PublishError`, `AccountToken`,
  `PublishFiles`.
- `ProviderRegistry` with built-in registration of `gh-pages`.
  `publish()` is `unimplemented!()` for now — Phase 0 exercises only
  trait shape, registry lookup, CLI plumbing.
- `NativeHost` impl in `host.rs` (browser open via the platform's
  default opener; HTTP fetch for the deploy poll; prompts via
  `dialoguer` or similar).
- Wire `crates/quarto/src/commands/publish.rs` to call
  `quarto_publish::execute(options)`. Drop the `NotImplemented` stub.
- Confirm `cargo xtask verify --skip-hub-build` passes.

## Phase 1 — `gh-pages` end-to-end (TDD)

**Tests written first:**

Unit tests:

- `common::git::run_git` returns stdout/stderr cleanly on success and
  on non-zero exit.
- `common::github::github_context_for_publish(dir)` returns the
  expected struct for fixtures: (a) not-a-repo, (b) repo with no
  origin, (c) repo with origin but no gh-pages branch, (d) repo with
  origin and gh-pages branch on a *bare local remote*.
- `verify_context` rejects (a) and (b) with a `Q-PUBLISH-NO-GIT` /
  `Q-PUBLISH-NO-ORIGIN` diagnostic message.
- `_publish.yml` reader parses Q1's array-of-mapping shape.

End-to-end test (gated, native-only):

- Set up: temporary dir → `git init --bare remote.git` → clone it →
  add a minimal Quarto website project (`_quarto.yml` +
  `index.qmd`) → commit + push → run
  `quarto publish gh-pages` against the clone with `--no-prompt
  --no-browser`.
- Assert: a `gh-pages` branch exists on the bare remote and contains
  a non-empty `index.html` plus a `.nojekyll` file. (We can re-clone
  the bare repo into a second temp dir at `gh-pages` to inspect.)

**Implementation:**

- `common::git`:
  - `run_git(args, cwd)` thin wrapper over `std::process::Command`.
  - `git_version()`, `git_branch_exists(name, cwd)`,
    `git_user_identity_configured(cwd)`,
    `git_current_branch(cwd)`, `git_dir_is_clean(cwd)`,
    `git_stash`/`git_stash_apply`.
- `common::github`:
  - `github_context(dir) -> GithubContext`: replicates Q1's
    `gitHubContext` (origin URL, gh-pages remote/local, derived site
    URL, CNAME handling, organization/repository). Supports both
    `git@host:org/repo[.git]` and `https://host/org/repo[.git]` URLs.
  - `github_context_for_publish(input) -> GithubContext`: also reads
    `website.site-url` from project config and overrides `siteUrl`
    when set.
  - `verify_context(ctx, "GitHub Pages")` → `Result<(), PublishError>`.
- `gh_pages::provider::GhPagesProvider`:
  - `publish_record`: returns `Some({id: "gh-pages", url: site_url})`
    when `ctx.gh_pages_remote` is true.
  - `authorize_token`: verify context, return anonymous account.
  - `prepare`: implements steps 3–11 of Q1's flow above (verify
    context + git identity, ensure local gh-pages branch via
    orphan-init if needed, sync from remote, render via
    `renderer.render(...)`, allocate scratch worktree, clean stale
    worktrees, `git worktree add --track -B gh-pages`, `git rm -r .`,
    copy render output in, write `.nojekyll` with deploy id, `git add
    -Af`, `git commit --allow-empty`). Stash provider state (worktree
    path, deploy id, target site URL) in the `Box<dyn Any>`. Builds
    the `PublishAction` plan ("would push commit <SHA> to
    origin/gh-pages: N files, S bytes").
  - `commit`: runs `git push --force origin HEAD:gh-pages` and
    cleans up the worktree.
  - `verify`: implements step 14 — polls `<siteUrl>/.nojekyll` until
    it returns the deploy id or the timeout elapses. Driven by
    `ux.wait`. Sets `outcome.verified`. Includes the default-site
    nudge (step 15) when applicable.
  - **`--dry-run` cleanup:** when the driver doesn't call `commit`,
    `prepare` is responsible for tearing down its scratch state. We
    achieve this by registering a Drop on the provider state so the
    worktree is removed regardless of whether `commit` ran. The
    local gh-pages branch advance also gets reset
    (`git update-ref` back to its prior tip) on dry-run abort so
    we don't leave the user's local branch ahead of the remote.
- `common::wait_for_deploy`:
  - `wait_for_deploy(check, timeout, host)` polls `check()` until it
    returns true, the timeout elapses, or the host's progress
    indicator is interrupted. Used by gh-pages now and by future
    providers.
- `commands::publish::execute` in the binary crate:
  - Parse options, look up provider by name, run
    `quarto_publish::execute_publish(...)`.
  - On unknown provider, print a list of available providers.

**End-to-end verification (per CLAUDE.md "End-to-end verification"
section):**

Three real-binary runs to record in the verification log before
closing the issue:

1. **Dry run.** Build a fixture in a project-local temp dir with a
   bare remote.
   `cargo run --bin quarto -- publish gh-pages --no-prompt
    --no-browser --dry-run` from the clone. Inspect the printed plan;
    re-clone the bare remote and confirm **no `gh-pages` branch was
    pushed**. Then re-check the local clone and confirm no
    `gh-pages` branch was left advanced (i.e. `--dry-run` cleaned up
    after itself).
2. **Real run.** Same setup, drop `--dry-run`. Inspect the `gh-pages`
   branch on the bare remote (clone it into a scratch dir): assert
   presence of `index.html`, `.nojekyll` (containing a deploy id),
   and any `site_libs/` artifacts. Confirm `index.html` contains the
   rendered title.
3. **JSON run.** Same setup as (2), add `--json --no-browser`.
   Confirm stdout contains a single parseable `PublishOutcome` JSON
   object; stderr contains NDJSON `PublishEvent` lines (one per
   render/upload/wait event). Confirm exit code 0.

For each, record the exact invocation, observed output snippets, and
an explicit "I inspected this" note in the verification log section.

## Phase 2+ — explicitly deferred

Listed here so they're easy to spin out as follow-up issues:

1. **Single-document publishing** — although Q2 unifies project +
   single-doc at the `ProjectContext` level, document staging
   (ensuring root is `index.html`, PDF iframe wrapper) still has to
   land before non-website single-file outputs can be published.
2. **Quarto Pub provider** — uses Q1's shared SHA-based
   `handlePublish`. Needs token storage (`~/.config/quarto/credentials/`).
3. **Netlify provider** — also uses `handlePublish`. Token + slug
   prompting.
4. **Posit Connect / Connect Cloud** — bundle-based.
5. **Confluence / HuggingFace** — out-of-scope.
6. **Account management UI** — `quarto publish accounts` and
   `quarto publish login <provider>`. Deferred until a provider
   needs persistent tokens.
7. **Deployment resolution UX** — picking among multiple `_publish.yml`
   targets when more than one matches.
8. **`--no-render` end-to-end path** — currently only tested with
   render enabled.
9. **YAML schema validation for `publish.<provider>.*`** — tracked
   separately as `bd-obcw`; blocked on Q2 gaining YAML validation
   infrastructure.
10. **Third-party provider extension mechanism** — the JS-runtime
    extension surface that registers new providers via
    `ProviderRegistry::register`. Out of scope; the Phase 0/1 design
    just preserves the option.
11. **Session-based "publish API"** — a stateful, multi-round driver
    over the same trait surface (e.g. for IDE integrations or web
    clients). The Phase 0/1 trait shape (async, serde-friendly types,
    `PublishHost` indirection, stable error codes) keeps this
    possible without committing to it.

## New `_quarto.yml` schema: `publish.<provider>.*`

This issue introduces a new top-level key in `_quarto.yml`:

```yaml
publish:
  gh-pages:
    wait: true            # default; --no-wait CLI flag overrides
```

Rationale: `_publish.yml` is "history of past deploys" (kept that
way originally so publishing produces clean version-control diffs).
Forward-looking *configuration* of a deployment — the `wait` toggle,
custom domains, future per-environment settings — belongs in
`_quarto.yml` under a `publish:` key. The `wait` toggle is the first
such config; we land it in this issue.

Resolution order:

1. CLI flag (`--wait` / `--no-wait`) wins.
2. Else `_quarto.yml` `publish.<provider>.<key>`.
3. Else built-in default (varies by provider — for gh-pages, `wait: true`).

**Follow-up: `bd-obcw`** tracks adding this key to the YAML
validation schema once Q2 has YAML validation support. Until then the
reader does best-effort parsing with explicit error messages on
malformed shapes — no schema validation, no autocomplete.

## Machine-readable output (`--json`) and non-interactive operation

We introduce `--json` in this issue. Rationale: `quarto publish` is a
prime target for AI agents and CI integrations; making them shell out
and parse human prose is a regression we shouldn't ship.

### Conventions (borrowed from `gh`, `gcloud`, `kubectl`, `cargo`)

- **`--json` implies strict non-interactive mode.** Equivalent to
  `--no-prompt`. Any required input not supplied via flags causes a
  *structured* error (one JSON object on stdout, non-zero exit), not
  a prompt.
- **Stdout = result; stderr = events.** The single final
  `PublishOutcome` JSON object goes to stdout. Progress events
  (rendering, uploading, waiting for deploy) go to stderr.
- **Under `--json`, stderr emits NDJSON events**, one per line,
  each a `PublishEvent` object: `{ "kind": "render-progress",
  "rendered": 3, "total": 5 }`, `{ "kind": "upload-progress", ... }`,
  `{ "kind": "deploy-waiting", ... }`. Without `--json`, stderr is
  the usual human-readable progress.
- **Errors are also JSON.** A failure under `--json` writes a single
  `{ "error": { "code": "Q-PUBLISH-NO-ORIGIN", "message": "...",
  "provider": "gh-pages" } }` to stdout (or stderr — see decision
  below) and exits non-zero. `PublishError` variants map 1:1 to
  stable `code` strings.

### How interactive flows work in machine-readable CLIs

Surveying existing tools:

- **`gh`, `gcloud`, `aws`, `kubectl`**: strict non-interactive mode is
  the default discipline. Stateful interactive bits (auth login) are
  factored into separate idempotent subcommands (`gh auth login`,
  `gcloud auth login`) that the caller runs *once*, ahead of time.
  Subsequent commands use cached credentials and never prompt. Under
  `--json` / non-TTY, missing required input is a clear error
  pointing to the flag.
- **`docker compose`, `terraform`**: same pattern — separate
  init/auth commands populate state, then non-interactive commands
  consume it.
- **Stateful event-stream CLIs (rare)**: a few tools (some IDE-
  integrated build tools, language-server-like protocols) emit
  structured "I need this input" events on stderr and accept
  responses on stdin. This effectively reinvents JSON-RPC over
  stdio. High complexity, high power. Out of scope for us.
- **In the limit, this is an HTTP API** — multi-round async with a
  session. `quarto publish` shouldn't grow into that, but the design
  shouldn't preclude someone building one on top.

### Decision for Quarto 2

Adopt the `gh`/`gcloud` pattern:

1. **Single-shot CLI with strict non-interactive mode under `--json`
   or `--no-prompt`.** No mid-flow input.
2. **Interactive bits get separate idempotent subcommands.** First
   one we'll need is `quarto publish login <provider>` — deferred to
   a follow-up. For Phase 1, gh-pages is anonymous so this doesn't
   bite us.
3. **The trait shape supports building a session-based API on top.**
   Concretely:
   - Provider methods are async (already decided).
   - All inputs/outputs are `serde`-serializable plain types.
   - `PublishHost` is the only place side effects happen, so a
     session-based driver can swap in a host that buffers prompts
     and returns deferred responses.
   - `PublishError` variants are stable codes, not free-form strings.
   These constraints are already in the design — they just earn
   their keep here too.
4. **Argument validation rejects `--prompt` together with `--json`**
   (mirroring the `--no-wait` + `--browser` rejection).

This is enough to keep the door open for a future "publish API"
without committing to building one.

## Resolved design decisions (from 2026-05-03 design conversation)

1. **New `quarto-publish` crate.** Matches `quarto-hub` etc.; lets us
   write integration tests without booting the whole CLI.
2. **`async_trait` on `PublishProvider`.** Pay the cost now so we
   don't have to re-shape the trait when HTTP providers land.
3. **Project-only in Phase 1.** Defer single-document publishing. Q2
   already associates *every* render — including a single bare
   `.qmd` — with a `ProjectContext`, so the provider trait should
   take "the project being published" uniformly and never branch on
   "is this a single document?". This eliminates the Q1
   `publishSite` vs `publishDocument` split at the trait level. (The
   document-staging code-path is still required *eventually* for
   non-website outputs, but it lives behind the renderer, not in
   the provider interface.)
4. **Account-token system stays minimal in Phase 1.** Just the
   `Anonymous` variant; `Authorized`/`Environment` follow when a
   provider needs them.
5. **Real git for tests.** Bare local remote in a temp dir. Worth
   the ~1–2s per test for fidelity. Helper to set this up will live
   under `quarto-publish/tests/common/` so future provider tests can
   reuse the rig pattern.
6. **`.nojekyll` deploy poll is in Phase 1, gated by an option.**
   New flag `--no-wait` (default: wait). Project metadata can also
   set `publish.gh-pages.wait: false`. The combination `--no-wait`
   together with the default `--browser` (i.e. opening a site that
   may not yet be live) is rejected at argument validation time
   with a clear error suggesting `--no-browser` or dropping
   `--no-wait`.
7. **`_publish.yml` aims for byte-compatibility with Q1.** The
   reader supports the full Q1 schema (array of mapping with `source`
   key + per-provider record arrays). The writer will emit only
   what Phase 1 needs (gh-pages doesn't write at all), but the on-
   disk shape stays Q1-compatible so users can flip projects between
   q1 and q2 freely.
8. **Phase 1 fixture is a small website project.** Single
   `index.qmd` with `_quarto.yml: project.type: website` + a title.
   More representative of real gh-pages users than a `default`
   project.

## Forward-looking constraint: hub-client (WASM) publishing

Future builds of the hub-client may allow publishing directly from
the collaborative web editor (e.g. "Publish to Netlify" from inside
the browser, no local CLI). The path we anticipate:

- A WASM-JS bridge exposes the `PublishProvider` trait surface to JS.
- Some providers (likely Netlify, Quarto Pub, anything HTTP-API-
  shaped) get JS implementations bundled into the hub-client web app,
  registered with `ProviderRegistry::register` at startup.
- A WASM-side `PublishRenderer` drives the hub-client's existing
  in-browser render pipeline (via `wasm-quarto-hub-client`) instead
  of calling out to `quarto render`.
- A browser-flavored `PublishHost` impl handles "browser open" (just
  `window.open`), HTTP fetch (browser `fetch`), and prompts (modal
  dialogs).
- Providers that shell out to native binaries (gh-pages → `git`,
  posit-connect → `rsconnect`) remain CLI-only; their `name()`s
  simply aren't registered in the WASM build.

**The `ProjectRenderSummary`-driven `PublishFiles` design above is
the load-bearing piece that makes this possible.** A WASM-side
publisher cannot walk the filesystem — it only has the render output
that came back from the in-browser pipeline. Deriving `PublishFiles`
from `ProjectRenderSummary` (rather than from a filesystem walk)
means the same provider trait works in both worlds without
"native-only" caveats baked into the contract.

The Phase 0/1 design preserves this option without doing any of the
work:

- All provider-facing types are `serde`-serializable (so they cross
  the WASM-JS boundary cleanly).
- `PublishHost` is the *only* side-effect surface (so swapping the
  native host for a browser host is a one-trait-impl change).
- `PublishRenderer` is narrow and decoupled from `ProjectContext`
  internals (so a hub-client renderer doesn't need to construct or
  serialize Q2 internals).
- Process exec is **out** of the trait surface — providers that
  need it (gh-pages) call `Command` directly inside their native-
  only impl. WASM providers naturally lack the affordance.

**Follow-up note for a future session:** When we wire up the
WASM-JS bridge, the work breaks into three pieces (none in this
issue):

1. `wasm-bindgen` (or equivalent) shims for the `PublishProvider`
   trait surface — async-friendly, JS-Promise-flavored.
2. A browser `PublishHost` impl in the hub-client React app.
3. A first JS-side provider (likely Netlify, since it's pure HTTP
   and already has a stable API) to validate the bridge end-to-end.

The trait shape locked in by `bd-t3ny` Phase 0 is the contract that
work will build against. If we discover a needed change there
(e.g. `Box<dyn Any>` for provider-private state doesn't cross the
WASM boundary cleanly), file it as a `discovered-from bd-t3ny`
issue and land the trait revision before the bridge work proceeds.

## Forward-looking constraint: third-party providers

We expect publishing endpoints to be implementable as **third-party
extensions** in the future, almost certainly via a JavaScript runtime
(QuickJS / Boa / similar). No work for that lands in this issue, but
the design must not paint us into a corner. Concrete implications:

- **Provider registry must be open, not closed.** The `Vec<&'static
  dyn PublishProvider>` registry is fine for built-ins, but the
  *lookup* path goes through a `ProviderRegistry` indirection that
  can later be augmented from extension-discovered providers
  (`registry.register(name, provider: Arc<dyn PublishProvider>)`).
  Built-ins register at construction time; extensions register at
  CLI-startup time once the extension loader runs. **No code outside
  the registry should hardcode a provider list.**
- **`PublishProvider` trait must be `dyn`-compatible and stable.**
  No generics on methods, no associated types tied to provider
  implementations, no return-position `impl Trait`. Async via
  `async_trait` (already proposed). All inputs/outputs must be
  serializable in principle (so an extension provider can receive
  inputs across an FFI/IPC boundary): use plain types
  (`PathBuf`, `String`, `serde`-derived structs), no captured
  closures or non-`'static` lifetimes in trait method signatures.
- **`PublishRenderer` is provider-facing, not project-facing.** The
  renderer trait the provider sees should expose only what a third-
  party provider needs (kick off a render, get back `PublishFiles`).
  It must not leak `ProjectContext`, `ProjectPipeline`, or other
  internal types that we don't want to commit to as a stable
  extension API. The native impl wraps `ProjectPipeline`; the
  extension impl will receive a host-side handle.
- **Side effects through a thin "host" interface.** Browser open,
  URL fetching (for `.nojekyll` poll), and prompts should go
  through a small `PublishHost` trait that providers receive,
  rather than calling `webbrowser::open` / `reqwest::get` directly.
  This makes the surface a third-party provider can rely on
  explicit and testable, and lets us swap the native host for an
  extension-bridge host later.
- **Process-execution stays *out* of the provider-facing interface.**
  Shelling out to `git` is a built-in-only concern (extensions get a
  higher-level "publish these files to this destination" interface,
  not raw process exec). gh-pages calls `std::process::Command`
  directly inside the provider impl; the trait does not expose a
  generic exec hook.

These constraints don't change the Phase 1 work shape — they just
sharpen which boundaries we treat as load-bearing.

## Structural-improvement review of Q1's `quarto publish`

Reviewing Q1's organization with fresh eyes (and with comparable CLIs
in mind: `vercel deploy`, `netlify deploy`, `wrangler publish`,
`firebase deploy`, `mkdocs gh-deploy`, `gh release create`,
`hugo deploy`, `cargo publish`), here's what I think is worth carrying
forward, what's worth simplifying, and what's worth changing.

### Things Q1 got right (keep)

- **Per-provider directory under one `publish/` namespace** with a
  shared `common/`. Easy to add a provider, easy to find provider
  code by name. Mkdocs/Hugo do similar.
- **`PublishProvider` interface as the unit of pluggability.** The
  shape (`publish_record` + `account_tokens` + `authorize_token` +
  `publish` + error classifiers) is well-factored and we should
  preserve it.
- **Render injected as a callback, not invoked by the provider.**
  Providers don't import the renderer — they get a `render(flags)`
  function. Keeps providers decoupled from the rendering pipeline.
  We keep this (as the `PublishRenderer` trait).
- **Distinction between "publish record" (project state) and
  "account token" (user state).** Two orthogonal axes: *what am I
  re-deploying?* vs *who am I authenticated as?* This is the right
  factoring; cleaner than e.g. Netlify CLI which conflates them.

### Things to simplify or change

- **Site/document split at the trait level.** Q1's `publish.ts` has
  parallel `publishSite` / `publishDocument` paths because Q1 keeps
  "single doc" and "project" as different shapes all the way through.
  Q2 already associates a `ProjectContext` with every render — even
  bare single-file `.qmd` renders go through a single-file
  `ProjectContext`. We exploit this: **the provider trait takes a
  uniform `PublishInput` (always project-shaped)**, and document-
  specific staging (the `index.html` / pdf-iframe wrapper that Q1's
  `stageDocumentPublish` does) is the renderer's job, not the
  provider's. Net result: one `publish` method per provider, no
  branches on `kind: "document" | "site"` inside provider code.
- **Deployment resolution belongs in core, not in the CLI command
  module.** Q1 has the `_publish.yml` reader in `src/publish/config.ts`
  *and* `src/command/publish/deployment.ts` (the picker). Keep all
  of that under `quarto-publish` so `_publish.yml` semantics are
  testable without the CLI driver. The CLI module shrinks to "parse
  args, build options, hand off."
- **Error classifiers (`isUnauthorized`, `isNotFound`) on the trait
  feel ad hoc.** Q1 uses these to drive the "re-authenticate and
  retry" loop in `cmd.ts`. Better Q2 shape: providers return a
  structured `PublishError` enum with explicit
  `Unauthorized`/`NotFound`/`Other` variants, and the retry loop
  matches on that. Removes two trait methods and one source of
  silent mis-classification.
- **`PublishOptions` is a grab bag.** Q1's `PublishOptions` mixes
  `input` (the thing being published) with CLI ergonomics (`prompt`,
  `browser`) and credentials (`server`, `token`). Q2 should split:
  `PublishInput` (project context + kind), `PublishUx` (prompt /
  browser / wait), `ProviderCredentials` (per-provider, optional).
  Easier to mock, easier to test, and `ProviderCredentials` becomes
  a per-provider extension point that third-party providers can
  define their own shape for.
- **`PublishFiles` walks the filesystem inside the renderer
  callback.** Q1's `publishSite` walks `projectOutputDir` after
  render to build the file list. That's a leak: the renderer
  result already knows what it produced. Q2's `ProjectPipeline`
  returns a `ProjectRenderSummary` with concrete output paths;
  `PublishFiles` should be derived from that summary, not from a
  filesystem walk. Side benefit: handles output-dir contents that
  weren't part of the render (e.g. user's `.htaccess`) deliberately
  rather than by accident — we choose what to include.
  *Additional benefit (see "Forward-looking constraint: hub-client
  (WASM) publishing" above):* a future WASM-side publisher cannot
  walk the filesystem at all, so this design is the contract that
  makes browser-driven publishing possible.
- **Anonymous-account boilerplate.** Q1 has every provider that
  doesn't need auth still construct an `anonymousAccount()`.
  Cleaner: the trait's `account_tokens` returns `Vec<AccountToken>`,
  default impl returns `vec![AccountToken::anonymous()]`, and
  providers without auth never override.
- **"Verify deployment landed" should be a generic concept, not
  gh-pages-specific.** The polling loop in Q1's gh-pages provider
  is reusable — Netlify also wants "wait until live." Lift to
  `common::wait_for_deploy(check: impl Fn() -> bool, timeout)` so
  every provider plugs into the same `--no-wait` semantics
  uniformly.

### Lessons worth borrowing from comparable CLIs

- **`vercel`/`netlify deploy`: deploy URLs are distinct from
  production URLs.** Their CLIs return both a "preview deploy URL"
  and a "production URL." gh-pages doesn't have the concept (it's
  push-or-not), but the `PublishRecord` should leave room for it
  (e.g. `url` vs `admin_url`) so future providers don't need a
  shape change. Q1 already does this; we keep it.
- **`wrangler publish`/`firebase deploy`: deployment manifest is
  declarative + per-environment.** `wrangler.toml` has top-level
  `[env.production]`, `[env.staging]`. Q1's `_publish.yml` is more
  like "history of past deploys" than "configuration for future
  deploys." We keep Q1's shape (it's what users have), but we should
  recognize that *configuring* a deployment (e.g. "this site uses
  custom domain X") may eventually want to live in `_quarto.yml`
  under a `publish:` key, not in `_publish.yml`. The `wait` toggle
  is the first such config — we put it under
  `publish.gh-pages.wait` in `_quarto.yml`, not in `_publish.yml`.
- **`mkdocs gh-deploy`: aggressive defaults + a single sharp tool.**
  `mkdocs gh-deploy` does exactly what `quarto publish gh-pages`
  does (and was likely a reference for Q1's design). Their UX
  lessons worth stealing: clear "force-push detected, are you
  sure?" warning when the gh-pages branch was modified by another
  tool; explicit "what was deployed" summary at the end (commit
  SHA, file count, total size). Q1 prints "Published to <URL>" —
  thin compared to mkdocs.
- **`cargo publish`: dry-run is first-class.** `--dry-run` runs the
  full machinery up to the actual upload and stops. Worth adding
  to `quarto publish` (renders, prepares the worktree, prints what
  *would* be pushed, doesn't push). Defer to a follow-up issue but
  note here so the architecture supports it (the publish flow
  should be structured as `prepare → commit → upload` so `--dry-run`
  cuts at the right seam).
- **`gh release`: machine-readable output via `--json`.** Useful
  for CI integration. Defer, but `PublishRecord` should be `serde`-
  serializable (it already is in the design).

### Shape-of-the-trait recommendations rolled into the plan

I've folded the structural changes above back into the trait sketch
below — the changes from the original draft are:

- `publish_site` / `publish_document` collapse to one `publish`.
- `is_unauthorized` / `is_not_found` removed; surfaced in
  `PublishError`.
- Old `PublishOptions` split into `PublishInput`, `PublishUx`,
  per-provider `Credentials`.
- `PublishHost` trait introduced (browser open, HTTP fetch for
  poll, prompt). Native impl in `quarto-publish`; extension impl
  later.
- `ProviderRegistry` is the single lookup path (no public
  hardcoded `Vec`).
- `wait_for_deploy` lives in `common`, not in `gh_pages`.
- **The publish flow is split into `prepare → commit → verify`** to
  give `--dry-run` (and any future "deploy planner") a clean cut
  point. `prepare` is the side-effect-free planning + render +
  staging step; `commit` is where the network/git push happens;
  `verify` is the optional post-commit deploy poll.

```rust
#[async_trait::async_trait]
pub trait PublishProvider: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn requires_server(&self) -> bool { false }
    fn requires_render(&self) -> bool { true }
    fn hidden(&self) -> bool { false }

    async fn publish_record(
        &self,
        input: &PublishInput,
        host: &dyn PublishHost,
    ) -> Result<Option<PublishRecord>, PublishError>;

    async fn account_tokens(
        &self,
        host: &dyn PublishHost,
    ) -> Result<Vec<AccountToken>, PublishError> {
        Ok(vec![AccountToken::anonymous()])
    }

    async fn authorize_token(
        &self,
        input: &PublishInput,
        host: &dyn PublishHost,
    ) -> Result<Option<AccountToken>, PublishError>;

    /// Plan and stage the publish. Side-effect rules:
    /// - May read from disk and the local git state.
    /// - May render (via `renderer`) and copy files to a staging
    ///   area inside the project's scratch dir.
    /// - May make read-only network calls (e.g. detect remote
    ///   branch presence) but **must not push, upload, or otherwise
    ///   mutate the destination.**
    /// Returns a `PreparedPublish` describing what would be
    /// committed.
    async fn prepare(
        &self,
        account: &AccountToken,
        input: &PublishInput,
        renderer: &dyn PublishRenderer,
        ux: &PublishUx,
        host: &dyn PublishHost,
        target: Option<&PublishRecord>,
    ) -> Result<PreparedPublish, PublishError>;

    /// Push/upload the prepared publish to the destination. After
    /// this returns Ok, the deployment is irrevocable from the
    /// CLI's perspective (rollbacks are a destination-specific
    /// matter — git revert + push, Netlify rollback, etc.).
    async fn commit(
        &self,
        prepared: PreparedPublish,
        host: &dyn PublishHost,
    ) -> Result<PublishOutcome, PublishError>;

    /// Optional post-commit verification (e.g. poll the live URL
    /// for `.nojekyll`). No-op for providers that don't support
    /// post-deploy verification. Driven by `ux.wait`.
    async fn verify(
        &self,
        outcome: &mut PublishOutcome,
        ux: &PublishUx,
        host: &dyn PublishHost,
    ) -> Result<(), PublishError> {
        Ok(())
    }
}

pub struct PreparedPublish {
    pub provider: &'static str,
    pub staging_dir: PathBuf,        // where the bytes live now
    pub files: PublishFiles,         // what's about to be uploaded
    pub destination: PublishDestination, // human-readable + structured
    pub plan: Vec<PublishAction>,    // "create gh-pages branch",
                                     // "force-push to origin/gh-pages",
                                     // "upload N files (S bytes)"
    /// Provider-private state needed for commit (e.g. the worktree
    /// path, the deploy id). Boxed-Any keeps the trait dyn-compatible
    /// without leaking provider-specific types into the public API.
    pub provider_state: Box<dyn std::any::Any + Send + Sync>,
}

pub struct PublishOutcome {
    pub record: Option<PublishRecord>,
    pub url: Option<Url>,           // production URL
    pub admin_url: Option<Url>,     // admin/dashboard URL (Q1 parity)
    pub summary: PublishSummary,    // commit SHA, file count, bytes
    pub verified: bool,             // set by verify()
}

pub enum PublishError {
    Unauthorized { provider: &'static str, source: anyhow::Error },
    NotFound { provider: &'static str, source: anyhow::Error },
    UnableToPublish { provider: &'static str, message: String },
    Other(anyhow::Error),
}
```

### What `--dry-run` does

The top-level `publish.rs` driver runs:

```rust
let prepared = provider.prepare(account, input, renderer, ux, host, target).await?;
if ux.dry_run {
    host.report_plan(&prepared);   // human or NDJSON depending on --json
    return Ok(PublishOutcome::dry_run(prepared));
}
let mut outcome = provider.commit(prepared, host).await?;
provider.verify(&mut outcome, ux, host).await?;
Ok(outcome)
```

This means **`--dry-run` exercises the entire planning + render +
staging path** (catching almost every bug a real publish would hit)
but stops short of the destination-mutating step. Concretely for
gh-pages, `--dry-run`:

- Detects git state, origin, and gh-pages branch presence.
- Renders the project to its output dir.
- Creates the worktree, cleans it, copies render output, writes
  `.nojekyll`, runs `git add` and `git commit` (commits to the
  worktree's local gh-pages branch).
- **Stops before** `git push --force origin HEAD:gh-pages`.
- Reports the plan: target remote, branch, file count, bytes,
  commit SHA that *would* be pushed.
- Cleans up the worktree (we delete the local commit too — `--dry-run`
  must not leave residue).

### What the trait does *not* do

- **Provider-specific state crosses `prepare → commit` via
  `Box<dyn Any>`.** That's a deliberate tradeoff: it keeps the trait
  dyn-compatible and lets every provider stash whatever it needs
  (worktree path, deploy id, signed URLs, ...) without leaking
  associated types. The provider downcasts inside `commit`. The
  alternative (typed associated state) breaks dyn-compatibility,
  which we need for the `ProviderRegistry`.

## Work items

(Filled in once design is approved. Sketched here for shape.)

Phase 0 — scaffolding (✅ landed on `feature/publish`):
- [x] Add `quarto-publish` crate skeleton + workspace registration
- [x] Define `PublishProvider` (with `prepare` / `commit` / `verify`
      split), `PublishRenderer`, `PublishHost`, `PublishInput`,
      `PublishUx` (incl. `dry_run`, `json`, `wait`),
      `PublishRecord`, `PreparedPublish`, `PublishOutcome`,
      `PublishSummary`, `PublishAction`, `PublishEvent`,
      `PublishError`, `AccountToken`, `PublishFiles`,
      `PublishDestination`
- [x] `ProviderRegistry` (open for runtime registration) with
      built-in `gh-pages` (prepare/commit unimplemented)
- [x] `NativeHost` (event/outcome rendering in human + NDJSON
      shapes; HTTP fetch and browser-open are stubbed errors and
      get filled in alongside the gh-pages `verify` step in Phase 1)
- [x] CLI argument validation: `--no-wait` + `--browser` rejected;
      `--json` + `--prompt` rejected; `--dry-run` + `--browser`
      downgraded to `--no-browser` with a note (and the validation
      ordering accepts `--dry-run --no-wait`)
- [x] Resolution helper for `publish.<provider>.*` from
      `_quarto.yml` (CLI flag → `_quarto.yml` → built-in default)
- [x] Wire `crates/quarto/src/commands/publish.rs` to call into the
      new crate; remove `NotImplemented` stub
- [x] Top-level driver runs `prepare → (dry-run? report : commit)
      → verify`; emits human or JSON output as appropriate
- [x] Tests: registry lookup + dynamic register, argument
      validation matrix, `publish.<provider>.*` resolution
      precedence, error path on unknown provider (43
      `quarto-publish` unit tests; full workspace `cargo nextest`
      green; smoke-tested CLI rejects bad combos with exit code 1
      and emits a clean JSON error envelope under `--json`)

Phase 1 — gh-pages end-to-end (✅ landed on `feature/publish`):
- [x] `common::git` wrappers + tests (15 tests)
- [x] `common::github::github_context` + tests against fixture
      repos (16 tests covering URL parse, CNAME, derived URLs,
      bare-remote gh-pages detection)
- [x] `verify_context` + diagnostic codes
      (`Q-PUBLISH-UNABLE` envelopes for no-git/no-repo/no-origin)
- [x] `_publish.yml` reader (Q1-compatible) + tests (11 tests
      covering Q1 schema, malformed shapes, unknown providers)
- [x] `GhPagesProvider::publish_record` (re-detect existing branch)
- [x] `ProjectPublishRenderer` (derives `PublishFiles` from
      `ProjectRenderSummary` — implemented in
      `crates/quarto/src/commands/publish.rs`)
- [x] `common::wait_for_deploy` + tests with a mock check fn
      (5 tests: ready, polling-then-ready, broken, timeout,
      error propagation)
- [x] `GhPagesProvider::prepare` (verify, ensure branch via
      worktree `--orphan` for first publish or `--track
      origin/gh-pages` otherwise, render, copy + `.nojekyll`,
      local commit, plan emission)
- [x] `GhPagesProvider::commit` (`git push --force` with
      `--set-upstream` on first publish, worktree cleanup)
- [x] `GhPagesProvider::verify` (`.nojekyll` poll gated on
      `ux.wait`, default-site nudge for `<user>.github.io`)
- [x] `--dry-run` cleanup: `Drop` impl on `GhPagesState` removes
      worktree and prunes the local gh-pages branch; e2e test
      confirms no residue
- [x] mkdocs-style end-of-publish summary (commit SHA + file
      count + total bytes) — emitted in human output and as
      `PublishSummary` fields in JSON output (incl. new
      `deploy_id` field)
- [x] End-to-end test against bare local remote (website
      fixture): dry-run, real-run, second-publish-force-push,
      verify-no-network, publish-record-detection (6 tests)
- [x] Manual end-to-end verification (all three runs) + log in
      this plan
- [x] `cargo xtask verify` clean (9/9 steps)

## Verification log

### 2026-05-03 — Phase 1 end-to-end verification

Fixture: `/tmp/q2-publish-test/`
- `bare.git/` — bare git repo standing in for `origin`.
- `clone/` — working clone with a minimal Quarto website project
  (`_quarto.yml: project.type: website` + a single `index.qmd`).
- Initial `main` branch pushed to `origin`.

Three runs against this fixture, output inspected each time:

**Run 1: `--dry-run`.**

```
$ cd /tmp/q2-publish-test/clone
$ q2 publish gh-pages --no-prompt --no-browser --dry-run
Preparing gh-pages publish...
Rendering for publish...
Render complete.
Plan for gh-pages:
  - Render /private/tmp/q2-publish-test/clone
  - Create remote branch 'gh-pages'
  - Upload 5 files (594554 bytes)
  - Push commit 1ccf61f62237dc9434cc0bd21c625adbc8fd93e7 to origin/gh-pages
Dry-run for gh-pages: would have published.
Files: 4 (594542 bytes)
$ git --git-dir=/tmp/q2-publish-test/bare.git branch -a
  * main
$ ls /tmp/q2-publish-test/clone/.quarto/scratch/
(empty)
```

✅ Plan emitted; **no `gh-pages` branch on origin**; **no leftover
worktree** under the project's scratch dir. Dry-run cleanup works.

**Run 2: real publish (`--no-wait` to skip the network probe).**

```
$ q2 publish gh-pages --no-prompt --no-browser --no-wait
Preparing gh-pages publish...
Rendering for publish...
Render complete.
Plan for gh-pages:
  - Render /private/tmp/q2-publish-test/clone
  - Create remote branch 'gh-pages'
  - Upload 5 files (594554 bytes)
  - Push commit 41d45fa0ed58060583d63dfc96a32ceae94a7920 to origin/gh-pages
Committing gh-pages publish...
Committed gh-pages publish.
Published via gh-pages.
Commit: 41d45fa0ed58060583d63dfc96a32ceae94a7920
Files: 5 (594554 bytes)

$ git --git-dir=/tmp/q2-publish-test/bare.git branch -a
  gh-pages
* main

$ git clone --branch gh-pages /tmp/q2-publish-test/bare.git /tmp/inspect
$ ls /tmp/inspect/
.nojekyll  index.html  site_libs

$ cat /tmp/inspect/.nojekyll
0f234c8fb17f

$ head /tmp/inspect/index.html
<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="generator" content="quarto-rust-0.1.0">
<title>Phase 1 verification</title>
<link rel="stylesheet" href="site_libs/bootstrap/bootstrap-icons.css">
<link rel="stylesheet" href="site_libs/quarto/quarto-theme-21263bc958169528.css">
</head>
```

✅ `gh-pages` branch present on origin. **Inspected** the cloned
branch: contains `index.html` (with rendered Quarto content + the
project title), `site_libs/`, and `.nojekyll` (containing the
deploy id surfaced in the outcome).

**Run 3: `--json` (machine-readable mode).**

```
$ q2 publish gh-pages --no-prompt --no-browser --no-wait --json \
    > stdout.txt 2> stderr.txt
$ echo $?
0

$ cat stdout.txt
{"provider":"gh-pages","record":{"id":"gh-pages"},"summary":{"commit":"cc89cad28f1b67865b1d0920e2d754e634cafc3f","deploy_id":"b4bab47acc57","file_count":5,"bytes":594554},"verified":false,"dry_run":false}

$ cat stderr.txt
{"kind":"prepare-start","provider":"gh-pages"}
{"kind":"render-start"}
{"kind":"render-complete"}
{"kind":"plan","provider":"gh-pages","actions":[{"kind":"render","project_dir":"/private/tmp/q2-publish-test/clone"},{"kind":"upload-files","count":5,"bytes":594554},{"kind":"push-branch","remote":"origin","branch":"gh-pages","commit":"cc89cad28f1b67865b1d0920e2d754e634cafc3f"}]}
{"kind":"commit-start","provider":"gh-pages"}
{"kind":"commit-complete","provider":"gh-pages"}

$ jq '.summary.commit' < stdout.txt
"cc89cad28f1b67865b1d0920e2d754e634cafc3f"
$ jq -c '.kind' < stderr.txt
"prepare-start"
"render-start"
"render-complete"
"plan"
"commit-start"
"commit-complete"
```

✅ Single parseable `PublishOutcome` JSON object on stdout. NDJSON
events on stderr, one per line, all parseable through `jq`.

### Build/test verification

- `cargo build --workspace`: clean.
- `cargo nextest run -p quarto-publish`: 101 tests pass (95 unit
  + 6 end-to-end integration).
- `cargo xtask verify --skip-hub-build`: ✓ all 9 steps passed
  (formatting, clippy, build, lint rules, full workspace tests,
  trace-viewer tests).

### Known gaps (filed as follow-ups)

- The `verify` step's `.nojekyll` poll cannot be exercised
  end-to-end against `localhost`/bare-remote — it requires a
  reachable URL. The probe logic is unit-tested via mocked
  `DeployProbe`s.
- First-publish nudge for `<user>.github.io` default sites is
  in the code but only triggers on `is_first_publish &&
  default_site_user(site_url).is_some()`; this fixture is not a
  default-site URL so the path wasn't exercised end-to-end.
- `--no-render` is honored (errors out — gh-pages requires
  render) but not exercised in this verification log.

