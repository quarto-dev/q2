# Research Plan: ipynb-filters and Engine Partitioning

**Status:** Research — future work, not part of the TS engine extensions project
**Depends on:** TS engine extensions (Plans 1a/1b), native Jupyter engine
**Context:** This plan was identified during review of the TS engine extensions
grand plan. The protocol and trait additions needed to *support* this plan are
included in Plans 1a/1b (the `partitioned_markdown` trait method and
`PartitionedMarkdown` protocol message). This plan covers the *implementation*
of ipynb-filters and the pipeline integration to use them.

## Motivation

Quarto 1's `ipynb-filters` feature allows user-provided subprocesses to transform
notebook JSON before execution. Filter-injected YAML metadata can override format
configuration. Two callers observe the filtered output outside the execute path:

1. **Format resolution** (render-contexts.ts:632): harvests filter-injected YAML
   and merges it into the already-resolved format config, pre-execute.
2. **Project indexing** (project-index.ts:102): builds per-file index entries
   (title, headingText, draft) from filtered content for website navigation,
   book chapter listings, blog entries, and cross-document link resolution.

In practice, `ipynb-filters` is only specified at project level (`_quarto.yml`)
or in extensions, never per-document. This simplifies the integration — we don't
need full metadata merging before knowing whether filters are in play.

## What's already in Plans 1a/1b

The TS engine extensions project delivers the infrastructure this plan builds on:

- **`partitioned_markdown` on the `ExecutionEngine` trait** (Plan 1a Phase 3) —
  with default impl `partition(markdown_for_file(file).value)`. Jupyter overrides.
- **`PartitionedMarkdown` protocol message** (Plan 1a Phase 1) — so TS engine
  extensions can also implement `partitionedMarkdown`.
- **`TsPartitionedMarkdown` protocol type** (Plan 1a appendix).
- **`EngineMeta.has_partitioned_markdown`** — TsEngine forwards to subprocess
  if the engine reports having it, falls back to default impl otherwise.
- **`target()` as harness-internal** — the harness checks if the TS engine
  implements it, calls it if so, keeps the result (including opaque `data`
  cookie) on the Deno side. Not a protocol message or Rust trait method.

## Scope of this plan

1. **ipynb-filter subprocess execution** in the native Rust Jupyter engine.
2. **Pipeline integration** for the format resolution YAML harvest.
3. **Project index integration** for filter-aware project scanning.
4. **`partition_markdown()` utility function** and `PartitionedMarkdown` Rust type.

## Background

### What partitionedMarkdown returns

Quarto 1's `PartitionedMarkdown`:
```typescript
{ yaml?: Metadata, headingText?: string, headingAttr?: PandocAttr,
  containsRefs: boolean, markdown: string, srcMarkdownNoYaml: string }
```

What callers actually use:
- **Format resolution**: only `yaml` (to merge filter-injected YAML into format)
- **Project indexing**: `yaml.title`, `yaml.draft`, `headingText`, plus stores
  the full partition in the index entry

### How ipynb-filters work (Quarto 1)

Filters are subprocesses: each receives notebook JSON on stdin, outputs
transformed JSON on stdout. Filters chain (output of one → input of next).
Results are cached on disk keyed by mtime of notebook + all filter scripts.

The filter list comes from `format.execute["ipynb-filters"]`.

### How target() is used

In Quarto 1, `target()` is called before format resolution. Its `metadata`
feeds format resolution, and its `data` cookie (e.g., Jupyter's kernelspec)
is passed through to `execute()`.

In q2, `target()` is harness-internal for TS engines and not needed on the
Rust side because:
- q2's pipeline extracts metadata from the parsed AST (via pampa)
- q2 constructs execution context from the AST, not from a target() call

## Research Items

### R1: Rust-side `PartitionedMarkdown` type

**Resolved in Plan 1a.** The struct has all 6 fields matching Quarto 1,
using q2-native types (`ConfigValue` for yaml, `PandocAttr` for heading
attributes). `TsEngine` converts from the protocol type
(`TsPartitionedMarkdown`) at the boundary.

### R2: `partition_markdown()` utility function

Needed for the default `partitioned_markdown` trait implementation (delivered
by Plan 1a). Splits QMD text into YAML frontmatter, first heading, and body.
Options:
- String-based splitter (regex or manual — lightweight, no tree-sitter)
- Use pampa's parser (accurate, but heavier — do we want a pampa dependency
  in the engine trait's default impl?)

The default trait impl is `partition(markdown_for_file(file).value)`, so
this function receives QMD text (already converted from percent/spin scripts).

### R3: Native Jupyter engine ipynb-filter implementation

The Rust Jupyter engine needs to override `partitioned_markdown` to:
- Read notebook JSON from disk
- Run filter subprocesses (stdin/stdout JSON chaining)
- Cache filtered results (mtime-based, matching Quarto 1's scheme)
- Convert filtered notebook JSON → markdown text
- Partition the markdown

**Open question:** How much of the notebook→markdown conversion does the
native Jupyter engine already have? What's the gap?

### R4: Pipeline integration for format resolution YAML harvest

After `MetadataMergeStage`, check if merged config has `execute.ipynb-filters`.
If so, call `engine.partitioned_markdown(file, Some(&format))` and merge the
returned YAML into the format.

This could be:
- A step inside `EngineExecutionStage` (before the main execute call)
- A new mini-stage between MetadataMerge and IncludeExpansion
- A hook in MetadataMerge itself

**Open question:** Where exactly in the pipeline? The engine must already be
identified (from `ctx.claimed_engine_name` or detection).

Since ipynb-filters are project-level only, the pipeline knows about them
from `_quarto.yml` before parsing any document — no chicken-and-egg.

### R5: Project index integration

When q2 builds a project-wide index (website nav, book chapters, etc.),
notebooks with ipynb-filters need `partitioned_markdown` called with the
resolved format. This requires:
- Engine subprocess running during project scanning
- Format resolution available per-file (at least enough to determine
  ipynb-filters presence)

q2 doesn't have project-wide nav indexing yet. Document as an integration
point for when that feature is built.
