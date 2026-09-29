# Windows: q2 emits and compares plain paths where a plain form exists (bd-1klbq2zd)

## Overview

On Windows, `std::fs::canonicalize` returns verbatim `\\?\C:\…` paths, and q2 carries them into JSON wire fields (`notebook_path`, `source_file`), status lines (`Rendering single file: \\?\C:\…`), env vars for user scripts (`QUARTO_PROJECT_DIR`) and path comparisons. The wire-path contract, the UNC exception and the research live in `2026-09-28-windows-json-errors-file-url-test.md` § Piece 2; this plan does not repeat them.

The fix ships as the two upper layers of a gh stack on top of draft PR #743 (mechanics: same file, § Stacked PR workflow):

```
(main) <- bugfix/bd-clq56rem-windows-jsonerrors-ipynb-hyperlink   #743, test rework; holds the Windows RED
       <- bugfix/bd-1klbq2zd-path-audit                           one canonicalize function, audit, guard fix, spelling-agnostic oracles
       <- bugfix/bd-1klbq2zd-dunce-seam                           that function -> dunce; wire-spelling tests
```

## Findings (2026-09-29, Windows, HEAD `b70dbe52`)

**Audit size.** `canonicalize(` has 186 hits in `crates/*/src`, but 133 sit inside `#[cfg(test)]` modules (mostly mock `SystemRuntime` impls). 53 are production code, and that count includes the trait declaration and the `native`/`wasm`/`sandbox` impls. That leaves roughly 45 real call sites across about 30 files. The largest clusters are `quarto-core` `project_resources.rs`, `project/format_paths.rs`, `output_sink.rs`, `project/mod.rs` and `engine/capture_files.rs`, plus `quarto-preview` `config.rs` and `quarto-hub` `watch.rs`/`sync.rs`/`admin/collect.rs`. Only 8 go through the runtime seam; the rest call `Path::canonicalize` or `std::fs::canonicalize` directly.

**Crates that cannot reach the seam.** `quarto-hub`, `quarto-trace-server` and `quarto-source-fetch` do not depend on `quarto-system-runtime`, yet all three are linked into `q2`. `quarto`, `quarto-core`, `quarto-preview`, `quarto-test` and `wasm-quarto-hub-client` do.

**The overwrite guard already loses data on Windows.** `render_to_file.rs:647` refuses `output_path == input_path` lexically. The input arrives canonicalized (verbatim today), while `--output` passes through raw (`render.rs:963`). Tested with the dev binary, both in a scratch dir holding `doc.qmd`:

```
q2 render doc.qmd --output C:\…\guard-repro\doc.qmd           # plain spelling of the input
q2 render doc.qmd --output C:\…\guard-repro\sub\..\doc.qmd     # any OS: lexically different, same file
```

Both exit 0 and replace `doc.qmd` with the rendered HTML. The second spelling bypasses the guard on every platform, so it gives a portable RED that Linux/macOS CI also runs. After the seam switch the plain spelling becomes the one that is caught and the verbatim spelling the one that slips past, so the fix belongs in the audit layer, not the seam layer.

**Out of scope, found while probing.** A relative `--output` fails outright: `q2 render doc.qmd --output out.html` → `output destination must be absolute: out_files\styles.css` (`output_sink.rs:141`). Tracked as bd-xdlbrc7m, not part of this work.

## Decisions

Decided 2026-09-29:
- **One shared canonicalize function** in `quarto-system-runtime`, which every routed site calls. Per-site `dunce` calls were rejected (about 45 copies with nothing keeping them in sync). The name is settled in layer 2.

## Design

### Layer 2: `bugfix/bd-1klbq2zd-path-audit` (behavior-preserving, except for the guard)

