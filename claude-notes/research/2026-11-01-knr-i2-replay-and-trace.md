# KNR interlude I2: knitr traces and replay

Date: 2026-11-01. Plan: `claude-notes/plans/2026-11-01-knr-interlude-spikes.md` (I2). Strand
bd-js24dold. Scratch branch `spike/knr-i2` (off the epic branch, unmerged, unpushed).
Binary: `target/debug/q2` built from that branch. Scratch files are in the session scratchpad
(`.../scratchpad/i2/`). Every claim is marked verified (command or file) or "reported, not
re-run".

## Answer

The replay fix is **about 20 lines** and can land on its own, before KNR-3, under bd-js24dold:
it does not belong to the flip. A second change that the flip needs, dispatching on "the capture
has a record", is one line and is already in the prototype. KNR-6 decision 2 holds in effect,
but its mechanism is wrong as written: an old knitr trace needs **no presenter**. Details below.

## 1. The S6 repro, re-run (verified)

Document: `i2/doc/doc.qmd` (`engine: knitr`, `trace: true`, one `{r}` chunk with `sum(x)`).

```bash
Q=target/debug/q2                         # this branch, before the change
$Q render doc.qmd                         # exit 0; writes .quarto/trace/doc/latest.json.gz
cp .quarto/trace/doc/latest.json.gz ../trace-knitr.json.gz
$Q render doc.qmd --replay ../trace-knitr.json.gz --output replayed.html     # exit 0
```

- The live `doc.html` contains `[1] 6`; `replayed.html` does not. Its chunk is a raw
  `<pre class="{r} code-with-copy">` block holding the source: unexecuted, exit 0, no message.
- The capture holds one entry with the keys `engine_name`, `input_qmd`, `result`. Its
  `result.markdown` does contain `[1] 6`, so the output was recorded and never used. There is
  no `record` key (knitr returns none yet).

## 2. Two things the repro turned up

- **Replaying a document that has `trace: true` overwrites the trace it replays.** My first
  replay read `.quarto/trace/doc/latest.json.gz`, then wrote a new trace to the same path with
  no `engine_captures`; the next replay failed with "Trace file ... has no `engine_captures`".
  Verified (`i2/replay1.log`). Copy the trace aside first. This is a footgun for the
  "attach a trace to a bug report" workflow in the replay doc; I added it to bd-js24dold rather
  than opening a strand.
- **Nothing notices a capture that was loaded and never used.** The silent result above is the
  absence of a check, not only of the claim. Proposed policy: after the engine stage, a
  `ReplayEngine` whose capture was never executed is an error. Not prototyped.

## 3. How Jupyter carries and presents a record (read)

`ReplayEngine::execute` (`engine/replay.rs`) compares the input byte for byte, parses
`capture.result`, and calls `present_record` only when `engine_name == "jupyter"`. Otherwise it
returns the stored result. `present_record` reads `capture.record`, fills ids for the stage\'s
units, calls `present_document(...)` with the current scope and target format, keeps the stored
side data, and returns the record on the result. Its only Jupyter-specific parts are the
`JUPYTER` constant in the error text and the dispatch. The replay doc (`replay-engine.md`) says
the trace observer always asks the stage for records: reported, not re-verified in code.

## 4. The minimal change (verified: 20 replay tests pass, 16 unchanged, 4 new)

1. `claims_language`: when the recorded engine is `knitr`, claim what knitr claims. I moved
   the match into `knitr_claims(language)` (a free function in `engine/knitr/mod.rs`) and call
   it from both engines, so the two cannot drift.
2. `execute`: dispatch to `present_record` when the capture has a record, not only for
   Jupyter; error text names the capture\'s engine.

The old trace, replayed with the patched binary, exits 0 and `replayed2.html` contains `[1] 6`.
The only differences from the live HTML are resource file names (`doc_files/` against
`replayed2_files/`, from my `--output` name). Verified (`i2/replay3.log`, a diff of the two
files).

Scratch tests (in `engine/replay.rs` on the scratch branch): knitr claims `r` Primary(1),
`python` Interop(0), `julia` none; a Jupyter replay still claims only its own name; an old knitr
capture without a record returns its stored markdown; a knitr capture whose `record` was built
with the same builder as the Jupyter test replays through the presenter and the stale stored
markdown is ignored. Caveat: that last test uses a plain stdout stream. It does not exercise
knitr-specific units (fallback units, `text/x-quarto-markdown`), which are KNR-2 Phase 2F.

## 5. Hard-fail policy (verified)

- A trace with no `engine_captures`: exit 1, "Trace file ... has no `engine_captures`.
  Re-record the trace from a real engine run with `trace: true` set." (`i2/replay1.log`).
- A changed input: exit 1, "Execution failed in knitr: replay miss: input QMD does not match
  recorded input (recorded 5006 bytes, got 5014 bytes)" (`i2/replay-miss.log`).
- Both already worked. What did not fail is the unclaimed language (section 2).

## What KNR-6 changes

- **Decision 2 (settled, epic decision 14) holds in effect.** An old knitr trace (markdown
  result, no record) keeps replaying, and it does so by returning the stored markdown verbatim,
  which is what `execute` already does. No presenter is involved, so the claim that it replays
  "through the pass-through presenter" is wrong; that presenter matters only for fallback units
  inside a record. I am flagging this to Gordon because decision 14 is settled; I read it as a
  refinement of the mechanism, not an overturn.
- **The claim fix is a prerequisite, not part of the flip**: every knitr replay test needs it.
- **Trace fields knitr must fill:** `capture.record`, from `ExecuteResult.record` once knitr
  returns one. Nothing else is new (the `engine_name`, `input_qmd` and `result` fields exist).
- Phase 6B gains the dispatch line and the unused-capture check.

## Not done

- A hand-built trace with a knitr-shaped record (fallback units, `quarto_output_anchors`): the
  record shape is I3\'s subject.
- Windows cross-check and wasm32 build.
- How hub consumers read a knitr record (waits for EXR-15).
