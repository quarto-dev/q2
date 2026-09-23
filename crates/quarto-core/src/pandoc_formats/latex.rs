//! `latex` — documented stub, not implemented (P7 Task 4).
//!
//! `--to latex` continues to return `Err("Unknown format: latex")` from
//! [`crate::format::Format::from_format_string`] — this module intentionally
//! adds **no** `FormatIdentifier::Latex` variant. Its only purpose is to
//! record the shape a future implementation would need, so the scope-out is
//! a committed decision rather than a silent gap.
//!
//! Q1's `latexFormat()` (`v1.11.3:src/format/latex/format-latex.ts`) is the
//! richest `formatExtras` of any format this epic surveyed: KOMA-script
//! template context (`documentclass`, `classoption`, `papersize`), a
//! citation-processing toggle distinct from `citeproc`, and numerous
//! `postprocessors` that rewrite the `.tex` source after pandoc emits it
//! (float placement, `\includegraphics` sizing, raw-LaTeX passthrough for
//! custom environments). None of that machinery exists in Q2.
//!
//! **`format: latex` emits `.tex` directly — the file extension wins over
//! the inner `pdf` recipe** (`v1.11.3:src/config/format.ts`'s `kPdf`
//! recipe wraps `latex` + a `latexmk`/`tectonic` postprocessing step only
//! when the *target* is `pdf`, not when it is `latex` itself). So a real
//! `latex` implementation is in scope for a future `--to latex` follow-on;
//! `--to pdf` (the latexmk/tectonic recipe) is explicitly **out of scope**
//! for this epic entirely, per
//! `claude-notes/plans/2026-08-20-pandoc-hybrid-epic.md`'s Prioritization
//! section.
