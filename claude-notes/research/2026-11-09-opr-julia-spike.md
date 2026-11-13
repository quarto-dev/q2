---
title: 'OPR-4a: can the Julia runner supply expression starts, anchors and output kinds?'
date: 2026-11-09
description: 'Spike verdict. QuartoNotebookRunner can supply expression starts and output anchors with a small, cheap change; it can tell a message from a warning with a logger wrapper. Julia cells have never interleaved in Quarto 1, so this is new behavior, not a restoration.'
---

**Plan:** [OPR-4a](../plans/2026-11-08-opr-4a-julia-spike.md) · **Epic:** [one presenter path](../plans/2026-11-07-one-presenter-path-epic.md)

## Verdict

| Fact | Verdict | Effort |
|---|---|---|
| Expression starts | **Feasible** with a runner change (about 10 lines in `include_str`, plus wire) | 1 session, with runner tests |
| Anchors (which expression produced each output) | **Feasible** with a runner change (tag displays and stream runs) | same session |
| Output kinds (message, warning) | **Feasible** with a runner change (a logger wrapper); only for `@info`/`@warn`/`@error`, not for raw `stderr` writes | same session |

The runner is the right place: only it sees expression boundaries, and one change covers all three. No q2 change is
needed beyond reading three optional keys. **Recommendation: build all three together (OPR-4b), in one runner
change.** None is separable at a useful cost: starts and anchors share the same plumbing, and kinds add about 25 lines.

Julia shows only the *last* expression's value (REPL semantics), so there is no per-expression value to anchor;
knitr prints every top-level value, so knitr cells and Julia cells will never interleave identically. Say so in the
docs (OPR-4d).

## Quarto 1 comparison (ran)

`quarto render` (a Quarto 1 dev checkout, version 99.9.9, `~/bin/quarto`) and `q2 render` (OPR-3 build) of one cell:

```julia
println("hello"); x = 1 + 1; @warn "careful"; @info "fyi"; x * 10
```

Both print the whole source block first, then `hello`, then one stderr block holding the warning and the info
together, then `20`. **Neither interleaves.** Interleaving for Julia is therefore new behavior, not a restoration of
Quarto 1. Also seen: the two engines produced identical stderr text, including the `@ Main.Notebook ...qmd:11` location line.

## What the runner does today (read)

Runner at `external-sources/QuartoNotebookRunner.jl` (branch `feat/quarto2-handled-languages`, `5aa4bd7`; q2 pins
`5e04284`, which is an ancestor).

- `QuartoNotebookWorker/src/render_code.jl`, `include_str`: parses the cell with `Meta.parseall`, which puts a
  `LineNumberNode` before each top-level expression, and evals them one at a time. The loop already tracks `loc`, but
  returns only the last value. **(read; the starting line of each expression is already available.)**
- `render_io.jl`, `_record!`/`StreamRuns`: a stream write is recorded synchronously as Julia writes it, as a run
  `(stream, number of displays before)`. Order across stdout, stderr and displays is therefore exact for Julia-level
  writes. **(read, and ran: the order below matches the program order.)**
- `InlineDisplay.jl`: `display` calls are queued in order; nothing says which expression queued them.
- `evaluation.jl`, `_format_worker_outputs`: merges streams and displays into the Jupyter `outputs`, by display count.
  The cell's `metadata.quarto` (with `emitUnits`) holds `unit`, `language`, `file` and one `line` per cell.
- Messages and warnings both go through `Logging.ConsoleLogger(context_stderr)`, so both are plain stderr text. **(read)**

All the plan's "What is known" claims check out against the source.

## Prototype (ran)

A scratch `git worktree` of the runner (branch `spike/opr-4a-expressions`, outside q2; diff below). Changes:

1. `InlineDisplay` gets `current` (index of the running expression), a parallel `expressions` vector for queued displays, and
   `kind` (the level of the log record being written).