**One production canonicalize function.** Add a free function in `quarto-system-runtime` (native only; name to settle during review, e.g. `quarto_system_runtime::canonicalize`) that wraps `std::fs::canonicalize` **unchanged** in this layer. `NativeRuntime::canonicalize` calls it. Audited direct `std` call sites that need a consistent spelling call the **free function**, never the runtime, even when a runtime is in scope. The runtime is not equivalent: `WasmRuntime::canonicalize` normalizes against its VFS (`wasm.rs:349`), and the test-mock runtimes return whatever they are told, so rerouting a `std` call through it would change behavior in this layer. Existing runtime calls stay as they are. Deliberately moving a site onto the runtime is out of scope unless the audit records it as its own behavior change with its own acceptance test.

A second shared function covers paths that may not exist yet: `canonicalize_deepest_existing` (now private in `output_sink.rs:420`) moves into `quarto-system-runtime` and calls the shared `canonicalize`. It canonicalizes the deepest existing ancestor, re-appends the missing tail, and keeps the lexical form on failure. Layer 3 then flips one function body, and every routed site changes spelling together, including not-yet-created outputs. That removes the plain-versus-verbatim mixing hazard that produced most of the prototype fallout.

Why not add dunce at each site: about 45 inlined copies with nothing keeping them in sync. Why not a `quarto_util` helper: `quarto_util` has no native/wasm split, and the runtime crate already owns that boundary. This refines the 2026-09-29 decision "dunce directly in `quarto-system-runtime`, no `quarto_util` helper yet"; it does not reverse it.

**Audit method.** Enumerate the production sites (the `cfg(test)` split above, re-checked per file rather than trusting the first-`#[cfg(test)]` heuristic). For each site, follow the result forward to its consumers, not just to its definition, and give it one disposition:

| Disposition | When | Action |
|---|---|---|
| **R — route** | The result is emitted (wire, status line, env var, error message) or compared, joined, `strip_prefix`ed or used as a map key against a path from another source | Call the shared function (a `std` site) or leave the runtime call (a site already on the seam) |
| **L — leave** | Both sides of every comparison come from this same call, or the result is consumed internally and never printed, and never meets a seam-derived path | No change; note why in the audit table |
| **N/A** | Trait declaration, `wasm`/`sandbox` impls, `cfg(test)` mocks | None |

**Comparisons need both operands.** Routing only the canonicalized side does not fix a comparison whose other side is a raw user path. That is exactly the overwrite-guard bug. For every R row that compares, strips a prefix or looks up a map key, the table records where **each** operand comes from and whether it must exist at that point (a planned output, for example, does not). Both operands end up in the same spelling: through `canonicalize` when the path must exist, and through the deepest-existing variant when it may not, so a missing path never becomes a new hard error. Both variants fall back to the lexical form rather than failing. At least one behavioral test per such site class feeds a differently-spelled input (verbatim on Windows, `sub/..` elsewhere).

**Crate scope is fixed up front.** In scope: every crate linked into the `q2` binary. `quarto-hub`, `quarto-trace-server` and `quarto-source-fetch` are all normal dependencies of `quarto` (`cargo tree -p quarto -e normal`), so all three ship. R sites in crates that lack the `quarto-system-runtime` dep are routed in this stack by adding that dep, which is native-only and already in the workspace. The only exception is a site the plan lists by name as outside acceptance, with a reason; each such site gets a follow-up strand.

The result goes into an audit table in this plan (site, consumer, operand origins, disposition, reason). The sweep is mechanical but spans about 30 files, so one sonnet subagent does it under a strict output contract (one row per site, evidence as `file:line`, no fix proposals), and I spot-check every R row and every out-of-acceptance exception against the source.

**Test oracles (17 in the prototype, plus any the audit adds).** Where a test checks *which* file a path names (the `classify_*`/`render_once_*` units), make the comparison spelling-agnostic by canonicalizing both sides through the shared function, so the test passes before and after layer 3. Where a test pins *what q2 emits* (`render_scripts_cli` `QUARTO_PROJECT_DIR`), keep the identity check in this layer and add the spelling assertion in layer 3 as a real RED.

