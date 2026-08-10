# Project profiles: port from Quarto 1 (bd-fu16z22k)

**Strand:** bd-fu16z22k (related: bd-ev8mk1rp, bd-mlj6)
**Status:** plan under iteration — not yet approved for execution
**Session goal (secondary):** this session doubles as a prototype for a
"feature porting" skill; after execution we will write up the process.

## Overview

Port Quarto 1's *project profiles* to Q2: activation via `--profile` /
`QUARTO_PROFILE`, `profile.default` + `profile.group` config, profile
config overlays (`_quarto-<name>.yml`), local config overrides
(`_quarto.yml.local`), profile-specific environment files
(`_environment-<name>`, seam left by PR #486), and conditional content
(`when-profile` — greenfield in Q2, built as the full
format/profile/meta trio). Q1 guesses silently in many corner cases;
Q2 will validate strictly with span-carrying diagnostics.

### ⚠️ Terminology

Q2 already uses "profile" for **`DocumentProfile`** — the pass-1
document summary and its cache (`document_profile.rs`,
`profile_cache.rs`, `PROFILE_KEY_VERSION`, `--clean-cache`). This
feature is **"project profiles"**. In code: `active_config_profiles`,
`ProjectProfileConfig`, `profile_config_paths` — never a bare
`profiles` field, and never reuse the `"profiles"` cache namespace.

## Q1 reference (what we're porting)

Q1 sources: `src/quarto-core/profile.ts`,
`src/project/project-profile.ts`, `src/quarto-core/dotenv.ts`,
schema `resources/schema/definitions.yml` (`project-profile`, closed
object: `default: maybeArrayOf string`, `group: maybeArrayOf (arrayOf
string)`). Docs: `quarto-web/docs/projects/profiles.qmd`.

Activation (first non-empty source wins):
1. `--profile` CLI (in Q1 literally overwrites `QUARTO_PROFILE`; *replaces*, never merges)
2. `QUARTO_PROFILE` process env var
3. `QUARTO_PROFILE` key read from `_environment.local`, then `_environment` (never from `_environment-<p>`)
4. `profile.default` in `_quarto.yml.local`
5. `profile.default` in `_quarto.yml`
6. (dropped in Q2: `RSTUDIO_PRODUCT=CONNECT` → `connect`)

Then group expansion: for each group in `profile.group` (read from
`_quarto.yml` only), if no member is active, append the group's
**first** member. The normalized list becomes the canonical active
set, visible to subprocesses as `QUARTO_PROFILE`.

Config merge order (lowest → highest): `_quarto.yml` → active
profiles' `_quarto-<p>.yml` in **reverse activation order**
(first-listed profile wins) → `_quarto.yml.local`. `.yml` preferred
over `.yaml`. Project root only; no profile variant of
`_metadata.yml`. The `profile:` key is read from the base config then
stripped before merging.

Separators in the profile string: `/[ ,]+/` (comma and/or space;
colons are NOT separators).

### Q1 → Q2 divergence table (deliberate)

| Behavior | Q1 | Q2 (this port) |
|---|---|---|
| `--profile` implementation | mutates process env (`Deno.env.set`) | data plumbed through `ProjectContext`; process env never mutated (policy from PR #486) |
| Unknown active profile | silent | **Q-5-19 warning** (silenced by declaring the name in `profile.default`/`group`) |
| Empty names from `" a,b"` | `["", "a", "b"]` pollutes list | trimmed; empty segments dropped; fully-empty selection = error |
| Mixed-shape `group` (`[a, [b,c]]`) | silently zero groups | **error** with span |
| Unknown keys under `profile:` | schema error (Q1 has schema) | **error** (closed-object check; one of the few places Q2 validates shape since there's no schema layer) |
| `profile:` inside `_quarto-<p>.yml` | silently inert (merged back in, ignored) | **warning** (inert + stripped) |
| Non-string `profile.default` entries | `String()` coercion (base) / uncoerced (.local) | **error** with span |
| Array merge in overlays | union-concat with dedup | Q2 `MergeOp::Concat` (append, no dedup); users control via `!prefer`/`!concat` tags |
| `metadata-files` inside profiles | documented as unresolved (wart) | moot for now — Q2 has no `metadata-files`; whether to add the feature at all is bd-spb7mobo (if ported, fix the wart rather than replicate it) |
| Connect auto-detect | yes (undocumented) | dropped; revisit with Connect support |
| Preview on profile change | terminates itself | config read at session start; restart to pick up (matches PR #486 env-file decision); document it |
| Active profile echo | deliberately not printed | printed at `-v` only; normal output stays quiet |
| Profile name charset | anything (incl. empty strings, path chars) | strict: `[A-Za-z0-9][A-Za-z0-9._-]*`, else Q-5-21 error |

### Decisions locked with Carlos (2026-08-10)

- Conditional content: build the **full trio** (`when-`/`unless-` ×
  `format`/`profile`/`meta` on `.content-visible`/`.content-hidden`
  divs and spans) as the final phase.
- Multi-profile precedence: **first-listed wins** (Q1 parity;
  consistent with PR #486's `_environment-<p>` layer order).
- Unknown active profile: **warning**, not silent, not fatal.
- Port `QUARTO_PROFILE`-from-`_environment` bootstrap: **yes**.
- Port `_quarto.yml.local`: **yes** (both roles: `profile.default`
  source and highest-priority merge layer).
- Connect auto-detect: **no**.
- Profile-name charset: **strict, error** (Q-5-21). Rule:
  `[A-Za-z0-9][A-Za-z0-9._-]*` — filename-safe, no leading `.`, no
  internal whitespace. Applies to names from every source (CLI, env,
  `profile.default`, `profile.group`).
- Empty `--profile` value: folded into Q-5-21 (it is an error code).
- Resolved active set echoed **at `-v` only**; normal output stays
  quiet (Q1 parity).
- `when-meta`: port Q1's **dotted-path lookup + truthiness** of the
  resolved metadata value.

## Architecture

### Where resolution happens

Everything lands in `ProjectContext::parse_config`
(`crates/quarto-core/src/project/mod.rs:1321`), between
`yaml_to_config_value` (~line 1340) and `resolve_project_type`:

1. Parse base `_quarto.yml` → `ConfigValue` (existing).
2. Extract + strip `profile:` → typed `ProjectProfileConfig
   { default: Vec<String>, group: Vec<Vec<String>> }` (strict
   validation here).
3. Parse `_quarto.yml.local` (if present) early to get its
   `profile.default`; hold its `ConfigValue` for the merge.
4. Resolve activation (see order above; env via `runtime.env_get`,
   dotenv bootstrap via a minimal read of `_environment{,.local}`
   once PR #486's parser is on main).
5. Read each active profile's `_quarto-<p>.yml`/`.yaml` →
   `ConfigValue` (strip+warn `profile:` keys).
6. Merge with `quarto-config`'s `MergedConfig`: layers lowest-first =
   `[base, profiles in reverse activation order, local]`;
   `materialize()` result becomes the `metadata` that steps 5–7 of
   `parse_config` already consume. **`MetadataMergeStage` needs zero
   changes** — `project.type`, `output-dir`, `render`, `resources`,
   pre/post-render, and `brand` become profile-aware for free.

Because the merged `ConfigValue` carries `SourceInfo` from the overlay
files, span integrity depends on registering those files everywhere
`_quarto.yml` is registered today (see "Source tracking").

### Profile selection input (no env mutation)

- `ProjectContext::discover(path, runtime)` keeps its signature and
  resolves activation itself (reading `QUARTO_PROFILE` through
  `runtime.env_get` — WASM/hub runtimes return nothing → profiles
  simply inactive there, which is correct for now).
- New `ProjectContext::discover_with_profile(path, runtime,
  Option<&[String]>)` (name TBD) carries an explicit CLI selection;
  `discover` delegates with `None`. CLI passes `--profile` values;
  `Some` replaces the env var entirely (Q1 semantics). All ~15
  existing `discover` call sites stay source-compatible.
- Resolved state stored on `ProjectConfig`:
  `active_config_profiles: Vec<String>` (normalized, activation
  order) and `profile_config_paths: Vec<PathBuf>` (overlay + local
  files actually read, for source binding / cache keys / preview).
- `render.rs` re-discovery after pre-render scripts (`render.rs:885`)
  passes the same explicit selection.

### CLI

`--profile` already exists on `q2 render` (`main.rs:143`,
`Vec<String>`) but is dropped by the `..` destructure at
`main.rs:762`. Wire it into `RenderArgs` and thread to every
`discover` call in `commands/render.rs` (`:280/:338/:423/:746/:827/:885`).
Accept both repeated flags and comma/space-separated values
(Q1-compatible split `[ ,]+`, then trim + drop empties). Add the same
flag to `preview`, `get-config`, and `publish` (all go through
`discover`). `q2 get-config` becomes the introspection surface —
"which config am I getting under `--profile x`" works for free.

### Subprocess environment (`QUARTO_PROFILE` for user code)

Q1 promise: engine code (Python/R) can read `QUARTO_PROFILE`. With
the no-mutation policy, the normalized active list is applied to
**child** processes via `Command::env` (engine subprocesses, pre/post
render scripts — the exact mechanism PR #486 built for project env
pairs). Special case vs #486's "real env always wins" filter:
`QUARTO_PROFILE` must be set on children **unconditionally** — if
`--profile b` overrode an inherited `QUARTO_PROFILE=a`, children must
see `b` (Q1 parity, where the env var was overwritten). Also insert
the normalized value into the project env map so `{{< env
QUARTO_PROFILE >}}` resolves.

### Source tracking / diagnostics plumbing

- Register overlay + local files with
  `quarto_yaml::file_id_for_filename` path-spelling discipline.
- Extend `bind_config_source` candidate lists: the convention is
  `config_path` + `extension_manifest_paths`
  (`commands/render.rs:752-757`); add `profile_config_paths`.
- Extend `MetadataMergeStage`'s `register` closure
  (`metadata_merge.rs:298-328`) so profile-file FileIds referenced by
  merged-config SourceInfos resolve in both document source contexts.
- Profile resolution diagnostics go to
  `ProjectConfig.config_diagnostics` (printed once per run).

### New error-catalog codes (subsystem `project`, Q-5-*)

| Code | Severity | Meaning |
|---|---|---|
| Q-5-19 | warning | Active profile matches nothing (no `_quarto-<p>.yml`, no `_environment-<p>`, not declared in `profile.default`/`group`) |
| Q-5-20 | error | Invalid `profile:` config shape (unknown key under `profile:`, mixed-shape `group`, non-string entries) |
| Q-5-21 | error | Invalid profile name. Names must match `[A-Za-z0-9][A-Za-z0-9._-]*` (filename-safe, no leading `.`, no whitespace); also covers empty-after-trim, including an empty `--profile` value. |
| Q-5-22 | warning | `profile:` key inside a profile overlay or `.local` file where it has no effect (in `.local`, only `default` is honored) |

(Parse failures in overlay files reuse the existing YAML Q-1-* codes;
they surface with the file's own spans.)

### Cache-key integration (correctness-critical)

`Pass1KeyInputs` (`cache_key.rs:106`) must gain: the normalized
active-profile list and `(path, bytes)` of every overlay/local file
read — otherwise switching profiles serves stale pass-1
`DocumentProfile`s. Follow the existing `metadata_files` pattern.
Comment heavily (both meanings of "profile" collide on these lines).

### Conditional content (final phase; greenfield)

- Syntax: `.content-visible` / `.content-hidden` on divs **and
  spans**, attributes `when-format`, `unless-format`, `when-profile`,
  `unless-profile`, `when-meta`, `unless-meta`.
- Semantics (from Q1's `content-hidden.lua`): different condition
  kinds AND together; multiple values within one kind OR;
  `unless-*` negates; `.content-hidden` with no conditions always
  hides; surviving nodes get the attributes stripped.
- Implementation: an AST transform in
  `crates/quarto-core/src/transforms/`, **Normalization phase**
  (content must disappear before crossref numbering counts it),
  format-agnostic registration in `build_transform_pipeline`. Reads:
  target format (ctx), active profiles (ProjectContext), merged
  metadata (for `when-meta`).
- Sub-decision to settle during the phase: format matching semantics
  (`when-format="html"` vs concrete formats like `revealjs` — Q1 has
  an alias table in `quarto.format.is_format`; port the alias
  groups we already model, document the rest).
- Strictness: unknown `when-*`/`unless-*` attribute spellings on a
  content-visible/hidden node → warning (new Q-2-* or Q-5-* code,
  decide in-phase).

### Deferred / out of scope (file follow-up strands at execution end)

- `quarto.project.profile` Lua API — Q2 has no `quarto.project`
  table at all yet; add when one exists.
- Preview hot-reload/watch of profile files (Phase-D hook noted in
  `quarto-preview/src/config.rs`); restart is the documented story.
- Auto-gitignore of `/_*.local` on project create (Q1 scaffolding
  nicety; Q2 project-create story is separate).
- Connect auto-detection.
- `metadata-file(s):` support in Q2 at all — decision strand
  bd-spb7mobo (port-and-fix-the-wart vs. deliberately drop with a
  good diagnostic).

## PR #486 coordination

Phases 0–2 below do not touch environment files and can proceed on
main now. Phase 3 (env integration) requires #486's
`environment.rs` parser and `StageContext.project_env`; it lands
after #486 merges (rebase, then fill the `&[]` seam at
`stage/context.rs:~259` and implement the `dotenvQuartoProfile`
bootstrap against the real parser). If #486 is delayed, Phase 3
waits; nothing else blocks.

## Work items

### Phase 0 — resolution core (pure logic + tests first)

- [x] Write failing unit tests for `project_profile` module:
      profile-string parsing (`[ ,]+`, trim, empty-drop, colon is
      not a separator), activation precedence (CLI replaces env
      replaces dotenv replaces local-default replaces base-default),
      group expansion (first-member default, append-after-explicit,
      multiple groups, flat vs nested list shape), strict shape
      errors (Q-5-20/21 cases), first-listed-wins ordering contract
      *(41 tests written first, observed failing on stubs, then pass)*
- [x] Implement `crates/quarto-core/src/project/project_profile.rs`:
      `ProjectProfileConfig` + `extract_profile_config` (extraction
      +strip, site-aware: BaseConfig/LocalConfig/Overlay),
      `resolve_active_profiles(inputs, &mut diags) ->
      Vec<ActiveProfile>` (name + `ProfileSource` provenance)
- [x] Add Q-5-19..22 entries to
      `crates/quarto-error-catalog/error_catalog.json` (docs_url
      suffix rule; catalog audit test green)

### Phase 1 — config overlays

- [x] Failing integration tests (quarto-core `tests/integration/`):
      overlay merge (scalar override, map deep-merge, array concat,
      `!prefer`), first-listed-wins with two profiles,
      `_quarto.yml.local` over profiles, `.yml` over `.yaml`,
      `profile:`-in-overlay warning, unknown-profile warning,
      span integrity of a diagnostic pointing into an overlay file
      *(25 tests in `project_profile_overlays.rs`, written first,
      observed failing on the delegating stub)*
- [x] Implement overlay discovery + merge in `parse_config`
      (`apply_project_profiles`); `active_config_profiles` +
      `profile_config_paths` on `ProjectConfig`;
      `discover_with_profile` entry point (`discover` delegates with
      `None` and reads `QUARTO_PROFILE` via `runtime.env_get`)
- [x] `_quarto.yml.local` early parse (profile.default) + final layer
      (`.yml.local` preferred over `.yaml.local`)
- [x] Register overlay files everywhere merged-value FileIds can
      surface: `MetadataMergeStage` register closure, render.rs
      config-sources (×2), `RenderScriptsContext` (+5 construction
      sites in render/publish/preview), `project_resources` (×2),
      `compile_theme_css::theme_error_candidates`
- [x] Verify: profile-aware `output-dir` / `render:` lists /
      pre-render scripts / resolved `ProjectContext.output_dir` via
      tests
- Note: the `QUARTO_PROFILE`-env-var glue through `runtime.env_get`
  is exercised end-to-end in Phase 2 (real process env through the
  binary); unit-testing it would need a mock-env `SystemRuntime`
  wrapper, which no other test needed yet.

### Phase 2 — CLI + cache + subprocess env var

- [ ] Failing tests: `--profile` reaches discover (incl. re-discovery
      at render.rs:885), comma-and-repeated flag forms, `--profile`
      replaces `QUARTO_PROFILE`, get-config shows overlay values
- [ ] Wire `--profile` into `RenderArgs` + `discover_with_profile`;
      add flag to preview / get-config / publish
- [ ] `Pass1KeyInputs` + `pass1_key`: active list + overlay bytes;
      test that profile switch changes the key
- [ ] `QUARTO_PROFILE` on child processes (engines, render scripts)
      unconditionally; normalized value in project env map for the
      `env` shortcode (this sub-item moves to Phase 3 if #486 has
      not landed yet)
- [ ] E2E: `cargo run --bin q2 -- render fixture --profile x`,
      inspect output (record invocation + snippet here)

### Phase 3 — environment integration (after PR #486 merges)

- [ ] Rebase over #486; failing tests: `_environment-<p>` layering
      order (local > first profile > … > `_environment`),
      `QUARTO_PROFILE` read from `_environment{,.local}` (bootstrap,
      not from profile variants), bootstrap loses to real env/CLI
- [ ] Fill the `&[]` seam in `StageContext::new`; implement
      bootstrap read in activation resolution; close the loop on
      Q-5-19's "no `_environment-<p>` either" clause
- [ ] Close bd-ev8mk1rp (superseded/implemented)

### Phase 4 — conditional content (full trio)

- [ ] Failing tests: div + span visibility for when/unless ×
      format/profile/meta; AND-across-kinds / OR-within-values;
      bare `.content-hidden`; attribute stripping on survivors;
      crossref interaction (hidden float does not consume a number);
      unknown-attribute warning
- [ ] Implement `conditional_content` transform (Normalization
      phase), register format-agnostically in
      `build_transform_pipeline`
- [ ] Settle + document format-alias matching semantics
- [ ] E2E render with `--profile`, inspect emitted HTML

### Phase 5 — docs, fixtures, wrap-up

- [ ] `docs/guides/projects/profiles.qmd` (user-facing; render with
      `cargo run --bin q2 -- render docs/`); update
      `environment.qmd` profile section
- [ ] smoke-all fixture(s) under `crates/quarto/tests/smoke-all/`
      (investigate how fixtures can carry CLI flags/env; if they
      cannot, cover via integration tests and say so here)
- [ ] Divergence table above reviewed & mirrored into docs where
      user-visible
- [ ] File deferred-work strands (Lua API, preview watch, gitignore
      scaffolding); update/close bd-mlj6
- [ ] Full gates: `cargo build --workspace`, `cargo nextest run
      --workspace`, `cargo xtask verify` (full — quarto-core is in
      scope), commit protocol per CLAUDE.md
- [ ] Write the "feature porting" process doc (session retrospective)

## Open questions for plan iteration

*(none — all resolved 2026-08-10; see "Decisions locked" above)*
