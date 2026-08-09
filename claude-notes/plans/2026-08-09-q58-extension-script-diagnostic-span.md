# Q-5-8 diagnostic points at wrong `_quarto.yml` span for extension-contributed pre-render scripts

**Strand:** bd-m6wmztln (p1 bug)
**Discovered-from strands:** bd-p86nlm92 (project_resources, same pattern — folded into this PR),
bd-2x0tmd7v (doc-level span-less gap, unverified), bd-xh1v98d9 (store manifest
path on `Extension`), bd-nv4p0eb1 (systematic span/FileId audit + API hardening)
**Status:** approved 2026-08-09 — in progress on branch
`braid/bd-m6wmztln-q58-extension-script-span`.

## Review decisions (2026-08-09)

1. **Scope**: fold the bd-p86nlm92 project_resources fix into this PR.
2. **Wording**: use the diagnostic builder's `add_info` to name the
   contributing extension manifest when the matched source file is not
   the project config file.
3. **Manifest path**: reconstruct `ext.path.join("_extension.yml")` now
   with a comment noting the coupling; bd-xh1v98d9 tracks storing the
   actual path on `Extension`.
4. New follow-up bd-nv4p0eb1: audit this bug class tree-wide and
   consider an API change making wrong-file binding unrepresentable.

## Work items

### Phase 1 — tests first
- [x] Integration test: extension-contributed failing pre-render script →
      Q-5-8 snippet names `_extension.yml`, not `_quarto.yml`
      (`render_scripts_cli.rs::failing_extension_contributed_script_snippet_names_extension_yml`)
- [x] Strengthen `failing_pre_render_script_aborts` to assert the snippet
      names `_quarto.yml` (passes already — control behavior correct)
- [x] Integration test for extension-contributed `project.resources`
      diagnostic (bd-p86nlm92 leg) — new module
      `extension_config_spans.rs`; red run shows the dropped-snippet
      variant live (Q-5-1 rendered with no span at all)
- [x] Unit tests for the candidate-matching helper (land with the helper —
      5 tests in `config_sources.rs`)
- [x] Run new tests, verify they fail on current code (2 failed as
      expected, control passed — 2026-08-09)

### Phase 2 — implementation
- [x] `ProjectConfig.extension_manifest_paths` populated in `parse_config`
- [x] Shared candidate-matching helper (register-on-match, returns matched
      path): `crates/quarto-core/src/config_sources.rs::bind_config_source`
- [x] Thread candidates through `RenderScriptsContext` — turned out to be
      **5** construction sites, not 4: `commands/render.rs` ×2,
      `commands/publish.rs` ×2, plus `quarto-preview/src/lib.rs` (boot-time
      pre-render run)
- [x] `script_error` uses helper + `add_info` extension attribution
- [x] Resources leg (bd-p86nlm92): refactored the diagnostic
      body into `resource_error_diagnostic`; new
      `resource_error_to_config_parse_error(err, &ProjectConfig)` used by
      the project-level call site; doc-level
      `resource_error_to_parse_error` keeps its contract, now documented
      as doc-provenance-only (layered-metadata caveat noted → bd-nv4p0eb1)
- [x] All new + existing tests pass (workspace: 11127 passed, 0 failed,
      2026-08-09)

### Phase 3 — verification
- [x] `cargo build --workspace` + `cargo nextest run --workspace`
      (11127 passed; `cargo xtask lint` clean)
- [x] `cargo xtask verify` (quarto-core changed → hub leg included) — all
      steps passed 2026-08-09
- [x] E2E: repro fixture renders corrected span; q2-connect-docs testbed
      renders corrected span; outputs recorded below
- [ ] Close bd-m6wmztln + bd-p86nlm92 with evidence (after PR review/merge)

## Overview

When a `project.pre-render` / `project.post-render` script fails, the Q-5-8
diagnostic attaches an ariadne snippet anchored at the YAML scalar that
declared the script. For scripts declared in the user's `_quarto.yml` this
works. For scripts contributed by an extension via
`contributes.metadata.project.pre-render` (the Q1 mechanism quarto-openapi
uses, supported in q2 since bd-ad7i1pc6 Phase 5 / bd-zb2tod5f), the snippet
renders `_quarto.yml` content at the **extension file's** byte offsets —
a confidently wrong span.

Observed live in the q2-connect-docs testbed (`posit-dev/quarto-openapi`
extension): the failing script `openapi-to-markdown.ts` is declared at
`_extensions/posit-dev/quarto-openapi/_extension.yml:8-9`, but the Q-5-8
snippet pointed at `_quarto.yml:10-13` (`post-render:` through a `render:`
glob) — a span the user never associated with the failing script.

## Root cause

`script_error` in `crates/quarto-core/src/project/render_scripts.rs:540-563`:

```rust
if let (Some((fid_usize, _, _)), Some(config_path)) =
    (info.resolve_byte_range(), ctx.config_path)
{
    let content = std::fs::read_to_string(config_path).ok();
    source_context.add_file_with_id(
        FileId(fid_usize),
        config_path.to_string_lossy().into_owned(),
        content,
    );
}
```