**Overwrite guard (TDD, portable RED).**
1. RED: add `render_cli_e2e::output_via_dotdot_spelling_of_input_refuses_and_preserves_source`, which creates `<dir>/sub/` in the fixture (so the spelling resolves on every OS instead of naming a missing directory), runs `--output <dir>/sub/../doc.qmd`, and asserts both refusal and an untouched source. It fails today on every OS (verified above via the binary). On Windows, also run the plain-spelling case; if it can't be expressed portably, put it in a `#[cfg(windows)]` test whose body is the Windows-only spelling (the gate matches what's being tested and doesn't hide a failure).
2. Fix: also refuse when the canonicalized `output_path` (via the runtime; the output exists whenever it collides with the input) equals `input_path`, and keep the lexical check for the case where canonicalize fails. Update the guard comment. Its "the moment the spellings agree" rationale is what failed here.
3. GREEN: the new test(s) and the existing `output_equal_to_input_refuses_and_preserves_source` pass.

**Unclassified prototype failures.** Classify `preview_static_e2e::a_page_inside_the_project_opens_on_that_page` (likely a plain/verbatim mix between the served root and the requested page) and `quarto-system-runtime cache_lru::tests::concurrent_get_and_set_lru_do_not_lose_the_set` (os error 5 on persist). For each: run it on the baseline repeatedly, then on the prototype seam, before calling anything flaky (`debugging.md`: no "flaky" verdict without a traced cause). Fix it in this layer if it is a mixing bug; file a strand if it is independent.

**Fold rule.** If the audit comes out at no more than about 10 R rows and the oracle changes stay mechanical, fold layers 2 and 3 into a single `bugfix/bd-1klbq2zd-path-audit` branch (rename not needed; `gh stack` then has two layers). Decide once the audit table exists.

### Layer 3: `bugfix/bd-1klbq2zd-dunce-seam`

- `dunce = "1"` under `[target.'cfg(not(target_arch = "wasm32"))'.dependencies]` in `quarto-system-runtime`. The shared function's body becomes `dunce::canonicalize`. UNC shares stay verbatim, per the contract exception.
- REDs before the switch, with the plain-form precondition used in `json_errors.rs` (`dunce::simplified` on the temp path): `source_file` in `--json-errors` output, the `Rendering single file:` status line, and `QUARTO_PROJECT_DIR`. The existing #743 RED (`json_errors::ipynb_parse_error_json_carries_cell_origin`) goes green.
- The acceptance probe from `2026-09-28-…-investigation/` shows `notebook_path`, `source_file` and the status line plain.

## Checklist

Layer 2 (`bugfix/bd-1klbq2zd-path-audit`):
- [x] `gh stack add bugfix/bd-1klbq2zd-path-audit`; `braid update bd-1klbq2zd --status in_progress`; CLAUDE.local.md context block by hand; commit this plan (`b83c5cf6`)
- [x] Baseline: `cargo nextest run -p quarto -p quarto-system-runtime` on unchanged code (7 failures, listed in § Baselines)
- [ ] Once the audit table exists, and before any call site changes: baseline every other crate that has R rows. Done: `quarto-core` (45 failures, § Baselines). Still to do: `quarto-hub`, `quarto-preview`, `quarto-test` (and any crate the spot-check adds), at `b83c5cf6` via `git switch --detach b83c5cf6` → run → `git switch bugfix/bd-1klbq2zd-path-audit`
- [x] Guard: RED test(s) → fix → GREEN (`37e3d127`)
- [x] Shared `canonicalize` + deepest-existing variant (std-backed, moved from `output_sink.rs`); `NativeRuntime::canonicalize` uses the former (`84f4d01a`)
- [ ] Audit table: subagent sweep DONE (§ Audit table); coverage cross-checked (every production hit has a row). Spot-check of every R/R? row against source: NOT done yet. Then route R sites, both operands for comparisons
- [ ] Spelling-agnostic test oracles
- [ ] Classify `preview_static_e2e::a_page_inside_the_project_opens_on_that_page` and `cache_lru` concurrent test
- [ ] Crate-scoped suites for every touched crate: failure set identical to the baseline except the guard tests
- [ ] Apply the fold rule
- [ ] Temporary seam prototype check: flip the function to dunce locally, confirm the audit fallout is gone, revert (not committed in this layer)

