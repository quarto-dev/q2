---
title: 'Single-file `q2 preview`: the VFS dependency-bootstrapping problem'
date: 2026-06-16
description: 'Investigates how single-file `q2 preview` can learn which sibling files a deck needs before the in-browser file system is populated, weighing static pre-resolution, lazy fetching and render-driven discovery.'
---

**Strand:** bd-9cyza5vy (design exploration)
**Discovered-from:** bd-kpuweafo (which shipped a partial, direct-image-only fix)
**Status:** design exploration — *no implementation committed beyond bd-kpuweafo's
direct-image sync*. The point of this doc is to map the space before we commit to
a mechanism, because the "obvious" fix (parse harder) has a non-obvious circularity.

## The core problem (the bootstrapping paradox)

`q2 preview` renders in the browser via WASM. The WASM pipeline reads **every**
file it needs — the deck, `{{< include >}}`d files, images, `_brand.yml`,
bibliography, CSL, `resources:` — out of an **ephemeral in-browser VFS**. The
native `q2 preview` server is responsible for *populating* that VFS before (and
during) rendering.

> To populate the VFS correctly we must know **which** sibling files the deck
> (transitively) needs. But determining what a deck needs requires **parsing**
> it (and its includes, recursively) — which is exactly the work the WASM
> pipeline does *after* the VFS is populated.

So: **parse-to-know-what-to-sync** depends on **sync-to-be-able-to-parse**. That
circularity is the whole problem. It does not exist in `q2 render` (reads the
real filesystem directly, no VFS) and is sidestepped — not solved — by project
mode.

## Verified current behavior (2026-06-16, E2E)

Fixture: `main.qmd` → `{{< include part.qmd >}}`; `part.qmd` has
`![](./inc-image.png)`.

| | direct `![](./img.png)` | `{{< include part.qmd >}}` | image *inside* the include |
|---|---|---|---|
| `q2 render` (single file) | ✓ | ✓ | ✓ (`naturalWidth 320`) |
| `q2 preview` **project** (`_quarto.yml`) | ✓ | ✓ | ✓ (blob URL) |
| `q2 preview` **single-file** | ✓ *(bd-kpuweafo)* | ✗ literal `?include` | ✗ |

The intuition "includes work but their images don't" is **false**: in single-file
preview *includes don't work either*. It's one uniform root cause — the VFS only
contains what we statically pre-synced (deck + `_brand.yml` + the deck's direct
image refs).

## Why single-file mode can't just walk the directory

Project mode resolves the paradox by **over-population**: `ProjectFiles::discover`
walks the whole project tree and syncs every `.qmd` / config / binary, so whatever
the pipeline later asks for is already present. Single-file mode (`bd-tnm3k`)
*deliberately refuses to walk*, because its "project root" is just the deck's
parent directory — which for `q2 preview ~/Downloads/note.qmd` is `~/Downloads`.
Walking it would index hundreds of unrelated files into automerge docs (cost,
privacy/noise, and — with `--allow-edit` — write-back surface). So single-file
mode must populate the VFS with a **precise** set, which forces it to *discover*
that set, which forces the paradox.

## The dependency channels (what a deck can pull in)

Any complete solution has to account for every way a `.qmd` references a sibling.
Today only `{{< include >}}` (via `deps.rs`, for the re-render *filter*) and
direct `Image` URLs (via bd-kpuweafo, for *sync*) are handled. The full set:

- `{{< include f.qmd >}}` — and **transitively**, includes within includes.
- `Image` URLs `![](./img.png)` — in the deck **and inside included files**.
- `_brand.yml` (done: bd-ggvq1j68) and themes (`theme: custom.scss` + its `@import`s).
- `bibliography:` / `csl:` paths.
- `{{{< embed other.qmd#cell >}}}` (embeds another doc's output).
- `resources:` globs.
- Raw HTML `<img src>`, `<video>`, `<link>`, `<script src>`; CSS `url(...)`.
- Listing/sidebar globs (project-shaped; less relevant single-file).
- **Dynamically generated** references (an R/Python cell that writes
  `![](plot.png)` at runtime) — *not statically discoverable at all*.

The long tail matters: a per-channel static extractor is a perpetual game of
catch-up and risks **drift** from what the render pipeline actually consumes.

## Design options

### A. Transitive static pre-resolution (extend bd-kpuweafo)
Parse the deck → collect includes + assets; recurse into each include; sync the
closure (included `.qmd` as text, images as binary). Re-run on deck-edit watch
events to kill the mid-edit staleness.
- **Pros:** stays inside today's VFS-sync model; no new runtime mechanism;
  deterministic; respects the no-walk safety property.
- **Cons:** re-implements dependency analysis *outside* the pipeline → two
  sources of truth → drift as channels are added; never covers dynamically
  generated refs; each channel is bespoke code.

### B. Lazy VFS population (on-miss fetch from the server)
When the WASM `file_read` misses, fetch the file from the native server (served
from disk, under the deck dir, with a traversal guard), populate the VFS, retry.
- **Blocker:** include-expansion and asset reads are **synchronous against the
  VFS**. On-miss fetch is async → needs either (a) async-ifying those reads (deep
  pipeline change) or (b) a multi-pass render (see C).
- **Pros:** no static analysis; handles arbitrary/dynamic refs; always fresh.
- **Cons:** reintroduces a disk-serving path (security posture shift,
  cf. `bd-teh4hbli`); single-file↔project mechanism divergence; the sync change
  is large.

### C. Render-driven discovery to a fixpoint (reuse the pipeline's own deps)
Run a lightweight **dependency pass** natively (quarto-core *is* native): parse
deck → discover direct deps → sync them → re-run → discover *their* deps → … until
the dependency set stops growing, then hand the populated VFS to the WASM render.
Crucially, source the dep set from the pipeline's **existing** machinery
(`IncludeExpansionStage`/`extract_include_path`, `ResourceCollectorTransform` /
`collect_referenced_asset_urls`, the resource report, and possibly the
`DocumentProfile` checkpoint) rather than a parallel walker — **one source of
truth**.
- **Pros:** no drift (the thing that decides what to sync is the thing that
  renders); covers every static channel for free as the pipeline learns them;
  mirrors the established project-orchestrator pass-1/pass-2 shape.
- **Cons:** iterative parse-sync-reparse; native side must run (part of) the
  pipeline twice (cheap for a single deck); still can't see dynamically generated
  refs (no static analysis can).

### D. Bounded / filtered walk
Walk the dir but cap count, or only sync files matching referenced *names*, or
warn past a threshold.
- **Cons:** still indexes unrelated files (the `~/Downloads` privacy/noise
  problem); the cap is arbitrary. This is just a softened `bd-tnm3k` violation.

### E. Async file_read in the WASM pipeline (enabler, not a standalone fix)
Make the pipeline's file reads async so on-miss lazy fetch (B) becomes possible
without a multi-pass. Largest blast radius; listed because it's the lever that
would make the cleanest lazy model viable, and may be worth it independently.

## Evaluation criteria

1. **No over-collection** — honor `bd-tnm3k` (don't index `~/Downloads`).
2. **No drift** — ideally a single source of truth for "what does this doc need".
3. **Freshness** — a reference added mid-edit should resolve (re-resolve on edit).
4. **Channel coverage** — includes, images, brand, bib/CSL, embed, resources, raw
   HTML/CSS refs; gracefully concede dynamically-generated refs.
5. **Bounded cost** — single deck; no pathological fan-out.
6. **Security** — don't broaden disk exposure (cf. `bd-teh4hbli`); keep the
   under-deck-dir containment guard.
7. **Mechanism parity** — minimize single-file↔project divergence.

## Tentative leaning (to be challenged)

**C (render-driven fixpoint), sourcing deps from the pipeline's own machinery**,
scores best on no-drift + coverage + parity, at the cost of an iterative native
pre-pass. **A** is the pragmatic incremental step (it's what bd-kpuweafo already
started) and could be the v1 that C later subsumes — *if* we're disciplined about
reusing pipeline extractors rather than writing parallel ones. **B/E** are the
"right" long-term shape if we ever want lazy/dynamic resolution, but they're a
much larger architectural commitment.

## Open questions

- Is there already a single native entry point that returns a deck's full static
  dependency closure (the `DocumentProfile` / resource report)? If so, C is mostly
  plumbing. If not, what's the cheapest way to get one without running engines?
- Do we *want* mechanism parity with project mode, or is single-file allowed to
  diverge (e.g. lazy fetch) as long as the rendered result matches?
- How should dynamically-generated references (code-cell-authored `![]()`) be
  handled — out of scope, or a documented "won't preview until the file exists +
  reload" caveat (engines don't run in WASM preview anyway; captures are replayed)?
- Mid-edit re-resolution: hook the existing single-file watcher to re-run
  discovery, or recompute on each render request?

## References

- Implementation that shipped the partial fix: `2026-06-16-single-file-preview-referenced-assets.md`
- Strands: bd-9cyza5vy (this), bd-kpuweafo (direct images, done), bd-ggvq1j68
  (`_brand.yml`, done), bd-tnm3k (single-file mode / no-walk), bd-teh4hbli
  (artifact route trust boundary), bd-kjrpya2d (`resources:` `.html` injection —
  prior art for "resolve upstream, inject via HubConfig").
- Code: `crates/quarto-hub/src/discovery.rs` (`single_file`, `with_*`),
  `crates/quarto-hub/src/context.rs` (single-file branch + reconcile),
  `crates/quarto-preview/src/config.rs` (`resolve_single_file_assets`),
  `crates/quarto-preview/src/deps.rs` (`extract_include_deps` — the include
  extractor), `crates/quarto-core/src/transforms/resource_collector.rs`
  (`collect_referenced_asset_urls`), the project orchestrator's pass-1/pass-2
  (prior art for a render-driven pre-pass).