It takes whatever `FileId` the script's `SourceInfo` resolves to and
unconditionally binds **`_quarto.yml`'s path and content** to it.

The FileId scheme (quarto-yaml `parse_file` /
`file_id_for_filename`) hashes the filename string passed at parse
time. The chain for an extension-contributed script:

1. `read_extension_with_org` (`crates/quarto-core/src/extension/read.rs:62-63`)
   parses `_extension.yml` with `extension_file.display().to_string()` as the
   filename → every scalar's SourceInfo carries
   `FileId(hash("<abs path>/_extension.yml"))`.
2. `apply_metadata_project_contributions`
   (`crates/quarto-core/src/project/mod.rs:624`) merges the
   `contributes.metadata.project` fragment under the user's config.
   `rebase_fragment_paths` may rewrite the script *string* (ext-dir →
   project-root) but only mutates `value.value`; `source_info` is
   preserved — so the merged scalar still points into `_extension.yml`.
3. `extract_render_scripts` copies that SourceInfo into `RenderScript`.
4. On failure, `script_error` registers `_quarto.yml` under the
   extension file's FileId. Ariadne then resolves the extension-file
   byte offsets against `_quarto.yml` content.

Two observable symptoms, depending on file sizes:

- **Misleading span** — the offsets fit inside `_quarto.yml`: an
  arbitrary unrelated span is highlighted (the q2-connect-docs case).
- **Dropped span** — the offsets exceed `_quarto.yml`'s length: the
  snippet silently disappears and the error renders with no location
  at all.

Note this is **not** a quarto-yaml bug: the hash-based FileId scheme is
designed for exactly this rebinding, and quarto-yaml exposes
`file_id_for_filename` so consumers can bind the *right* file. The bug
is q2's assumption that the only possible source file is `_quarto.yml`.

## Reproduction (self-contained)

Fixture (three files):

```
repro/
  _quarto.yml
  index.qmd
  _extensions/acme/failing/_extension.yml
```

`_quarto.yml` (padding comments make the wrong offsets land *inside*
the file, producing the misleading-span variant; shrink the file to see
the dropped-span variant instead):

```yaml
# This _quarto.yml deliberately has enough content that the
# extension-file byte offsets land inside it, producing a
# misleading-but-renderable span (the q2-connect-docs symptom).
project:
  type: default
  post-render:
    - "./post-render.sh"
  render:
    - "**/*.qmd"
    - "!drafts/"

format:
  html:
    toc: true
```

`_extensions/acme/failing/_extension.yml`:

```yaml
title: failing
author: Acme
version: 0.0.1
contributes:
  metadata:
    project:
      pre-render:
        - "false"
```

`index.qmd`: any trivial document.

Observed (2026-08-09, `cargo run --bin q2 -- render repro`):

```
Running pre-render script: false
Error: [Q-5-8] Pre-render script failed
   ╭─[ …/repro/_quarto.yml:2:50 ]
 2 │ # extension-file byte offsets land inside it, producing a
   │                                                  ───┬───
   │                                                     ╰───── Script `false` exited with status 1. …
```

The span points at column 50 of a comment — those are the byte offsets
of the `"false"` scalar in `_extension.yml`.

Control (same failing script declared directly under
`project.pre-render` in `_quarto.yml`): the snippet correctly points at
the `- "false"` entry (`_quarto.yml:4:7`). Output inspected end-to-end
through the real binary in both cases.

## Blast-radius audit (all `add_file_with_id` diagnostic sites)

| Site | Verdict |
| --- | --- |
| `render_scripts.rs:540` `script_error` | **Buggy** — this strand (bd-m6wmztln). |
| `project_resources.rs:864` `resource_error_to_parse_error` | **Buggy, same pattern** — `project.resources` is extension-contributable (`FRAGMENT_PATH_PATTERNS`). Filed as bd-p86nlm92. |
| `theme_diagnostic.rs:51` `sass_error_to_parse_error` | **Correct** — takes `candidate_sources: &[(FileId, &Path)]`, registers only on FileId match. This is the precedent pattern for the fix. |
| `quarto/src/commands/render.rs:1073/1086` `config_source_context` / `attach_config_source` | **Correct** — verifies `file_id_for_filename(config_path) == fid` before binding. |
| `project/mod.rs:947` `project_type_error` | **Safe** — `type:` is read from the user's config before fragment merging, so the SourceInfo is always `_quarto.yml`'s. |
| `metadata_merge.rs:298` register block | **Gap, not misleading** — extension manifests are never registered in doc SourceContexts, so extension-anchored per-document diagnostics render span-less. Filed as bd-2x0tmd7v (unverified, p3). |

## Fix plan

Follow the `sass_error_to_parse_error` precedent: candidate-list
matching instead of unconditional binding.

### Phase 1 — tests first (TDD)