2. `include_str` records `loc.line - first_code_line` per expression and sets `current` before each eval.
3. Stream runs are keyed by `(stream, displays, expression, kind)`, so a warning, an info and a raw stderr write
   stay separate runs. A `KindLogger` wraps the `ConsoleLogger` and sets `kind` for exactly the one write that
   `handle_message` makes.
4. The wire grows: `CellResult.streams` tuples gain `(expression, kind)`; `CellResult` gains `expression_starts` and
   `display_expressions`; `PROTOCOL_VERSION` 8 to 9.
5. `evaluation.jl` emits, into `metadata.quarto` (only with `emitUnits`): `expression_starts`, `output_expressions`,
   `output_anchors` (lines before each output: the next expression's start, or the line count for the last) and `output_kinds`.

### Demonstration

A twelve-expression cell (`println`, a value, `@warn`, a custom `display` that emits `image/svg+xml` as a plot does, `@info`, a
raw `stderr` write, a `print`, a final value), each expression on its own line:

```
expression_starts=[0,1,2,3,4,5,6,7,8,9,10,11]
output_expressions=[1,4,8,9,10,11,12]
output_anchors    =[1,4,8,9,10,11,12]
output_kinds      =[null,"warning",null,"message",null,null,null]
  stream stdout "hello"            (expression 1)
  stream stderr "Warning: careful" (expression 4, kind warning)
  display_data  image/svg+xml      (expression 8)
  stream stderr "Info: fyi"        (expression 9, kind message)
  stream stderr "raw stderr"       (expression 10, kind null, not merged with the info)
  stream stdout "tail "            (expression 11)
  execute_result 20                (expression 12)
```

A harder cell (`#|` options, a comment, a multi-line `function` and a multi-line array, a blank line):
`expression_starts=[1,8,10,11,12,13,14,16]` against 17 lines of code after the options, which is right (the
function starts on line 1 after a comment line; `x = [1, 2,` starts at 14 and spans two lines). The loop's
two `println`s are attributed to expression 2 (`f(2)`, line 8) with anchor 10, the next expression's start. A subprocess
(`run(`echo ...`)`) is attributed to its own expression. A `@async` print that lands while a later
`sleep` is running is attributed to the expression running at that time (the only possible rule).

### Timing (ran)

A cell of 1200 expressions with 600 `@info` and 600 `display` calls, five cache-busted runs on a warm server:
baseline median 4.22 s, prototype median 4.10 s. No measurable overhead (noise is about 0.5 s; the logging
dominates). The only cost is per output, not per character.

### Message against warning (ran)

`@info`, `@warn` and `@error` are told apart by level: `@info` and below map to `"message"`, `@warn` to `"warning"`,
`@error` to `"error"`. Order against stdout is kept (the demonstration above). What it cannot do:

- A raw `println(stderr, ...)` has no level: kind null. In knitr everything on stderr has a class, so q2 must treat
  null as "stderr, neither": show it unless the whole `stderr` is hidden. Decide the mapping in OPR-4b.
- The runner's existing `warning: false` drops **every** stderr stream at capture time (`_tidy_streams`), including
  `@info` and raw stderr. See "Findings" below.

## Proposed wire (not implemented)

Runner to engine: the cell's `metadata.quarto` gains optional keys, written only when the engine asked for units
(`emitUnits`, so default notebooks stay byte-identical; the prototype splits stream runs unconditionally, which
OPR-4b should gate on the same option):

| `metadata.quarto` key | Shape | q2 cell-metadata key it becomes |
|---|---|---|
| `expression_starts` | `[int]`, 0-based line within the code after the `#|` options | `quarto_expression_starts` |
| `output_anchors` | `[int]`, parallel to `outputs` | `quarto_output_anchors` |
| `output_kinds` | `[null or "message" or "warning" or "error"]`, parallel to `outputs` | `quarto_output_kinds` |
| `output_expressions` | `[int]`, 1-based, parallel to `outputs` | not needed by q2; kept for debugging |

`ts_protocol.rs` needs **no change**: these ride inside the notebook cells that `read_engine_notebook` already
reads, and the record format is unchanged. What changes in q2 is the mapper that turns Julia cells into presenter
inputs (OPR-4d; OPR-4b is the runner, OPR-4c the pin and the proof that the keys arrive): copy the three keys under the knitr names (see `presenter/knitr_inputs.rs`, `EXPRESSION_STARTS_KEY`
and the two beside it). The Julia engine extension is expected to need only a dependency bump of the runner pin (read, not run through q2; OPR-4c confirms)
(`resources/extension-subtrees/julia-engine`), and `RecordInfo.version` may stay 1 because the keys are optional.
The runner's own protocol version (worker IPC, 8 to 9) is internal.

## Findings outside the spike's question (not fixed here)

1. **`warning` is declared display-only for Julia (OPR-3) but the runner still enforces it at capture.**
   With `#| warning: false` the runner drops all stderr, so a record taken with `warning: false` has no stderr,
   and flipping to `warning: true` is, by the hash rule, not a change that invalidates the record: the presenter
   would show nothing. This is the inverse of D2's assumption ("when it is in the hash, capture enforced it").
   Seen: the `edge.qmd` cell with `warning: false` lost `@warn` **and** `@info`. I did not render this through q2, so the end-to-end
   effect is unconfirmed. Fix in OPR-4b (decision recorded in OPR-4b and the epic): have the runner stop dropping stderr when `emitUnits` is set (the presenter
   enforces it, using kinds), or remove `warning` from Julia's display-only list.
2. Output written by C code through libc stdio (`puts`) is lost, before and after the prototype, because libc
   buffers it when stdout is a pipe and nothing flushes it. Unrelated to attribution; pre-existing.
3. For a cell that throws, the anchor of the throwing expression is the end of the code, not the start of the
   next (never-run) expression. Harmless for placement; mention in OPR-4b tests.

## Effect on OPR-4b to OPR-4d

All three facts are feasible, so the verdict is not partial. Suggested scope for Gordon: OPR-4b as written (runner
change, mapper, parity), with the stderr-at-capture fix from Finding 1 folded in. The runner change lives in the
Julia fork and is a Gordon-side companion push, as for OPR-3.

## What I did and did not do

- Ran: the prototype on four cells, timing, the Quarto 1 and q2 renders (isolated server, scratch HOME,
  baseline runner project), the baseline `puts` check.
- Read only: q2's `ts_protocol.rs`, `record-wire.ts`, `knitr/pieces_map.rs` and the presenter key names.
- Not done: the runner's own test suite (the prototype changes the `streams` tuple shape, so
  `QuartoNotebookWorker/test/streams_test.jl` and `protocol_test.jl` need updating in OPR-4b); a Plots.jl figure (a
  custom `display` stand-in goes through the same `InlineDisplay` path); a q2 render of the prototype runner.

