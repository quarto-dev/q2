# R0 exploration: a versioned, hashable `PandocRequest`

**Date:** 2026-10-01. **Phase:** pandoc-request R0 (`claude-notes/plans/2026-10-01-pandoc-request-R0-foundations.md`).
**Method:** the 24 committed recordings (`crates/quarto-core/tests/fixtures/pandoc-recordings/`), replayed with the pinned native pandoc 3.11 through `pandoc-recording replay --env/--arg`; a throwaway Python prototype of the job id over the recordings\' `argv.json`, `env.json`, `manifest.json` and `meta.json`. Native only: the wasm side is H0's.

## Recommendation (for R1)

Ship both fields in R1's first schema; the cost is small and the checks below passed.

1. **`schema_version`: yes.** An integer in the request and the published JSON Schema; the host refuses a request whose version it does not know.
2. **`job_id`: yes, computed by Rust (`prepare()`), opaque to TS.** `sha256` over a canonical encoding of `{schema_version, tool: wasm_sha256, argv[1..], env minus SOURCE_DATE_EPOCH, files: {path: sha256}, share_tree: tree id}`: JSON, keys sorted, no whitespace, UTF-8, first 16 hex digits is enough for a cache key. In the prototype it gave 24 distinct ids for the 24 recordings and changed on every one of: an env change, an extra argv element, one file's content. `SOURCE_DATE_EPOCH` is excluded by construction (see next point). The share tree is identified by its content id (the recordings already do this: `share/<tree-id>`), and `files` by content hash, so the id does not depend on mtimes or temp paths.
3. **`SOURCE_DATE_EPOCH` goes in `env`, set at click time in production and fixed in tests.** Evidence (docx, pptx, typst, all fixtures tried):
   - with it pinned, two replays are byte-identical *whole files*, not only `document.xml`;
   - changing it changes docx and pptx bytes (zip entry times, `docProps/core.xml` `created`/`modified`), so it must be pinned for reproducible output, and a job id that included it would never repeat;
   - removing it makes output depend on the wall-clock second (two runs in the same second can still match);
   - typst output does not depend on it.
4. **Epub is not byte-reproducible yet, with an `identifier` or without.** Without one, pandoc writes a random `urn:uuid:`. With `-M identifier=…` and `SOURCE_DATE_EPOCH` pinned, the fixtures with no callouts (tables, images) were byte-identical across runs, but the callouts fixture still differed in `ch001.xhtml`: the vendored Lua builds attribute tables and emits `data-icon`/`data-appearance` in per-run random order (Lua `pairs` order). Consequences: (a) R5 should supply an `identifier` (derived from the document) so epub ids and downloads are stable; (b) byte-identical epub needs deterministic attribute emission in the vendored filters, which departs from Q1's files, so do not promise it: until someone decides, epub equality is semantic (`quarto-output-extract compare`, which sorts xhtml attributes and normalizes uuids/timestamps).
5. **pptx has no random UUIDs here.** The GUID-looking values in a pptx (`{05A4C25C-…}`) come from pandoc's reference.pptx and are identical across runs and across `SOURCE_DATE_EPOCH` values; the only run-to-run differences were `docProps/core.xml` timestamps (and zip times). The plan's expectation of "random pptx UUIDs" did not materialise for pandoc 3.11 with these fixtures.

## What the exploration also found (affects other phases)

- **Outputs embed the path pandoc ran at** (docx image `descr` holds the share-tree path, for example). A wasm run at `/__q2_share__` therefore differs from a native run at a temp path in those strings, and a native replay at a different directory differs from the capture. The recordings handle the native side with a fixed replay directory (`replay::canonical_work_dir`); H0/H3 comparisons across native and wasm must be semantic. If a byte-identical native/wasm comparison is ever wanted, the answer is to run native at `/__q2_share__` too, which needs root or a chroot-like mount, and is not recommended.
- **The job id is only useful as a cache/dedupe key if `prepare()` is deterministic, and today it is not.** The pipeline temp dir name is random and appears in the input JSON's source info and in the params blob (the rewrite step normalizes it for recordings). Not tested: whether two renders of one document otherwise produce identical input JSON. R1 should make `prepare()` emit placeholder-rooted paths (`/__q2_tmp__`, `/__q2_doc__`, `/__q2_share__`) directly, which also removes the rewrite step for wasm; otherwise the job id would change on every render.

## PDF as a second job instead of a `post` step

Today's design has the typst request carry `post: compile_typst`. As a second job fed by the first, what it would take:

- **Job 1** is the typst-writer request with `output_path = *.typ`, unchanged. **Job 2** is a different kind (`TypstCompileRequest`): inputs are job 1's output *by job id* (the host resolves it to bytes), the project files the `.typ` references, the vendored typst packages, the font set (hash of the font list, which `typst-available-fonts` already needs before job 1 runs), the compiler's wasm hash, and `SOURCE_DATE_EPOCH` for PDF metadata.
- **Needed in the schema:** a `kind` discriminator, `inputs_from: [job_id]`, and the same `job_id` rule (the tool hash is the typst wasm's). Job 2's id embeds job 1's id, so a PDF is cached per (document state, fonts, compiler).
- **What it buys:** the `.typ` result is reusable (Download as typst and as PDF share job 1), each job is independently cacheable and cancellable, and `post` does not have to grow a plugin interface.
- **What it costs:** an orchestrator that runs a small DAG instead of one request, and a decision on where job 2's request is built (Rust, to keep "TS never re-derives anything Rust owns"). Not needed for R1; R1 should only reserve `kind` (default `pandoc`) and leave `post` as is, so H7-H9 can adopt the DAG without a schema break. This is a recommendation, not a decision: the PDF phases decide.

## Reversibility

Nothing here changed production code. The prototype lives only in this note; R1 owns the real implementation. `pandoc-recording replay --env/--arg/--no-compare` stays as a small general tool.