1. **Integration test** (`crates/quarto/tests/integration/render_scripts_cli.rs`,
   following its existing fixture/skip conventions):
   `failing_extension_contributed_script_points_at_extension_yml` —
   fixture project as in the repro above, but with a Python
   `sys.exit(1)` script (cross-platform; skip when no Python, like the
   existing tests). Assert stderr contains `[Q-5-8]`, contains
   `_extension.yml` in the snippet header, and does **not** render a
   `_quarto.yml`-anchored snippet.
2. **Strengthen the existing control**: extend
   `failing_pre_render_script_aborts` to assert its snippet names
   `_quarto.yml` (guards against regressing the direct-declaration
   case).
3. **Unit test** for the new candidate-matching helper (see below):
   given a SourceInfo whose fid hashes to the extension manifest path,
   the helper picks the manifest; given the config file's fid, it picks
   the config; given an unknown fid, it returns nothing (span-less
   degradation, never a wrong binding).
4. Run the new integration test, verify it **fails** on current code
   (wrong/absent file in snippet) before implementing.

### Phase 2 — implementation

1. **`ProjectConfig` records its source files.** New field, e.g.
   `extension_manifest_paths: Vec<PathBuf>`, populated in
   `parse_config` (`project/mod.rs:1329`) from the already-discovered
   `extensions` (`ext.path.join("_extension.yml")` — discovery only
   accepts that exact name, and `ext.path` is literally the manifest's
   parent, so the reconstructed string equals the one hashed at parse
   time). Together with the existing `config_path` this is the full
   candidate set for project-scope config diagnostics.
2. **Shared helper** in quarto-core (natural home: next to
   `script_error`, or promoted to a small shared module if
   bd-p86nlm92 is folded in): given a `SourceInfo` and candidate paths,
   resolve the fid, find the candidate whose
   `quarto_yaml::file_id_for_filename` matches, read its content, and
   register it. No match ⇒ register nothing (keep `with_location`; the
   renderer degrades to span-less, matching theme_diagnostic's
   documented contract).
3. **Thread candidates through `RenderScriptsContext`**: add
   `extension_manifest_paths: &[PathBuf]` (or replace `config_path`
   with a general `source_candidates` slice — decide at implementation
   based on how noisy the four construction sites get). Update the four
   construction sites (`commands/render.rs` ×2, `commands/publish.rs`
   ×2); the compiler finds any others (preview boots pre-render scripts
   through these same paths).
4. `script_error` uses the helper for all three call sites (Q-5-8,
   Q-5-10 empty-entry, Q-5-10 launch-failure).

### Phase 3 — verification

- `cargo build --workspace`, `cargo nextest run --workspace`.
- `cargo xtask verify --skip-hub-build` minimum; full `verify` if
  anything WASM-visible in quarto-core's API changed (the new
  `ProjectConfig` field is plain data; hub-build leg likely needed
  since quarto-core changed).
- End-to-end: re-run the repro fixture and the q2-connect-docs testbed
  render; paste the corrected diagnostic (snippet anchored at
  `_extensions/posit-dev/quarto-openapi/_extension.yml:9`) into the
  strand before closing.

## End-to-end verification (2026-08-09, output inspected)

Invocation: `./target/debug/q2 render <fixture>` (freshly built binary).

**Repro fixture** (extension-contributed `"false"`): the Q-5-8 snippet
now anchors at `_extensions/acme/failing/_extension.yml:8:11`,
highlighting the `- "false"` entry, followed by:

```
ℹ This entry is contributed by the extension manifest
  `…/_extensions/acme/failing/_extension.yml` (`contributes.metadata.project`),
  not by your project configuration file.
```

**Control fixture** (script declared in `_quarto.yml`): unchanged —
snippet anchors at `_quarto.yml:4:7` on the `- "false"` entry, no
attribution line.

**q2-connect-docs testbed** (the originally reported case): snippet
anchors at
`…/docs-quarto-2/_extensions/posit-dev/quarto-openapi/_extension.yml:9:11`,
highlighting `- _extensions/posit-dev/quarto-openapi/openapi-to-markdown.ts`,
with the attribution info line. Previously it pointed at
`_quarto.yml:10-13` (`post-render:` block).

## Open questions for review

1. **Scope**: fix only `render_scripts.rs` here and leave bd-p86nlm92
   (project_resources) as a follow-up, or fold it into the same PR
   since it reuses the same helper + `ProjectConfig` field? My
   recommendation: same PR — the helper lands once, the second
   consumer is ~10 lines and shares the test fixture shape.
2. **Diagnostic wording**: when the script comes from an extension, the
   corrected snippet already names `_extension.yml` in the ariadne
   header. Should the problem text *additionally* say "contributed by
   extension `acme/failing`" for clarity (helps when the snippet
   degrades span-less)? Cheap to add from the matched candidate path;
   default: yes for the no-match degradation path only.
3. **`_extension.yml` vs `.yaml`**: discovery currently only reads
   `_extension.yml` (`extension/discover.rs:154,177`), so candidate
   reconstruction is exact today. If `.yaml` support is ever added,
   storing the *actual* manifest path on `Extension` (instead of
   reconstructing via join) would be more robust — worth doing now
   (one field on `Extension`), or defer? Default: reconstruct now,
   note the coupling in a comment.
