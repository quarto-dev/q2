# Moby Dick import spike (P1 T8 replacement input)

Branch `import/p1-pandoc-reader` @ b3f7e4c67, pandoc 3.11 (native and wasm), 2026-10-03/04, macOS arm64.
Replaces T8's adversarial "compressible" docx with a realistic large document. No product code, limits,
plan checklist or epic changed.

## Verdict: PASS

| Criterion | Result |
|---|---|
| wasm run succeeds | yes (3/3 vitest, 3/3 Chromium, 3/3 WebKit) |
| wasm JSON equals native JSON | yes (see caveat on media path) |
| Chromium `peakMb - startMb` < 1024 | **251-255 MB** |
| wall time < 60 s | **3.7 s** in Chromium; 10.8 s in node vitest |

## Inputs

- Source: Project Gutenberg #2701, `https://www.gutenberg.org/ebooks/2701.epub.noimages` -> 728,811 B EPUB
  (despite "noimages" it carries one cover JPEG).
- `SOURCE_DATE_EPOCH=1700000000 pandoc moby.epub -o moby.docx` -> **758,405 B** docx, no stderr.
- Native read: `pandoc -f docx --track-changes=all -t json --extract-media=<dir> -o out.json moby.docx`
  -> **1.36 s**, `out.json` 8,229,409 B, one extracted file (`media/rId9.jpg`, 148,596 B).
  Compare `collected_total_bytes` = 300 MiB: this input uses about 0.05 % of it.

## Native vs wasm

Same argv as interface 2 (`importJob.ts` / the `images-docx` recording's request shape).

- wasm output is 8,229,257 B vs native 8,229,409 B. The 152-byte difference is exactly the
  `--extract-media` prefix: native ran with a scratchpad path, wasm with `/__q2_share__/import/media`.
  After substituting the prefix in the native text, `JSON.parse` of both are deeply equal (vitest `toEqual`).
- wasm collected exactly one file, `/__q2_share__/import/media/media/rId9.jpg`, 148,596 B (same size as native).
- wasm stderr empty; host diagnostics `[]`.

## Wall time and memory

Vitest (node, `vitest.wasm.config.ts`, `execute()` directly, includes nothing but the run): 10743, 10904, 11022 ms.
Node is ~3x slower than the browser here (no warm tier-up / different engine); the browser is the relevant figure.

Playwright (fresh page + tiny-docx warm-up per run, RSS of all `ms-playwright` processes sampled every 100 ms;
no other Playwright browsers were running; preview server on :5199):

| Browser | runs: elapsedMs | runMs | wasm memoryBytes | growth (peak - start) MB |
|---|---|---|---|---|
| Chromium | 3670 / 3693 / 3686 | ~3650 | 233,439,232 (223 MiB) | 255 / 252 / 251 |
| WebKit | 3330 / 3334 / 3297 | n/a (see json) | - | 275 / 271 / 274 |

Chromium baseline RSS (start) was 770-840 MB, peak 1019-1091 MB; the trigger is the growth, not the peak.
Firefox was not run (does not launch here).

## Content notes (for P3's transform)

`pandoc -f json -t markdown --wrap=none` on the wasm JSON: 1.26 MB markdown, no stderr.

- **Headings**: 148 `Header` blocks: 1 h1 (title), 141 h2, 5 h3, 1 h4. All **135 chapters** present as h2
  `CHAPTER N. Title.` (note "135. The Chase.---Third Day." where the EPUB had an em-dash), plus Etymology,
  Extracts, Epilogue, and h3 sub-headings (e.g. the Etymology subtitle). Gutenberg boilerplate is *in* the body:
  "The Project Gutenberg eBook of ..." h2, "Original Transcriber's Notes" h4, and the full licence h2 at the end.
  Chapter headings are real headers, not style-derived divs.
- **Metadata**: only `title` ("Moby Dick; Or, The Whale") and `author`; metadata title (h1 in body is a second,
  different title "MOBY-DICK; or, THE WHALE.").
- **Inline**: 216k `Str`, 392 `Emph`, 7 `Strong`, 151 `LineBreak` (verse and the extracts), 143 `Link`.
  No `Note`/footnotes at all (0), no `Div`s, no `Span`s other than anchors.
- **Spans**: 138 `Span` all with class `anchor` and an id like `Xf0e95592...` (docx bookmarks the EPUB->docx
  step emitted before each chapter/TOC target); the 143 links are internal TOC links to them. P3 should
  expect a bookmark-anchor span before nearly every heading and decide whether to keep or strip them.
- **Images**: 2 `Image` nodes (cover, title page?) both pointing at the *same* collected file
  `/__q2_share__/import/media/media/rId9.jpg` (note the doubled `media/media` from pandoc's
  `<extract-dir>/media/` layout), with `width=5.833in height=9.176in` attrs, empty alt/title. One collected
  file serves both references - collection must dedup by path, not count one per reference.
- Other blocks: 2842 `Para`, 30 `Plain`, 1 `Table`, 1 `BulletList`, 2 `BlockQuote`, 2 `HorizontalRule`.
- No track-changes spans or comments (the source is an EPUB conversion, so `--track-changes=all` was a no-op).

Not exercised by this document: footnotes, comments, tracked changes, style-derived divs, many images.

## Reproduce

```sh
S=<scratch>; cd $S
curl -sSL -o moby.epub https://www.gutenberg.org/ebooks/2701.epub.noimages
export SOURCE_DATE_EPOCH=1700000000
pandoc moby.epub -o moby.docx
time pandoc -f docx --track-changes=all -t json --extract-media=$S/nat/media -o nat/out.json moby.docx
```

`repro/` holds the throwaway specs used (stored as `.txt` so they are not picked up by vitest/playwright globs):

- `mobyDick.spike.wasm.test.ts.txt` -> `hub-client/src/pandoc/mobyDick.spike.wasm.test.ts`;
  `MOBY_DOCX=... MOBY_NATIVE_JSON=... MOBY_NATIVE_MEDIA_DIR=... MOBY_OUT=... npx vitest run --config vitest.wasm.config.ts src/pandoc/mobyDick.spike.wasm.test.ts`
  (`console.log` is swallowed by this config, so stats are appended to `$MOBY_OUT.stats`).
- `pandoc-import-moby.harness.spec.ts.txt` -> `hub-client/e2e/`, with `playwright.moby.config.ts.txt` as a
  throwaway config (copy it into `hub-client/`, set `webServer` port 5199 and `outputDir`):
  `MOBY_DOCX=... MOBY_OUT=<x>.json npx playwright test -c <config> --project=chromium` (then `--project=webkit`).
  Needs `dist/` built with `VITE_E2E=1` (it has the `runImport` hook).
