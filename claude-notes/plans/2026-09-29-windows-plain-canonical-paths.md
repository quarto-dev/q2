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

**One production canonicalize function.** Add a free function in `quarto-system-runtime` (all targets, see § Target gating below; name to settle during review, e.g. `quarto_system_runtime::canonicalize`) that wraps `std::fs::canonicalize` **unchanged** in this layer. `NativeRuntime::canonicalize` calls it. Audited direct `std` call sites that need a consistent spelling call the **free function**, never the runtime, even when a runtime is in scope. The runtime is not equivalent: `WasmRuntime::canonicalize` normalizes against its VFS (`wasm.rs:349`), and the test-mock runtimes return whatever they are told, so rerouting a `std` call through it would change behavior in this layer. Existing runtime calls stay as they are. Deliberately moving a site onto the runtime is out of scope unless the audit records it as its own behavior change with its own acceptance test.

**Target gating.** `84f4d01a` exports both functions for native targets only (`lib.rs` gates `mod canonical`), but some routing candidates compile for wasm32 too: `project/book/render_item.rs`, `project/book/static_analyzer.rs` and `stage/stages/include_expansion.rs` are ungated modules (rows 16-18). Routing them to a native-only function would break the WASM build. Decision: export both functions on every target. On wasm32 the body stays `std::fs::canonicalize`, which is exactly what those sites call today, so wasm behavior does not change and no call site needs its own `cfg`. Layer 3's dunce switch is `cfg`-split inside the function body (dunce is a native-only dep). The audit table records the target of every candidate.

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

- `dunce = "1"` under `[target.'cfg(not(target_arch = "wasm32"))'.dependencies]` in `quarto-system-runtime`. The shared function's native body becomes `dunce::canonicalize`; the wasm32 body stays `std::fs::canonicalize`. UNC shares stay verbatim, per the contract exception.
- REDs before the switch, with the plain-form precondition used in `json_errors.rs` (`dunce::simplified` on the temp path): `source_file` in `--json-errors` output, the `Rendering single file:` status line, and `QUARTO_PROJECT_DIR`. The existing #743 RED (`json_errors::ipynb_parse_error_json_carries_cell_origin`) goes green.
- The acceptance probe from `2026-09-28-…-investigation/` shows `notebook_path`, `source_file` and the status line plain.

## Checklist

