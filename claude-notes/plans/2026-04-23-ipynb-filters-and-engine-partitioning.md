# Research Plan: ipynb-filters in q2

**Status:** Research — future work, not part of the TS engine extensions project
**Depends on:** TS engine extensions (Plans 1a/1b/1c), native Jupyter engine, and the website-project pipeline (DocumentProfile checkpoint, two-pass orchestrator) already on `main`.
**Supersedes:** an earlier draft of this plan (in git history) that assumed Plans 1a/1b would ship a `partitioned_markdown` trait method and `PartitionedMarkdown` protocol message. That assumption was reversed during plan review on 2026-04-28; see "Why we changed approach" below. The earlier specifics are preserved in the file's git history if needed.

## What this plan covers

How q2 should implement Quarto 1's `ipynb-filters` feature — user-provided subprocesses that transform notebook JSON before consumption — given the q2 pipeline shape.

## Brief: how Quarto 1 does it

In Q1, ipynb-filters appear in two distinct paths:

1. **Pre-execute, format-aware partition path.** `ExecutionEngineInstance.partitionedMarkdown(file, format)` for Jupyter calls `markdownFromNotebookFile(file, format)` which calls `jupyterNotebookFiltered(file, format.execute["ipynb-filters"])`. The notebook is read from disk, filters chain (output of one → input of next), and the filtered JSON is converted to markdown. Filter results cache by `(file mtime, filter script mtimes)`. Two callers consume this filtered partition:
   - **Format-resolution YAML harvest** (`render-contexts.ts:632`): merges filter-injected YAML into the already-resolved format config, pre-execute. Filters can therefore override format-level settings.
   - **Project-index population** (`project-index.ts:102`): builds per-file index entries — title, headingText, draft — from filter-aware content. Powers website navigation, book chapters, blog listings, cross-doc link resolution.

2. **Post-execute filter pass on the kernel-output notebook.** `ExecutionEngineInstance.execute()` runs the kernel against the *unfiltered* notebook (`executeKernelOneshot/Keepalive`), then runs the same filter chain against the post-execution notebook (`notebookFiltered(target.input, ...)`), then converts to markdown for the engine result. Filters can therefore modify cell outputs and metadata produced by the kernel.

