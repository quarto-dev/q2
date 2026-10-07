# `incr_compile` spike (H9, T6): verdict NO

**Question.** Is an incremental-compile PDF-preview mode worth building on typst.ts's `incr_compile`?

**Method.** Throwaway vitest in `ts-packages/typst-host` (deleted after the run, not committed), Node, typst.ts 0.7.0 / typst 0.14.2,
one warm `TypstSession` per fixture, the recorded `.typ` of the typst fixtures with one line appended per edit (12 edits each).
PDF: `session.compile` (PDF format). Incremental: `compiler.withIncrementalServer` + `compile({format: 'vector', incrementalServer})`,
shadow files re-mapped per edit. Times are the median over edits 2-12 (edit 1 is the cold compile).

| fixture   | PDF warm | PDF cold | incr warm | incr cold | delta size |
|-----------|---------:|---------:|----------:|----------:|-----------:|
| callouts  |  4.2 ms  |  131 ms  |  0.4 ms   |   9 ms    |   940 B    |
| citations |  2.7 ms  |  170 ms  |  0.2 ms   |   8 ms    |  1004 B    |
| crossrefs |  4.8 ms  |  148 ms  |  0.3 ms   |   8 ms    |   844 B    |
| images    |  1.8 ms  |   98 ms  |  0.1 ms   |   7 ms    |   860 B    |
| tables    |  2.8 ms  |  112 ms  |  0.2 ms   |   8 ms    |   956 B    |

(The fixtures are small and typst memoizes, so absolute numbers are best cases.)

**Per-edit latency number.** The typst leg of a warm plain PDF compile is 2-5 ms; incremental saves at most ~4 ms of it. Each edit still
pays the pandoc leg: 165 ms in Chromium, 299 ms in WebKit (evidence section 12). So a PDF preview refresh is ~170-300 ms either way, and
the incremental path changes it by about 1-2 %.

**Verdict: no.** It cannot lower per-edit latency where it matters (the pandoc leg), its output is a vector delta for typst.ts's own
renderer rather than a PDF (so it would need that renderer and lose the pdf.js viewer), and the D2(b) warm-pandoc-instance spike, the
only thing that would cut the dominant cost, is a separate and larger question. Compile-on-demand "Download as PDF" plus the pdf.js viewer
(which reopens a recompile in place and keeps position) is the baseline and stays the design. No code kept; D2(b) not run.