Layer 2 (`bugfix/bd-1klbq2zd-path-audit`):
- [x] `gh stack add bugfix/bd-1klbq2zd-path-audit`; `braid update bd-1klbq2zd --status in_progress`; CLAUDE.local.md context block by hand; commit this plan (`b83c5cf6`)
- [x] Baseline: `cargo nextest run -p quarto -p quarto-system-runtime` on unchanged code (7 failures, listed in § Baselines)
- [x] Once the audit table exists, and before any call site changes: baseline every other crate that has R rows: `quarto-core` (45 failures), `quarto-hub` + `quarto-preview` + `quarto-test` (3 failures), all at `b83c5cf6` (§ Baselines)
- [x] Guard: RED test(s) → fix → GREEN (`37e3d127`)
- [x] Shared `canonicalize` + deepest-existing variant (std-backed, moved from `output_sink.rs`); `NativeRuntime::canonicalize` uses the former (`84f4d01a`)
- [x] Audit table: subagent sweep, coverage cross-check, spot-check (§ Spot-check verdicts), routing (`28bea643`). `ts_process` failures classified (§ Baselines, bd-j5ij00i0). wasm32: `cargo check -p quarto-system-runtime --target wasm32-unknown-unknown` passes; the `quarto-core` wasm32 check cannot run on this host (no clang for tree-sitter's C build), so CI's hub-client build leg is the check for rows 16-18
- [x] Spelling-agnostic test oracles: `commands::render` and `render_scripts_cli` `canonical` helpers call the shared function; `QUARTO_PROJECT_DIR`/`OUTPUT_DIR` assert identity (absolute + equal after canonicalizing both sides). Unflipped: only the 2 baseline `render_scripts_cli` failures. `preview.rs` helpers wait on the flip
- [x] Classify `preview_static_e2e::a_page_inside_the_project_opens_on_that_page`: passes under the flip, nothing to fix. `cache_lru` concurrent test: classified, independent of this work (bd-cpzr71jr). `preview.rs` helpers: no `commands::preview` test fails under the flip, no change
- [ ] Crate-scoped suites for every touched crate: failure set identical to the baseline except the guard tests
- [x] Apply the fold rule: do **not** fold. R rows exceed 10 and the flip fallout is not small (§ Flip results)
- [x] Temporary seam prototype check (`932313dc`, flip reverted, `Cargo.lock` == HEAD): audit fallout is **not** gone, ~129 new failures (§ Flip results)
- [x] Classify every § Flip results cluster as test oracle vs product mixing (§ Flip classification)
- [x] Route `quarto-preview/src/config.rs:403` (+ `:423/:447/:536`) through the shared function; flip-RED = `config::tests::single_file_deps_resources_glob` (RED from the `932313dc` flip run; GREEN pending the flip re-run). Audit rows 29-32 corrected to R
- [x] Fix every product site the classification finds: none beyond config.rs
- [x] Oracle sweep (`111e2cf7`): the failing files' std helpers and inline std canonicalizes now call the shared function
- [ ] Re-run the flip on all six crates: new failures only from known noise (bd-j5ij00i0, bd-cpzr71jr), then revert

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

`cargo nextest run -p quarto-hub -p quarto-preview -p quarto-test --no-fail-fast`: 728 run, 3 failed (1 skipped): `quarto-hub storage::tests::test_storage_manager_prevents_double_lock`, `quarto-hub storage::tests::test_storage_manager_standalone_prevents_double_lock`, `quarto-preview render_scripts_boot::pre_render_scripts_run_once_at_boot`.

**After routing (`28bea643`, unflipped)**, all six crates in one run (`-p quarto -p quarto-system-runtime -p quarto-core -p quarto-hub -p quarto-preview -p quarto-test`): 6676 run, 53 failed. Matches the baseline sets except for:
- newly failing, both in `quarto-core engine::ts_process` (code this layer does not touch; both reported `FAIL + LEAK`): `test_malformed_frame_is_fatal`, `test_spawn_into_tcp_child_dies_after_echoing_token`. Run on their own at `28bea643`, both pass (and `test_single_dial_invariant` fails, as in the baseline). So they only fail under the full parallel run. Classified as load-sensitive tests, independent of this layer (no `engine/` change since `b83c5cf6`; neighbours ran 5-8x slower in the 6-crate run). The spawn test keeps a 2 s watchdog around a `powershell` 5.1 cold start; with 44 CPU-burner jobs it failed 1 of 5 reps on HEAD with the same `DEADLOCK DETECTED`. The malformed-frame test syncs with `sleep(50ms)` and `request()` never checks `shutting_down`, so a late-registered request waits out its 10 s window against a 10 s watchdog (traced from source; not reproduced by CPU load alone). Filed as bd-j5ij00i0.

`cache_lru::tests::concurrent_get_and_set_lru_do_not_lose_the_set`: 1 of 40 reps fails on HEAD, unflipped (`failed to persist cache entry sass/_lru_index … os error 5`). All in-process index access is under `INDEX_LOCK`. A single-threaded replace-rename loop in `%TEMP%` with no concurrency also fails (1 of 3000), so something outside the process holds the just-rewritten file. Independent of this work; production ignores the error. Filed as bd-cpzr71jr.
- now passing (were baseline failures): `pandoc_long_tail_formats::textile_fresh_baseline_snapshot`, `pandoc_goldens::test_mermaid_fixture_preserves_diagram_source_text`, `pandoc_execute_defaults::tier_d_execute_defaults_reach_engine`, `marimo_engine_e2e::sc10_widget_render_shows_header_include_and_body_island`.
- 12 `spin::tests::golden_*` failed, not 13; the baseline entry gives only a count, so the difference can't be pinned to a named test.

Temporary dunce flip, before and after routing: the new `render_item::tests::seed_map_keys_match_the_discovered_document_inputs` and `include_expansion::tests::self_include_is_caught_at_the_first_level_on_disk` both fail without the routing (the discovered input is plain, the keys are verbatim; the document gets spliced into itself once) and pass with it. The existing `seed_map_covers_numbered_files_and_skips_unnumbered` also failed under the flip; its oracle now uses the shared function.

Compare sets, not counts. Raw logs are session scratch only; re-derive from this list.

## Flip results (temporary dunce flip at `932313dc`, six crates)

`cargo nextest run -p quarto -p quarto-system-runtime -p quarto-core -p quarto-hub -p quarto-preview -p quarto-test --no-fail-fast --build-jobs 6 --test-threads 8`: 6676 run, 171 failed (unflipped: 53). The capped parallelism kept free RAM above 8 GB. The #743 RED and `preview_static_e2e::a_page_inside_the_project_opens_on_that_page` pass. The new failures (~129) are all plain-vs-verbatim mixing: one operand std-canonicalized (verbatim), the other from the shared function (plain).

- `quarto` `commands::render::render_once_tests::{single_document_outside_a_project_renders_beside_the_source, project_render_reports_output_dir_and_input_output_pairs}`: expectation verbatim, product plain (`render.rs:3351`). Step 6 missed these two.
- `quarto-core` unit: `project_resources::tests` (~22; `OutOfProject` with a verbatim `project_root`, e.g. `expand_literal_path` at `project_resources.rs:1405`), `project::tests::directory_metadata_tests` (12), `project::tests::project_brand` (2).
- `quarto-core` integration (~88): `idempotence` 26 ("active file present in discovered project", `idempotence.rs:191`), `render_page_in_project` 14, `repo_actions_pipeline` 13 ("no output for href 'index.html'"), `book_preview` 7, `secondary_nav_pipeline` 7, `breadcrumbs_pipeline` 5, `headroom_pipeline` 3, `project_pipeline` 3, `project_profile_overlays` 3, and one each in `book_project_type`, `execution_policy`, `format_css`, `incremental_rebuild`, `project_resources::orchestrator_engine_channel`, `render_to_html_captures`, `render_to_html_user_grammars`. The first three files have std `canonical()` helpers (`idempotence.rs:67`, `repo_actions_pipeline.rs:27`, `render_page_in_project.rs:50`), so these clusters are probably test oracles. That is not verified per cluster yet.
- `quarto-hub::integration admin_collect_lifecycle::collect_lifecycle_quarantine_restore_purge`: the test std-canonicalizes `hub_dir` (`:106`, `:135`).
- `quarto-preview config::tests::{single_file_deps_includes_declared_resources, single_file_deps_resources_glob}`: **product bug**. `config.rs:403` std-canonicalizes `canonical_root` and passes it to `quarto_core::project_resources::expand_patterns` (`:480`), which canonicalizes the matches through the runtime. The containment check yields `OutOfProject`, the `if let Ok` swallows it, and declared `resources:` drop out of the single-file preview closure. Audit rows 29-32 (L, self-consistent) were wrong: `expand_patterns` is a seam consumer.

Baseline tests that pass under the flip (run-to-run variation, not attributed): `metadata_path_resolution::frontmatter_sidebar_resolves_sibling_relative_qmd`, `pandoc_render_to_file::render_document_to_file_docx_embeds_a_relatively_referenced_image`, the six `julia_engine_e2e::j*`. The run left `crossrefs_all_docx__docx.snap.new` and `integration__pandoc_shim_goldens__equation_golden.snap.new` (deleted).

### Flip classification (sonnet sweep, static; central claims checked in the main session)

Every cluster except config.rs is a **test oracle**: the verbatim operand is a test-side std canonicalize, and every production caller spells the same argument through the runtime or the shared function (`discover` at `project/mod.rs:1907`; `RenderMode::Subset` from `project.files[].input` at `render.rs:1236`; `ExecutionPolicy::Only` from the shared fn at `preview_static.rs:296`, per its contract at `execution_policy.rs:30-32`).

| cluster | verbatim operand (test) | meets the runtime spelling at |
|---|---|---|
| `render_once_tests` (2) | `render.rs:3295` (`website()`), `:3345` | `assert_eq` on `report.output_dir/project_dir/outputs()` |
| `project_resources::tests` (~22) | `temp.path().canonicalize()` ×29 plus 4 expected values | `project_resources.rs:609` containment → `OutOfProject` |
| `directory_metadata_tests` (12) | `test_project_context`, `project/mod.rs:3153` | `mod.rs:217` `strip_prefix` |
| `project_brand` (2) | `mod.rs:4026, 4049` | `assert_eq` on `resolved.dir` |
| quarto-core integration (16 files) | each file's local `canonical()` helper, or inline in `execution_policy.rs` / `project_profile_overlays.rs` | `ActivePage`/`Subset`/`Only` membership against `project.files`, or `output_path.strip_prefix(site_root)` |
| `admin_collect_lifecycle` | `:135` (`:106` is harmless: re-canonicalized by `collect.rs:210`) | `batch_dir.starts_with` |

The only **product** site is `quarto-preview/src/config.rs:403` (see rows 29-32). The sweep found no other production std canonicalize that meets a seam path (the others sit in `cfg(test)`, or both of their operands come from std: `deps.rs:102`, `lib.rs:443-444`, `sync.rs`, `same_canonical_path`, trace-server `is_within`). Other integration files with the same std `canonical()` helper (`book_*`, `brand_fonts`, `fail_fast`, …) passed under the flip and are left unchanged.

## Guard finding (layer 2, step 3)

On Windows, `PathBuf::push` folds `..` away when the base is verbatim (`\\?\`), so `canonical(dir).join("sub").join("..")` is lexically the input itself and the first version of the dotdot test passed vacuously. The test now hangs the detour off `dunce::simplified(dir)` and asserts the `..` survives. Both new tests were RED for the right reason (`expected refusal … stderr: Rendering single file: …`) before the fix. The guard now also compares `runtime.canonicalize` of both operands; `determine_output_paths` takes a `&dyn SystemRuntime` (all callers, including the two book callers, pass theirs).

## Audit table (sonnet sweep at `b83c5cf6`; spot-checked, see § Spot-check verdicts)

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
| 29-31 | `quarto-preview/src/config.rs:403,423,447` | preview (y) | std | L (self-consistent) | **R** (flip): `:403`'s root meets runtime-canonical matches in `expand_patterns`; `:423/:447` compare against that root, so all move together |
| 32 | `config.rs:536` | preview (y) | std | R? (rel paths rejoined onto a seam root?) | **R** with 29-31 (same `to_in_tree_rel` root) |
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

Target of each quarto-core candidate (checked against `cfg` gates at `16cd2d5b`): row 14 native-only (`project_resources.rs:1222` `#[cfg(not(target_arch = "wasm32"))]`); row 15 native-only (`book/mod.rs:16` gates `multi_file_html`); rows 16, 17, 18 **compile for wasm32** (ungated modules `render_item`, `static_analyzer`, `include_expansion`). Rows in `quarto`, `quarto-hub`, `quarto-preview`, `quarto-test` are native binaries/libs. The wasm32 rows need the all-target export (§ Target gating).

Candidate routing set before spot-check: std rows 14, 15, 16, 17, 18, 20(+21), 23, 24, 25, 32?, 36, 37, 38, 39, 45?. That is ~12-15 sites, above the fold threshold of about 10 R rows, so the fold rule leans towards keeping layer 3 separate; decide after the spot-check.

### Spot-check verdicts (main session, source at `b83c5cf6`)

| row | verdict | evidence |
|---|---|---|
| 7-8 | seam, no change | `project/mod.rs:150` and `:211` both call `runtime.canonicalize`; the FileId spelling and the layer-id re-derivation share that one function, so they flip together |
| 14 | **L** | `same_canonical_path` canonicalizes both operands with the same call; the lexical fallback only runs when one side is missing, and a missing `dst` can never be the existing `source` |
| 15 | **R** (route with 16) | `multi_file_html.rs:173` looks up `chapter_seed_map` with its own std key; `:498/:504` build and probe `item_inputs` from `project.dir` joins vs `project.files[].input`, both std-canonicalized |
| 16 | **R** (confirmed) | keys: `render_item.rs:312`, std canonicalize of `project.dir.join(file)`. Lookups: `pass2_renderer.rs:315,461,583,1203` use `doc_info.input` straight from `ProjectContext::discover` (runtime canonical at `mod.rs:1906`, then the walk / `from_path` at `:1976`). After the flip every lookup misses and book chapters lose their numbering seed |
| 17 | **R** (route with 16) | `static_analyzer.rs:107` mirrors 16's key derivation, per its comment |
| 18 | **R** (confirmed) | `include_expansion.rs:128-130` seeds `include_stack` with `doc.path` (seam spelling); `:284/:565` insert and probe std-canonical paths. After the flip a self-include is caught one level late (its content is spliced once). The same paths feed `recorded_includes` next to row 19's runtime-canonical entries |
| 19 | seam, no change | runtime call; routing row 18 makes both include sinks agree |
| 20-23 | **R** (emission) | admin `data_dir` canonicalizations: error text and the `purge`/`restore` paths. The `collect` manifest check (`collect.rs:209/212`) canonicalizes both sides itself, so manifests scanned by an older binary still match |
| 24 | **R** (emission) | `main.rs:216` admin scan: bail text and the canonical `data_dir` it records |
| 25 | **R** (emission) | `main.rs:370` project root: tracing and the `StorageManager` root |
| 26-28 | L (re-confirmed) | `sync.rs:631/816/907`: both containment operands are std-canonicalized in place, independent of how `project_root` is spelled. Security-sensitive; left untouched on purpose |
| 32 | ~~L~~ **R** (corrected by the flip) | `quarto-preview/config.rs:403-536`: the in-place candidates are self-consistent, but `canonical_root` also goes into `expand_patterns` (`:480`), whose containment check (`project_resources.rs:606-609`) uses runtime-canonical matches. Under the flip every declared `resources:` entry is `OutOfProject` and the `if let Ok` drops it. Rows 29-32 routed together |
| 36 | **R** | `get_config.rs:71`: the std `input` goes into `DocumentInfo::from_path` next to the seam-derived `project.dir` from `discover_with_profile` |
| 37 | **R** (emission) | `commands/hub.rs:80`: tracing and the `StorageManager` root, as row 25 |
| 38 | **R** | `commands/preview.rs:137`: the std root feeds `resolve_project_and_initial_page` and the served project |
| 39 | **R** | `preview_static.rs:297`: the std `path` is both the render input (re-canonicalized by the seam) and the base for `find_project_root_upward` and the initial page; suspect for `preview_static_e2e::a_page_inside_the_project_opens_on_that_page` |
| 45 | **R** (emission) | `quarto-test/runner.rs:67`: the std `input_path` sits next to the seam-derived `output_path` in failure reports |

Routing set: rows 15 (3 calls), 16, 17, 18 (2 calls), 20-25, 36-39, 45. Left as they are: 14 and 32 (downgraded from R? by the spot-check), 26-28. No crate beyond the baselined ones is added.

## Verification

Crate-scoped only on this machine: CLAUDE.local.md overrides the AGENTS.md pre-push gates (workspace build and test runs, `cargo xtask verify`) here, because those runs are very slow on this Windows host. CI runs the full gates on Linux/macOS. One `cargo build --workspace` is still offered before the PR, since CI has no Windows leg. Every run is compared against the layer-2 baseline, not judged on its own:

```bash
cargo nextest run -p quarto -p quarto-system-runtime          # baseline + each layer
cargo nextest run -p quarto-core -p quarto-preview            # when audit routes sites there
cargo nextest run -p quarto -E 'test(json_errors::)'          # layer 3: the #743 RED goes green
cargo check -p quarto-core --target wasm32-unknown-unknown    # after routing wasm32-compiled sites, and after the layer-3 body split
```

The wasm check is crate-scoped (target installed here); CI's hub-client build leg is the full WASM backstop.

Linux/macOS CI covers the rest, including the portable guard test.

## Risks

- A site the audit marks L may still meet a seam path through a consumer the sweep missed. The prototype flip at the end of layer 2 is the backstop: it reruns the same suites that showed the 20 fallout failures.
- `dunce::canonicalize` differs from std only on Windows; on Unix the layer-3 switch is a no-op, so Linux/macOS CI cannot catch a Windows regression in layer 3. That is why the Windows crate runs and the probe are mandatory before the PR.
- The shared-function naming and location are a small API decision in a foundation crate; flag it in the PR body.