Layer 3 (`bugfix/bd-1klbq2zd-dunce-seam`):
- [ ] `gh stack add bugfix/bd-1klbq2zd-dunce-seam`; CLAUDE.local.md block
- [ ] REDs for `source_file`, status line, `QUARTO_PROJECT_DIR`
- [ ] dunce switch → GREEN, including #743's RED
- [ ] Probe re-run; output inspected and recorded here
- [ ] Crate-scoped suites vs baseline; ask about one `cargo build --workspace` before the PR

Ship:
- [ ] Ask before pushing; `gh stack submit --auto` (drafts) after review; PR text via `/open-pr`
- [ ] Mark #743 ready; merge bottom-up with merge commits on go-ahead; close bd-clq56rem, then bd-1klbq2zd
- [x] File a strand for the relative `--output` failure (bd-xdlbrc7m)

## Baselines (Windows, unchanged layer tip `b83c5cf6`)

`cargo nextest run -p quarto -p quarto-system-runtime --no-fail-fast`: 671 run, 7 failed:
- `quarto-system-runtime vfs::tests::test_vfs_clear_preserving_prefix`
- `json_errors::ipynb_parse_error_json_carries_cell_origin` (the #743 RED, on purpose)
- `preview_static_e2e::cli_flags_override_project_preview_keys`
- `preview_static_e2e::project_preview_keys_set_the_defaults_and_unsupported_keys_warn`
- `project_profile_cli::render_verbose_echoes_active_profiles`
- `render_scripts_cli::explicit_interpreter_command_line_with_args`
- `render_scripts_cli::list_form_runs_scripts_in_order`

`cargo nextest run -p quarto-core --no-fail-fast`: 5272 run, 45 failed (all pre-existing Windows): 13 `engine::content_processors::spin::tests::golden_*` (CRLF), 7 `glob::expand::tests::*`, `transforms::hephaestus::tests::artifact_path_is_content_and_size_addressed`, `pandoc_filters::format_defaults::tests::test_reference_doc_path_forwarded`, `pandoc_filters::params::tests::test_top_level_literals`, `engine::ts_protocol::tests::test_ts_wire_parity_fixture`, `engine::ts_process::tests::test_single_dial_invariant`, and integration: `pandoc_long_tail_formats::textile_fresh_baseline_snapshot`, `pandoc_goldens::{test_mermaid_fixture_preserves_diagram_source_text, test_fixtures_match_q1_golden}`, `metadata_path_resolution::frontmatter_sidebar_resolves_sibling_relative_qmd`, 3 `orange_book_lua::*`, 2 `listing_pipeline::table_*`, `pandoc_shim::{test_route_n_requires_post_init_position, test_proof_missing_type_surfaces_lua_traceback}`, `pandoc_shim_goldens::test_equation_golden`, `pandoc_render_to_file::render_document_to_file_docx_embeds_a_relatively_referenced_image`, 6 `julia_engine_e2e::j*`, `pandoc_execute_defaults::tier_d_execute_defaults_reach_engine`, `marimo_engine_e2e::sc10_widget_render_shows_header_include_and_body_island`. The run leaves two `.snap.new` files under `crates/quarto-core/tests/integration/snapshots/`; delete them after each run.

Compare sets, not counts. Raw logs are session scratch only; re-derive from this list.

## Guard finding (layer 2, step 3)

On Windows, `PathBuf::push` folds `..` away when the base is verbatim (`\\?\`), so `canonical(dir).join("sub").join("..")` is lexically the input itself and the first version of the dotdot test passed vacuously. The test now hangs the detour off `dunce::simplified(dir)` and asserts the `..` survives. Both new tests were RED for the right reason (`expected refusal … stderr: Rendering single file: …`) before the fix. The guard now also compares `runtime.canonicalize` of both operands; `determine_output_paths` takes a `&dyn SystemRuntime` (all callers, including the two book callers, pass theirs).

## Audit table (sonnet sweep at `b83c5cf6`; spot-check PENDING)

Coverage cross-checked in the main session: the production `canonicalize(` hit list (first-`#[cfg(test)]` cutoff per file) matches the rows below exactly; `capture_files.rs`, `format_paths.rs`, `quarto-hub/src/watch.rs` hits are all test-only.

**Reading rule (main session, applied on top of the agent's dispositions):** a row that is already on the runtime seam needs no change in this layer even when the agent marked it R for emission; it flips with the seam in layer 3, which is the intended behavior. Only `std`/`Path` sites marked R (or R?) are routing candidates. Also, `std` sites that share a key/comparison with a routed `std` site must be routed together (e.g. rows 15-17), or the flip splits the pair.

| # | site | crate (runtime dep) | form | disposition (agent) | main-session note |
|---|---|---|---|---|---|
| 1-2 | `quarto-core/src/output_sink.rs:260,262` + former def :420 | core (y) | deepest | L | now `quarto_system_runtime::canonicalize_deepest_existing` |
| 3 | `output_sink.rs:314` | core (y) | runtime | R (emitted in `DestOutsideAllowedRoots`) | seam: no change |
| 4 | `output_sink.rs:341` | core (y) | runtime | R (emitted `dest`) | seam: no change |
| 5 | `output_sink.rs:381` | core (y) | runtime | L | |
| 6 | `render_to_file.rs:243` | core (y) | runtime | R (error text) | seam: no change |
| 7 | `project/mod.rs:150` | core (y) | runtime | R? (FileId hash spelling must match `MetadataMergeStage`) | verify the matching-id derivation |
| 8 | `project/mod.rs:211` | core (y) | runtime | R? (same as 7) | verify |
| 9-10 | `project/mod.rs:1907, 2105` | core (y) | runtime | L (origin of `project.dir`) | |
| 11 | `project/orchestrator.rs:2369` | core (y) | runtime | L | |
| 12 | `project_resources.rs:607` | core (y) | runtime | R (error text) | seam: no change |
| 13 | `project_resources.rs:739,756` | core (y) | runtime | L | |
| 14 | `project_resources.rs:1224` `same_canonical_path` | core (y) | std | R? (fallback compares raw `dst` vs seam `source`) | route; check fallback with both operands |
| 15 | `project/book/multi_file_html.rs:173,498,504` | core (y) | std | L (self-consistent) | route together with 16/17 |
| 16 | `project/book/render_item.rs:312` `chapter_seed_map` | core (y) | std | **R**: keys looked up by `pass2_renderer.rs:315,461,583,1203` with seam-derived `doc_info.input`; would miss after the flip (book numbering) | highest-confidence; route |
| 17 | `project/book/static_analyzer.rs:107` | core (y) | std | L (mirrors 16) | route together with 16 |
| 18 | `stage/stages/include_expansion.rs:284,565` | core (y) | std | R? (`include_stack` seeded with raw `doc_path`; `profile.includes` mixes with row 19) | verify |
| 19 | `stage/stages/include_resolve.rs:593` | core (y) | runtime | R? | seam; check the shared sink with 18 |
| 20 | `quarto-hub/src/admin/collect.rs:209` | hub (**n**) | std | R (error text) | needs runtime dep |
| 21 | `collect.rs:212` | hub (n) | std | L | pair with 20 |
| 22 | `collect.rs:383` | hub (n) | std | L | |
| 23 | `collect.rs:467` | hub (n) | std | R (printed at `main.rs:327`) | |
| 24 | `quarto-hub/src/main.rs:216` | hub (n) | std | R (bail text) | |
| 25 | `quarto-hub/src/main.rs:370` | hub (n) | std | R (tracing) | |
| 26-28 | `quarto-hub/src/sync.rs:631,816,907` | hub (n) | std | L (self-consistent containment) | |
| 29-31 | `quarto-preview/src/config.rs:403,423,447` | preview (y) | std | L (self-consistent) | |
| 32 | `config.rs:536` | preview (y) | std | R? (rel paths rejoined onto a seam root?) | verify |
| 33 | `quarto-preview/src/deps.rs:102` | preview (y) | std | L | |
| 34-35 | `quarto-preview/src/lib.rs:443,444` | preview (y) | std | L (deliberate pair) | |
| 36 | `quarto/src/commands/get_config.rs:71` | quarto (y) | std | R (feeds seam-derived project machinery) | |
| 37 | `quarto/src/commands/hub.rs:80` | quarto (y) | std | R (tracing; `StorageManager`) | |
| 38 | `quarto/src/commands/preview.rs:137` | quarto (y) | std | R (log/error display, per its comment) | |
| 39 | `quarto/src/commands/preview_static.rs:297` | quarto (y) | std | R? | likely tied to the `preview_static_e2e` prototype failure |
| 40-41 | `quarto/src/commands/render.rs:278, 421` | quarto (y) | runtime | R (error text) | seam: no change |
| 42 | `quarto-source-fetch/src/archive.rs:105` | source-fetch (n) | std | L | |
| 43 | `quarto-trace-server/src/lib.rs:252` | trace-server (n) | std | L (both operands same call) | |
| 44 | `quarto-system-runtime/src/native.rs:94` | runtime | impl | L (the seam) | now calls the shared fn |
| 45 | `quarto-test/src/runner.rs:67` | quarto-test (y) | std | R? (feeds `render_document`) | verify |
| 46 | `wasm-quarto-hub-client/src/lib.rs:1740` | wasm | runtime | N/A (wasm VFS only) | |
| 47-49 | `traits.rs:333`, `sandbox.rs:205`, `wasm.rs:349` | runtime | decl/impls | N/A | |

Candidate routing set before spot-check: std rows 14, 15, 16, 17, 18, 20(+21), 23, 24, 25, 32?, 36, 37, 38, 39, 45?. That is ~12-15 sites, above the fold threshold of about 10 R rows, so the fold rule leans towards keeping layer 3 separate; decide after the spot-check.

## Verification

Crate-scoped only on this machine: CLAUDE.local.md overrides the AGENTS.md pre-push gates (workspace build and test runs, `cargo xtask verify`) here, because those runs are very slow on this Windows host. CI runs the full gates on Linux/macOS. One `cargo build --workspace` is still offered before the PR, since CI has no Windows leg. Every run is compared against the layer-2 baseline, not judged on its own:

```bash
cargo nextest run -p quarto -p quarto-system-runtime          # baseline + each layer
cargo nextest run -p quarto-core -p quarto-preview            # when audit routes sites there
cargo nextest run -p quarto -E 'test(json_errors::)'          # layer 3: the #743 RED goes green
```

Linux/macOS CI covers the rest, including the portable guard test.

## Risks

- A site the audit marks L may still meet a seam path through a consumer the sweep missed. The prototype flip at the end of layer 2 is the backstop: it reruns the same suites that showed the 20 fallout failures.
- `dunce::canonicalize` differs from std only on Windows; on Unix the layer-3 switch is a no-op, so Linux/macOS CI cannot catch a Windows regression in layer 3. That is why the Windows crate runs and the probe are mandatory before the PR.
- The shared-function naming and location are a small API decision in a foundation crate; flag it in the PR body.
