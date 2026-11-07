---
title: 'EXR-16 S3: running inline expressions in the Positron editor'
date: 2026-11-02
description: 'A working spike runs each inline span on the document kernel through session.execute in Interactive mode and shows the value in the editor. Interactive is the only mode that returns correct values without console or history side effects; the extension API route has two confirmed gaps.'
---

**Question.** Can Positron run inline expressions such as `` `{python} sum(x)` `` on a
document\'s kernel and show the values inline, and which route should a contribution take:
inside core, or through the extension API with an upstream change?

**Answer.** Yes, inside core, by calling `session.execute` in `Interactive` mode once per span,
in document order. The extension API route (`positron.runtime.executeCode`) cannot give correct
values today, for two reasons confirmed in the source (below). Go for the core route.

**Branch.** `spike/q2-s3-inline-exec` in `/Users/gordon/src/positron-spike-s3`, cut from
`upstream/main` at `d42d28a288`. Pushed to Gordon\'s fork (`origin`) on 2026-11-02. The scanner fix is `6c6fe24ef6` and the value rule
is `acaa4450cd` (see the S4 findings note). Rendering is a Monaco decoration; the example
documents are in `~/docs/inline-output/` (`s3-spans.qmd`, `s3-many-spans.qmd`, `s3-interrupt.qmd`,
`s3-cells-and-spans.qmd`).

## Method

The spike scans the document for spans (`quartoInlineExpr.ts`), sends each as a one-line unit
through the existing Quarto execution queue with `session.execute`, collects every runtime
message with an observer, and renders the value after the span as injected text. The three modes
`Interactive`, `Transient` and `Silent` were each run against the example documents. Gordon
measured the behaviors below in a dev build; the two extension API gaps were read from the
source, not run.

## Measured behavior, by mode

| Mode | Values | Console echo | Console history | Notes |
|---|---|---|---|---|
| `Interactive` | correct | none | no entry | the only mode that works |
| `Transient` | fails on valued expressions | shown (documented) | none | `TokenError: unexpected EOF in multi-line statement, (1, 3)`; spans with no value work |
| `Silent` | every valued span returns undefined | none | none | no `Result` messages at all; errors still arrive |

- The `Transient` failure was not traced. Plain IPython 9.18 on Python 3.12.12 evaluates
  `sum(x)` fine, so the cause is in how Positron wraps the code for that mode.
- `Interactive` deviates from its documentation. The enum comment in
  `src/vs/workbench/services/languageRuntime/common/languageRuntimeService.ts:439-444` says it is
  displayed and stored in history, but called through `session.execute` it did neither in the
  console. The console service probably adds both; this was not traced. The kernel is a
  different matter: the supervisor sends `store_history: true` for `Interactive`
  (`extensions/positron-supervisor/src/KallichoreSession.ts:926-940`), and the counter check
  below shows the kernel does count each span.

## Measured behavior, all modes

- **Completion.** An execution resolves at the `Idle` state message that matches the execution
  id. The `Result` arrives a few milliseconds before `Idle`.
- **Order and state.** Order across spans is correct and kernel state persists, so a span that
  mutates state (`x.append(5)`) makes later values drift between runs. That is the same as
  running a cell, and the note belongs in any user-facing docs.
- **During a running cell.** A span sent while a cell runs waits its turn in the kernel. It was
  not routed through the cell queue, and nothing looked wrong.
- **Interrupt and timeout.** A `KeyboardInterrupt` stops the batch. A 20 second no-`Idle` timeout
  also stops it. **Kernel restart** (run on 2026-11-02 with `~/docs/inline-output/s3-restart.qmd`,
  a span that sleeps 15 seconds, Python): after Gordon confirmed the restart prompt, the span
  immediately showed `KeyboardInterrupt` as its result. So the restart interrupts the running
  code first, the span ends as an interrupt does, and the batch stops. The 20 second timeout was
  not reached, so that path was not exercised here. Not checked: that the kernel then came back
  up, and what a restart does to spans that had not yet run.
- **Cost.** 200 spans take about 1.5 seconds in all, and kernel start dominates.
- **Errors** arrive in every mode, including `Silent`.

## The extension API route: two confirmed gaps

Read from `src/vs/workbench/api/common/positron/extHostLanguageRuntime.ts` and
`src/vs/base/common/async.ts` in the spike checkout.

