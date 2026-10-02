# import-recordings

Recordings of native pandoc *reader* runs for document import (epic
`claude-notes/plans/2026-10-03-document-import-epic.md`, plan P1). P3's Rust tests and
P1's real-wasm tests (`hub-client/src/pandoc/pandocHost.wasm.test.ts`) read the same
directories, so wasm and native are checked against one truth. Sibling of
`../pandoc-recordings/`, which records writer runs.

## Regenerate

```bash
cargo xtask capture-import-recordings generate   # rebuild source.<ext> from sources/ (when a source changes)
cargo xtask capture-import-recordings            # run native pandoc over source.<ext>, rewrite the rest
```

Unix only; needs the pinned native pandoc on `PATH` (the version in
`resources/pandoc-wasm.json`). Both run with `SOURCE_DATE_EPOCH=1700000000` and are
byte-identical on re-run. Capture reads only the checked-in `source.<ext>`; it never
deletes, lists or hashes `expected.qmd`, which P3 adds to each fixture directory.

## Layout: `<name>-<format>/`

| Path | Contents |
|---|---|
| `source.<ext>` | the input (generated from `sources/` by `generate`, then checked in) |
| `argv.json` | the exact interface-2 argv, paths under `/__q2_share__/import/` |
| `pandoc.json` | pandoc's `out.json`, pretty-printed with sorted keys; tests compare parsed JSON. Absent when pandoc fails (`corrupt-docx`) |
| `stderr.txt` | pandoc's stderr |
| `status.json` | `{ "status": <exit code> }` |
| `media/…` | every file `--extract-media` wrote, at its path relative to the extract dir |
| `manifest.json` | every file above (not `expected.qmd`, not itself) with size and sha256; a browser cannot list directories |

Native pandoc cannot write to `/__q2_share__`, so capture runs in a temp dir and relocates
that path to `/__q2_share__/import` in `pandoc.json` and `stderr.txt` (it fails if any temp
path survives).

## Turning `manifest.json` into an interface-2 media manifest

Each file under `media/` has `pandoc_path` = `/__q2_share__/import/media/` + its path
relative to `media/` (so `media/media/rId10.png` is `/__q2_share__/import/media/media/rId10.png`,
the path in the `Image` targets of `pandoc.json`), `sha256` from the manifest, and
`status: "stored"`.

## Fixtures

- `basic-docx`, `basic-odt`, `basic-rtf`, `basic-epub`, `basic-pptx`: headings, emphasis, a link, a table and an image (pptx: two slides with speaker notes, no image).
- `track-changes-docx`: the spike's insertion, deletion and comment cases, plus a comment with a reply over one range (two `comment-start` then two `comment-end` spans) and two paragraphs whose paragraph marks carry an insertion and a deletion (patched into `word/document.xml` by `generate`; pandoc cannot write those).
- `writer-bugs-docx`: the spike's `losses.md` (manual `1.`, `1)`, `+` and `-` text at a line start, multi-paragraph and list footnotes, restarted numbering, alpha and roman lists), escaped so the docx holds real `Str "1."` text.
- `highlights-docx`: highlighted runs (`Span .mark`).
- `images-docx`: a PNG and a JPEG, the PNG used twice (one extracted file).
- `corrupt-docx`: `basic-docx` truncated to 60%. pandoc exits 63 (`couldn't unpack docx container`); there is no `pandoc.json`.
- `emf-docx`: a hand-built EMF and WMF (one rectangle each). Both extract as `media/rId9.emf` and `media/rId12.wmf`.
- `quarto-made-docx`: a docx Quarto itself wrote (copied from `../pandoc-recordings/recordings/callouts-docx/reference/`), with layout tables.
- `comments-edge-docx`: the six cases of P1 T3 in one document (below).

## docx reader findings (P1 T3, pandoc 3.11, `--track-changes=all`)

Recorded from `comments-edge-docx` (and `track-changes-docx` for paragraph marks). These are
pandoc-written docx patched by `generate`; **expected, unverified for Word-saved files** until a
Word-authored docx is added. P3's transform consumes them; `comments-edge-docx/pandoc.json` is the
authority, excerpts here are condensed.

- **(a) Comment reply.** Two `comment-start` spans directly after one another (parent first), and the
  reply's `comment-end` nested **inside** the parent's `comment-end` span:

  ```
  Span ("", ["comment-start"], [id 0, author Ann, date …]) [Str "Parent", Space, Str "comment."]
  Span ("", ["comment-start"], [id 1, author Dee, date …]) [Str "Reply", Space, Str "comment."]
  Str "range" … Str "reply"
  Span ("", ["comment-end"], [id 0]) [ Span ("", ["comment-end"], [id 1]) [] ]
  ```

  The docx carries `commentsExtended.xml` with `w15:paraIdParent`; pandoc ignores it. The reply is
  recognizable only by this adjacency and nesting.
- **(b) Resolved comment.** Not distinguishable. The docx carries `w15:done="1"` for comment 2; its
  `comment-start` / `comment-end` spans are identical in shape to an unresolved comment's.
- **(c) Paragraph-mark changes.** An empty span as the paragraph's last inline:
  `Span ("", ["paragraph-insertion"], [author Bob, date …]) []` (`paragraph-deletion` likewise).
  Present in `track-changes-docx` and `comments-edge-docx`.
- **(d) Multi-paragraph comment text.** The comment's paragraphs are joined by `LineBreak` inside the
  one `comment-start` span: `[Str "First" … Str "comment.", LineBreak, Str "Second" … Str "comment."]`.
- **(e) Range starting inside emphasis.** The `Emph` is split around the start span:
  `Emph [Str "emphasised"], Space, Span .comment-start […], Emph [Str "words"], …, Span .comment-end`.
- **(f) Overlapping, non-nested ranges.** A flat sequence, not nested:
  `start 5, …, start 6, …, end 5, …, end 6` (each end span empty; only adjacent ends nest, as in (a)).