## Appendix: prototype diff

Against runner `5aa4bd7`. The scratch worktree is not part of this repository.

```diff
diff --git a/src/QuartoNotebookWorker/src/InlineDisplay.jl b/src/QuartoNotebookWorker/src/InlineDisplay.jl
index 6e72e21..a398b2b 100644
--- a/src/QuartoNotebookWorker/src/InlineDisplay.jl
+++ b/src/QuartoNotebookWorker/src/InlineDisplay.jl
@@ -6,12 +6,18 @@
 # `render_mimetypes` function as "normal" cell output does.
 struct InlineDisplay <: AbstractDisplay
     queue::Vector{Any}
+    # Spike (OPR-4a): the 1-based index of the top-level expression running when each
+    # queued display arrived; `current` is set by `include_str` between expressions and
+    # `kind` by the logger wrapper while one log record is written.
+    expressions::Vector{Int}
+    current::Base.RefValue{Int}
+    kind::Base.RefValue{String}
     cell_options::Dict
     # Shared with the cell's output capture, which reads the queue's length.
     lock::ReentrantLock
 
     function InlineDisplay(cell_options::Dict)
-        new(Any[], cell_options, ReentrantLock())
+        new(Any[], Int[], Ref(0), Ref(""), cell_options, ReentrantLock())
     end
 end
 
@@ -25,7 +31,10 @@ end
 Base.displayable(::InlineDisplay, m::MIME) = true
 
 function _enqueue!(d::InlineDisplay, result)
-    lock(() -> push!(d.queue, result), d.lock)
+    lock(d.lock) do
+        push!(d.queue, result)
+        push!(d.expressions, d.current[])
+    end
     return nothing
 end
 
@@ -35,7 +44,7 @@ function with_inline_display(f, cell_options)
     inline_display = InlineDisplay(cell_options)
     pushdisplay(inline_display)
     try
-        return f(inline_display), inline_display.queue
+        return f(inline_display), inline_display.queue, inline_display.expressions
     finally
         popdisplay(inline_display)
     end
diff --git a/src/QuartoNotebookWorker/src/protocol.jl b/src/QuartoNotebookWorker/src/protocol.jl
index 0d7f227..da38114 100644
--- a/src/QuartoNotebookWorker/src/protocol.jl
+++ b/src/QuartoNotebookWorker/src/protocol.jl
@@ -15,7 +15,7 @@ Base.lock(f, lio::LockableIO) =
 
 const MsgID = UInt64
 const MAGIC = UInt32(0x514E5257)  # "QNRW"
-const PROTOCOL_VERSION = UInt8(8)  # Place cell streams relative to display results
+const PROTOCOL_VERSION = UInt8(9)  # Place cell streams relative to display results
 
 module MsgType
 const CALL = 0x01       # expects response
@@ -118,9 +118,15 @@ struct CellResult
     results::Dict{String,MimeResult}
     display_results::Vector{Dict{String,MimeResult}}
     # Stream name, text, and how many of `display_results` came before it.
-    streams::Vector{Tuple{String,String,Int}}
+    # Spike (OPR-4a): stream name, text, displays before, 1-based top-level expression
+    # that wrote it (0 = none), and the log kind ("message", "warning", "error", or "").
+    streams::Vector{Tuple{String,String,Int,Int,String}}
     error::Union{Nothing,String}
     backtrace::Vector{String}
+    # Line (0-based, from the first line of code) where each top-level expression starts.
+    expression_starts::Vector{Int}
+    # The expression each of `display_results` came from.
+    display_expressions::Vector{Int}
 end
 
 struct RenderResponse
@@ -272,6 +278,8 @@ function _serialize(io::IO, x::CellResult)
     _serialize(io, x.streams)
     _serialize(io, x.error)
     _serialize(io, x.backtrace)
+    _serialize(io, x.expression_starts)
+    _serialize(io, x.display_expressions)
 end
 
 function _serialize(io::IO, x::RenderResponse)
@@ -369,6 +377,8 @@ function _deserialize(io::IO)
             _deserialize(io),
             _deserialize(io),
             _deserialize(io),
+            _deserialize(io),
+            _deserialize(io),
         )
     elseif tag == TAG_RENDERRESPONSE
         RenderResponse(_deserialize(io), _deserialize(io))
diff --git a/src/QuartoNotebookWorker/src/render.jl b/src/QuartoNotebookWorker/src/render.jl
index 0370916..4683b8a 100644
--- a/src/QuartoNotebookWorker/src/render.jl
+++ b/src/QuartoNotebookWorker/src/render.jl
@@ -50,7 +50,7 @@ function _render_thunk(
     is_expansion_ref::Ref{Bool} = Ref(false);
     inline::Bool,
 )
-    captured, display_results = NotebookState.with_cell_options(cell_options) do
+    captured, display_results, display_expressions = NotebookState.with_cell_options(cell_options) do
         with_inline_display(thunk, cell_options)
     end
 
@@ -75,6 +75,8 @@ function _render_thunk(
                     captured.streams,
                     string(typeof(error)),
                     collect(eachline(IOBuffer(clean_bt_str(true, backtrace, error, mod)))),
+                    get(captured, :expression_starts, Int[]),
+                    display_expressions,
                 ),
             )
         end
@@ -135,6 +137,8 @@ function _render_thunk(
                         ),
                     ),
                 ),
+                get(captured, :expression_starts, Int[]),
+                display_expressions,
             ),
         )
     end
diff --git a/src/QuartoNotebookWorker/src/render_code.jl b/src/QuartoNotebookWorker/src/render_code.jl
index a8bb2b6..e38de5e 100644
--- a/src/QuartoNotebookWorker/src/render_code.jl
+++ b/src/QuartoNotebookWorker/src/render_code.jl
@@ -87,10 +87,13 @@ function include_str(
     inline_display::InlineDisplay,
 )
     loc = LineNumberNode(line, Symbol(file))
+    starts = Int[]
     try
         ast = _process_code(mod, code; filename = file, lineno = line)
         @assert Meta.isexpr(ast, :toplevel)
-        return io_capture(;
+        # Lines are counted from the first line of code, after the `#|` options.
+        first_line = line + last(_drop_cell_options(code))
+        captured = io_capture(;
             cell_options = cell_options,
             inline_display,
             rethrow = InterruptException,
@@ -106,6 +109,8 @@ function include_str(
                         line_and_ex.args[1] = ex
                         continue
                     end
+                    push!(starts, loc.line - first_line)
+                    inline_display.current[] = length(starts)
                     # Wrap things to be eval'd in a :toplevel expr to carry line
                     # information as part of the expr.
                     line_and_ex.args[2] = ex
@@ -122,11 +127,12 @@ function include_str(
             end
             return result
         end
+        return merge(captured, (; expression_starts = starts))
     catch err
         if err isa Base.Meta.ParseError
             return (;
                 result = err,
-                streams = Tuple{String,String,Int}[],
+                streams = Tuple{String,String,Int,Int,String}[],
                 error = true,
                 backtrace = catch_backtrace(),
             )
diff --git a/src/QuartoNotebookWorker/src/render_io.jl b/src/QuartoNotebookWorker/src/render_io.jl
index 4d2df82..6d9b167 100644
--- a/src/QuartoNotebookWorker/src/render_io.jl
+++ b/src/QuartoNotebookWorker/src/render_io.jl
@@ -17,16 +17,16 @@ end
 # leave the `stdout` runs either side of each one as separate blocks, so the
 # worker drops those streams itself and joins the runs that remain.
 function _tidy_streams(streams, extensions, warning::Bool)
-    tidied = Tuple{String,String,Int}[]
-    for (name, text, displays) in streams
+    tidied = Tuple{String,String,Int,Int,String}[]
+    for (name, text, displays, expression, kind) in streams
         warning || name == "stdout" || continue
         text = strip_worker_precompilation(text, extensions)
         isempty(text) && continue
         previous = isempty(tidied) ? nothing : tidied[end]
-        if previous !== nothing && previous[1] == name && previous[3] == displays
-            tidied[end] = (name, previous[2] * text, displays)
+        if previous !== nothing && previous[[1, 3, 4, 5]] == (name, displays, expression, kind)
+            tidied[end] = (name, previous[2] * text, displays, expression, kind)
         else
-            push!(tidied, (name, text, displays))
+            push!(tidied, (name, text, displays, expression, kind))
         end
     end
     return tidied
@@ -37,23 +37,28 @@ end
 # before it, so a display between two writes to one stream splits the run. The
 # display's lock guards the runs too, so the count and the queue agree.
 struct StreamRuns
-    runs::Vector{Tuple{String,Int,IOBuffer}}
+    runs::Vector{Tuple{String,Int,Int,String,IOBuffer}}
     inline_display::InlineDisplay
 end
-StreamRuns(inline_display) = StreamRuns(Tuple{String,Int,IOBuffer}[], inline_display)
+StreamRuns(inline_display) =
+    StreamRuns(Tuple{String,Int,Int,String,IOBuffer}[], inline_display)
 
 function _record!(write_bytes, runs::StreamRuns, name::String)
     lock(runs.inline_display.lock) do
-        run = (name, length(runs.inline_display.queue))
-        if isempty(runs.runs) || runs.runs[end][1:2] != run
+        # stdout never carries a kind; only a log record written to stderr does.
+        kind = name == "stderr" ? runs.inline_display.kind[] : ""
+        run = (name, length(runs.inline_display.queue), runs.inline_display.current[], kind)
+        if isempty(runs.runs) || runs.runs[end][1:4] != run
             push!(runs.runs, (run..., IOBuffer()))
         end
-        write_bytes(runs.runs[end][3])
+        write_bytes(runs.runs[end][5])
     end
 end
 
-_streams(runs::StreamRuns) =
-    [(name, String(take!(buffer)), displays) for (name, displays, buffer) in runs.runs]
+_streams(runs::StreamRuns) = [
+    (name, String(take!(buffer)), displays, expression, kind) for
+    (name, displays, expression, kind, buffer) in runs.runs
+]
 
 # One of the streams a cell writes to. A write from Julia is recorded as it
 # happens, which is what keeps the order across streams. The file descriptor
@@ -108,6 +113,32 @@ end
     Base.redirect_stderr(stream::CapturedStream) = redirect_stderr(Base.pipe_writer(stream))
 end
 
+# Spike (OPR-4a): tags each log record's stderr write with its level, so a message can be
+# told from a warning. `ConsoleLogger` writes a record with one write to its stream,
+# inside `handle_message`, so a Ref set around the call attributes it exactly.
+struct KindLogger <: Logging.AbstractLogger
+    inner::Logging.ConsoleLogger
+    inline_display::InlineDisplay
+end
+Logging.min_enabled_level(l::KindLogger) = Logging.min_enabled_level(l.inner)
+Logging.shouldlog(l::KindLogger, args...) = Logging.shouldlog(l.inner, args...)
+Logging.catch_exceptions(l::KindLogger) = Logging.catch_exceptions(l.inner)
+function Logging.handle_message(l::KindLogger, level, args...; kws...)
+    kind =
+        level >= Logging.Error ? "error" :
+        level >= Logging.Warn ? "warning" : "message"
+    lock(l.inline_display.lock) do
+        l.inline_display.kind[] = kind
+    end
+    try
+        Logging.handle_message(l.inner, level, args...; kws...)
+    finally
+        lock(l.inline_display.lock) do
+            l.inline_display.kind[] = ""
+        end
+    end
+end
+
 # Capture what the cell writes to `stdout`, `stderr` and the logger. The readers
 # stop once the cell's code has returned rather than once the pipes reach their
 # end: a process the cell started and left running holds a copy of the write
@@ -132,7 +163,7 @@ function _capture(
     context_stderr = IOContext(captured_stderr, :color => colored, io_context...)
     redirect_stdout(context_stdout)
     redirect_stderr(context_stderr)
-    logger = Logging.ConsoleLogger(context_stderr)
+    logger = KindLogger(Logging.ConsoleLogger(context_stderr), inline_display)
 
     # A new task seeds its rng from the current task, which moves the stream the
     # cell's own code draws from, so put the seed back afterwards.
diff --git a/src/evaluation.jl b/src/evaluation.jl
index 733741b..a838ca4 100644
--- a/src/evaluation.jl
+++ b/src/evaluation.jl
@@ -405,7 +405,9 @@ function _evaluate_code_cell!(
 
     for (mth, remote) in enumerate(worker_results)
         allow_error_cell = get(remote.cell_options, "error", allow_error_global)
-        outputs = _format_worker_outputs(remote, allow_error_cell, record_error!, chunk)
+        outputs, out_expressions, out_kinds =
+            _format_worker_outputs(remote, allow_error_cell, record_error!, chunk)
+        trace = quarto === nothing ? nothing : _trace_metadata(remote, chunk, out_expressions, out_kinds)
 
         cell_options = expand_cell ? remote.cell_options : Dict()
 
@@ -435,8 +437,8 @@ function _evaluate_code_cell!(
             (;
                 id = expand_cell ? string(nth, "_", mth) : string(nth),
                 cell_type = chunk.type,
-                metadata = expand_cell ? _cell_metadata(quarto; expansion = mth) :
-                           _cell_metadata(quarto),
+                metadata = expand_cell ? _cell_metadata(quarto; expansion = mth, (trace === nothing ? (;) : trace)...) :
+                           _cell_metadata(quarto; (trace === nothing ? (;) : trace)...),
                 source = process_cell_source(source, cell_options),
                 outputs,
                 execution_count = 1,
@@ -453,21 +455,34 @@ execute results, evaluation errors, output streams, and show errors.
 """
 function _format_worker_outputs(remote, allow_error_cell, record_error!, chunk)
     outputs = []
+    # Spike (OPR-4a): which expression and which log kind produced each output.
+    out_expressions = Int[]
+    out_kinds = Union{Nothing,String}[]
+    nexpr = length(remote.expression_starts)
+    tag!(expression, kind = nothing) =
+        while length(out_expressions) < length(outputs)
+            push!(out_expressions, expression)
+            push!(out_kinds, kind)
+        end
     processed = process_results(remote.results)
 
     # Streams and displays go out in the order the cell wrote them. A cell that
     # throws keeps its streams but not its displays.
     displays = isnothing(remote.error) ? remote.display_results : []
     shown = 0
-    for (name, text, displays_before) in remote.streams
+    for (name, text, displays_before, expression, kind) in remote.streams
         while shown < min(displays_before, length(displays))
             shown += 1
             _push_display!(outputs, displays[shown], allow_error_cell, record_error!, chunk)
+            tag!(remote.display_expressions[shown])
         end
         push!(outputs, (; output_type = "stream", name, text))
+        tag!(expression, isempty(kind) ? nothing : kind)
     end
     for display_result in displays[shown+1:end]
         _push_display!(outputs, display_result, allow_error_cell, record_error!, chunk)
+        shown += 1
+        tag!(remote.display_expressions[shown])
     end
 
     if isnothing(remote.error)
@@ -505,8 +520,30 @@ function _format_worker_outputs(remote, allow_error_cell, record_error!, chunk)
     # Errors from the `show` output of values, not from cell evaluation.
     # If something throws here the user's `show` method has a bug.
     _push_show_errors!(outputs, processed.errors, allow_error_cell, record_error!, chunk)
+    tag!(nexpr)
 
-    return outputs
+    return outputs, out_expressions, out_kinds
+end
+
+# Lines of code after the leading `#|` options and blank lines, as the worker counts them.
+function _code_line_count(source::AbstractString)
+    lines = collect(eachline(IOBuffer(source)))
+    dropped = something(findfirst(l -> !(startswith(l, "#|") || isempty(strip(l))), lines), length(lines) + 1) - 1
+    return length(lines) - dropped
+end
+
+# `output_anchors`: the source lines that precede each output, i.e. up to the start of the
+# next expression (or the end of the code, for the last).
+function _trace_metadata(remote, chunk, out_expressions, out_kinds)
+    starts = remote.expression_starts
+    total = _code_line_count(chunk.source)
+    anchor(e) = e == 0 ? 0 : e < length(starts) ? starts[e+1] : total
+    return (;
+        expression_starts = starts,
+        output_expressions = out_expressions,
+        output_anchors = [anchor(e) for e in out_expressions],
+        output_kinds = out_kinds,
+    )
 end
 
 function _push_display!(outputs, display_result, allow_error_cell, record_error!, chunk)
diff --git a/src/precompile.jl b/src/precompile.jl
index a35932f..290b0b5 100644
--- a/src/precompile.jl
+++ b/src/precompile.jl
@@ -45,9 +45,11 @@ PrecompileTools.@setup_workload begin
                             WorkerIPC.MimeResult("text/plain", false, UInt8[0x31]),
                     ),
                     WorkerIPC.MimeResult[],
-                    [("stdout", "1\n", 0)],
+                    [("stdout", "1\n", 0, 1, "")],
                     nothing,
                     String[],
+                    [0],
+                    Int[],
                 ),
             ],
             false,
```