1. **Only the first `Result` resolves the promise.** `handleObserverMessage` sends a `Result`
   message to `ExecutionObserver.onCompleted` (`extHostLanguageRuntime.ts:2134-2135`), which calls
   `promise.complete(result)` (`:174-179`). `DeferredPromise.complete` returns early when the
   promise is settled (`async.ts:1864-1866`), so a later `Result` is dropped. If no `Result`
   arrives, `onFinished` completes the promise with `{}` at `Idle` (`:147-154`). A value that
   arrives only as `display_data` therefore returns `{}`; the observer sees it only as
   `text/plain` through `onOutput` and as images through `onPlot` (`:100-111`), never as
   `text/html` or `text/markdown`.
2. **`Transient` echoes in the console.** The enum documents it as displayed to the user and
   not stored in history (`languageRuntimeService.ts:451-456`). Only `Silent` is documented as
   hidden (`:458-463`), and it returns no values, as measured.

Both gaps mean an extension cannot read an inline value correctly or quietly. A contribution
through the extension API would need Positron to resolve with the full message list or the last
output bundle, and a hidden mode that still returns values.

## Kernel state: spans count as executions

On a fresh Python kernel, a cell that prints `get_ipython().execution_count` was run, then three
spans in `Interactive` mode, then the cell again. The second run read 5, as predicted for a
first run of 1 (the three spans plus the cell itself). So each span is one execution on the
kernel: it advances the counter and goes into the kernel\'s own history, even though the
console shows nothing. The "no entry" in the table is Positron\'s console history only. Any
later cell that shows its execution number, or reads `Out` or `.Last.value`, sees the spans.
Document: `~/docs/inline-output/s5-exec-count.qmd`.

## R works through the same route

A short R document (`~/docs/inline-output/s5-r-spans.qmd`: sum, mean, a string, a logical, a
vector, an error, and a Python span that must be ignored) ran in `Interactive` mode against
Ark and showed correct values. The route does not depend on the language, and the
Python-only caveat below no longer applies to the basic case.

## What Quarto 1 does

Quarto 1 does not run spans as code. `cell_execute_inline` (`notebook.py:636-687`) sends one
empty execute request with all expressions in `user_expressions`; the kernel evaluates them and
returns one mime bundle per expression in the reply. This avoids both gaps (no `Result` or
`display_data` messages, no echo, no history entry) and is what spike S5 (Ark and
`user_expressions`) would test. The core route above is the one reachable today, because it needs
only existing calls. `user_expressions` is not reachable from the workbench: `session.execute`
takes `(code, id, mode, errorBehavior)`, and the supervisor hard-codes `user_expressions: {}`
in the request (`KallichoreSession.ts:937`). A contribution that used it would need a new API
through the workbench, the extension host and the supervisor, and for R an implementation in
Ark (the stub at `console_repl.rs:2215`). Its advantages are that it rests on documented
protocol behavior rather than on `Interactive`\'s undocumented quiet mode, that results come
back per expression, and that, by the protocol, it leaves the execution counter alone. Spike S5
was not run (see the plan).

## Findings to carry forward

- **The decision for a Positron contribution.** Running each span through `session.execute`
  works for Python and R, but every span counts as an execution on the kernel (see "Kernel
  state" above). The cleaner route, `user_expressions`, leaves the counter and the kernel
  history alone, but it needs a new API through the workbench, the extension host and the
  supervisor, and for R an implementation in Ark. Whoever builds inline execution has to
  choose between shipping the reachable route with that side effect and building the API
  first. This spike does not make that choice.
- **Escaped backticks.** The scanner treated `` \`{python} 1\` `` as a span. Fixed in
  `6c6fe24ef6` (a backslash-escaped opener is literal; only the opener is checked). This is
  stricter than Quarto 1, whose regex (`notebook.py:654-656`) only rejects an opener that
  follows another backtick.
- **`HTML()` values.** The value is `<IPython.core.display.HTML object>` under a rule that reads
  only `text/plain`. Resolved by S4: the rule is chosen per output target, as Quarto 1 does.
  For the editor decoration the repr is the best text available; that is a known limit.
- **Decorations belong to the editor widget.** Monaco clears them when that widget switches
  model, so opening a report in the same editor group wiped them. A decoration needs a
  non-empty range, and a class name with no style also worked; the combination of an empty
  range and no class did not render.

## Not tested

The timeout path (nothing in these runs triggered it); kernels other than Python and R; documents with hundreds of cells; `Unprocessed`
and `NonInteractive` modes; any user-facing setting; why `Transient` fails; where `Interactive`
suppresses echo and history. The spike also has a few lint warnings (`in` operator, single-quoted
strings) that were left alone.
