# pandoc-recordings

Recordings of real native pandoc runs (pandoc.wasm epic, phase R0). R1, R4, R5,
H0, H1 and H3 all read `recordings/`; do not fork it.

## Regenerate

```bash
cargo xtask capture-pandoc-recordings   # Unix only; needs the pinned native pandoc on PATH
```

It builds `q2`, renders every fixture in `crates/xtask/src/capture_pandoc_recordings.rs`
(`FIXTURES`) to docx, pptx, epub and typst with `QUARTO_PANDOC=scripts/pandoc-capture.sh`
and `SOURCE_DATE_EPOCH=1700000000`, then normalizes each capture with
`crates/quarto-pandoc-recording`. Review the diff: a pandoc, filter or fixture change
shows up here.

## Layout (`recordings/`)

| Path | Contents |
|---|---|
| `share/<tree-id>/`, `share/<tree-id>.manifest.json` | the share tree (`/__q2_share__`), stored once per distinct content |
| `<fixture>-<format>/argv.json` | argv including argv[0] (`pandoc`); replayed as-is pandoc would take `json` for an input file, so drop element 0 when spawning |
| `.../env.json` | env (`QUARTO_SHARE_PATH`, `QUARTO_FILTER_PARAMS`, `QUARTO_FILTER_DEPENDENCY_FILE`, `SOURCE_DATE_EPOCH`, locale) |
| `.../fs/__q2_tmp__/` | pipeline temp files: input JSON, empty dependency file, epub `pandoc-formats/`, typst template partials |
| `.../fs/__q2_doc__/` | the document directory (qmd and resources) |
| `.../meta.json` | share tree id, exit status, the `-o` path, `reference`, `capture_output` |
| `.../reference/<out>` | the byte-comparison reference: native pandoc's output at the canonical replay dir |
| `.../capture/<out>` | the output of the original capture run (paths are the capture machine's) |
| `.../manifest.json` | every file with size and sha256 (a browser cannot list directories) |

## Placeholders

Real roots are rewritten to `/__q2_tmp__`, `/__q2_share__` (the `share_root` in
`resources/pandoc-wasm.json`), `/__q2_doc__` and `/__q2_out__` in argv, env, the
decoded `QUARTO_FILTER_PARAMS` blob and every text file, and the rewrite fails if any real
root survives. The only intentionally dropped input is the pipeline's `typst-packages/`
cache (fonts, unread by pandoc).

## Replaying and comparing

`pandoc-recording replay recordings <name>` materializes a recording under
`/tmp/q2-pandoc-replay/<name>` (a fixed path: outputs embed the path pandoc ran at,
for example docx image descriptions) and checks the output against `reference/`.
docx, pptx and typst replays are byte-equal under `SOURCE_DATE_EPOCH`. An epub gets a
random `urn:uuid:` and its Lua-built xhtml attributes come out in random order, so epubs
are equal only after `quarto-output-extract compare` (see `crates/quarto-output-extract/src/epub.rs`
for exactly what it normalizes). The capture and replay tests are in
`crates/quarto-pandoc-recording/tests/integration/recordings.rs`.