`ExecutionEngineInstance.markdownForFile(file)` is the **un**filtered conversion path — it goes through `markdownFromNotebookJSON(nb)` directly, with no filter chain. It feeds `target.markdown` (the engine's "what the user wrote" view).

In practice ipynb-filters are a project-level setting (`_quarto.yml` `execute.ipynb-filters: [...]` or set by an extension); nbdev is the only known user.

## Why q2 should consolidate filtering at `markdown_for_file`

q2's pipeline differs from Q1's in two ways that matter for this feature:

- **`EngineClaimsFileStage` runs before `ParseDocumentStage`** (Plan 1c). Non-QMD inputs (`.ipynb`, percent scripts) are converted to QMD via the engine's `markdown_for_file` *before* the parser ever sees them. By the time `MetadataMergeStage` and `DocumentProfileStage` run, the content is already converted. This is the single conversion point — there is no later pre-execute hook the way Q1 has.
- **`DocumentProfile` (post-merge, pre-mutation) is q2's analog of Q1's `partitionedMarkdown` for cross-document features.** Sidebars, link rewriting, listings, project indexing, the incremental-rebuild profile cache — all of these read `Vec<DocumentProfile>` via `ProjectIndex`, not engine output. Whatever metadata Q1 reads from `partitionedMarkdown.yaml/headingText/draft` is exactly what `DocumentProfile` exposes.

That means there is no remaining caller in q2 for a "filter-aware partition" engine method. Both Q1 callers (format harvest, project indexing) are subsumed by the natural metadata-merge cascade and `DocumentProfile`, **provided the converted QMD is already filter-aware** when it enters `ParseDocumentStage`.

So q2's Jupyter engine should run ipynb-filters inside `markdown_for_file`. Once it does:

- The QMD entering `ParseDocumentStage` reflects filter-modified cell sources.
- `MetadataMergeStage` sees filter-injected document-frontmatter YAML and layers it normally with project + format defaults — no separate "harvest" step is needed; precedence is the existing merge precedence. **Caveat to re-validate (surfaced by the multi-engine merge work):** the existing merge precedence has a non-obvious behavior for keys whose *kind* differs across layers — `MergedCursor::as_array`/`as_value` take the topmost layer that defines the key and **drop lower layers of a different kind** (a project-level scalar `engine: jupyter` is discarded the instant a higher layer writes `engine:` as an array). So a filter that injects an `engine:`/`format:`-affecting key interacts with that precedence, not a naive last-writer-wins; confirm against an nbdev fixture (see `claude-notes/plans/2026-05-27-multi-engine-execution.md` "Merge behavior" and the Open Questions below).
- `DocumentProfileStage` produces a filter-aware profile, so project indexing is automatically correct.
- `EngineExecutionStage` sends the filter-aware QMD as `TsExecuteOptions.input`. Q1 sends *unfiltered* markdown there and re-reads the filtered notebook inside execute(); the q2 engine can do whichever it wants — filters live inside Jupyter's own implementation, and the protocol's `input` field carries whatever Jupyter chose to expose.

The implementation needs a shared `(file mtime, filter script mtimes) → filtered JSON` cache between `markdown_for_file` and the engine's kernel-execute path so filters don't run twice. Q1 has this (`filteredNotebookFromCache`); q2 needs the equivalent.

## Trade-offs of consolidating

This is a deliberate departure from Q1 in two ways:

- **Per-document `ipynb-filters: [...]` no longer takes effect.** Filter resolution has to happen before parse, but document frontmatter isn't read until after parse. So filter list can only come from project-level config (`_quarto.yml` or extensions). The earlier draft of this plan documented Q1's de-facto pattern: *"In practice, ipynb-filters is only specified at project level (`_quarto.yml`) or in extensions, never per-document."* nbdev is the only known user and uses project-level only, so the practical impact is nil. If a future use case appears, a frontmatter-level pre-filter stage could be added — not painted into a corner.

- **Engine `execute()` receives filtered QMD as `input`, not unfiltered.** Q1 hands engines `target.markdown` as the user's pre-filter view. q2's simplification means engines see post-filter content in `TsExecuteOptions.input`. The Jupyter engine still re-reads the source notebook for kernel input (matching Q1's pattern there), so kernel error positions still relate to user-authored cells. Most other engine uses of `input` (cell-option parsing, structural inspection, source mapping) are content-equivalent under filters that don't structurally rearrange cells.

- **Source-mapping for filtered cells matches Q1's partition path** (which already loses original-cell provenance for filter-modified cells), not Q1's execute path (which preserves it because it doesn't pre-filter). Q1 already accepts this asymmetry. q2 makes the loss uniform across all consumers of the converted QMD. Engines that want to rebuild original-cell positions can maintain their own filter line maps; that work is no different from what Q1 would need to do.

The "upgrade" framing is that filter awareness reaches every q2 consumer of the converted QMD instead of being scoped to two specific Q1 call sites. Whether that's net-positive for an engine extension depends on the engine; for nbdev's filters this is the desired behavior.

## What needs to change in the engine trait surface

The `ExecutionEngine` trait method `markdown_for_file(file, runtime) -> (String, SourceInfo)` does not currently expose project-level filter config. To run filters during conversion, the Jupyter implementation needs access to either:

- the relevant slice of `ProjectContext` (specifically `execute.ipynb-filters` from the merged project metadata), or
- a small `MarkdownForFileConfig` parameter passed by the calling stage.

Either is a small Plan 1c-adjacent change. Whichever lands, it should:
- be project-scoped, not per-document;
- be available at `EngineClaimsFileStage` time (i.e. after project config is read but before document parse);
- not force engines that ignore filters to handle config they don't care about.

## Implementation roadmap

This work depends on Plans 0, 1a, 1b, 1c being complete, and the website project pipeline on `main`. When picked up:

1. **Add the project-level filter list to the `markdown_for_file` plumbing.** Plan 1c-adjacent: extend the trait signature or pass a config struct. Built-in engines that don't run filters ignore it.

2. **Implement filter execution in q2's Jupyter `markdown_for_file`.** Spawn the filter scripts via `SystemRuntime::execute`, pipe notebook JSON through, chain results. Cache by `(file mtime, filter script mtimes)`. Reuse the cache from execute()'s kernel-input path so filters don't run twice per file.

3. **Wire the filter cache into the Phase-8 profile cache invalidation logic.** Filter scripts are part of the cache key for `DocumentProfile` of files they affect.

4. **Document the per-document constraint.** If `_quarto.yml`'s `execute.ipynb-filters` is project-level-only, document this and emit a warning if a frontmatter `ipynb-filters` is encountered.

5. **Test with nbdev fixtures.** The validation target.

## Open questions

- **Filter-injected YAML and `Format` selection.** If a filter injects `format: pdf` into the notebook YAML, format selection in q2 has already happened upstream of `markdown_for_file`. This matches Q1's behavior (format selection is pre-filter; only sub-format config is filter-overridable), but it's worth confirming with an nbdev test case before locking it in.

- **Concurrency and the filtered-notebook cache.** A project render that touches the same notebook from Pass 1 and Pass 2 should not re-run filters; the cache must be project-scoped (not per-pass).

- **Error reporting in filters.** When a filter subprocess fails or emits non-JSON, the diagnostic should name the filter script and the offending cell range if possible. Q1 just throws a bare `Error()`; q2 can do better.

## References

- Q1 entry point: `src/execute/jupyter/jupyter.ts` — `partitionedMarkdown` (line 360), `execute` filter call (line 499).
- Q1 filter chain: `src/core/jupyter/jupyter-filters.ts`.
- Q1 cache: `src/core/jupyter/filtered-notebook-cache.ts`.
- q2 pipeline shape: `crates/quarto-core/src/pipeline.rs` (stage order); `crates/quarto-core/src/document_profile.rs` (profile shape); `crates/quarto-core/src/project/orchestrator.rs` (two-pass orchestration).
- DocumentProfile contract: `claude-notes/designs/document-profile-contract.md`.
- Plan 1c: `claude-notes/plans/2026-04-16-plan1c-extension-integration.md` (the `EngineClaimsFileStage` / `markdown_for_file` flow).
