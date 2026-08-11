# Observed re-running the repro at `main` @ `808215fc` (q2 0.17.0)

```
$ cd claude-notes/plans/2026-08-11-lua-filter-returned-table-form-investigation/repro
$ q2 render
Rendering project: …/repro (type: website)
Rendered 4 of 4 files to …/repro/_site
```

Markers grepped out of the rendered HTML:

| document | filter form | rendered marker | verdict |
| --- | --- | --- | --- |
| `index.html` | `return { Str = … }` | `Marker: MARKER` | **filter did not run** |
| `list-form.html` | `return { {Str=…}, {Str=…} }` | `Marker: MARKER` | **filter did not run** |
| `control.html` | `function Str(el)` | `Marker: FUNCTION-FORM-RAN` | works |
| `walk-form.html` | global `Pandoc` + `doc:walk{…}` | `Marker: WALK-TABLE-RAN` | works |

**Zero diagnostics.** The render exits 0 and prints nothing beyond the two
progress lines — no warning, no note, nothing that distinguishes "your filter
ran and matched nothing" from "your filter was never consulted." This is the
part of the bug the strand calls the worst available failure mode, and it
reproduces exactly.

## Full parity matrix (q2 today vs pandoc 3.9.0.2)

Reproduce the whole table in one command:

```
$ ../pandoc-probes/run-parity-matrix.sh
```

It runs each probe through both engines the way the differential suite does —
`pampa input.md -F <probe>.lua -t plain` against
`pandoc -f markdown input.md -L <probe>.lua -t plain` — and takes `PAMPA=…` to
point at a patched build, so it doubles as the fix's smoke check.

Output at `808215fc` / pandoc 3.9.0.2:

| probe | script shape | pandoc | q2 | |
| --- | --- | --- | --- | --- |
| `tf` | `return {Str=f}` | `TABLE-FORM-RAN` | `MARKER` | ✗ reported bug |
| `lf` | `return {{Str=f},{Str=g}}` | `LIST-FORM-RAN` | `MARKER` | ✗ reported bug |
| `hybrid` | `return {Str=f, {Str=g}}` | `ARRAY-RAN` | `MARKER` | ✗ |
| `trav` | `return {traverse='topdown', …}` | `STR-TOPDOWN-PARA` | `MARKER` | ✗ |
| `listtrav` | list, per-entry `traverse` | `S-P1` | `MARKER` | ✗ |
| `fnret` | `return function(x) … end` | **error** | `MARKER` | ✗ (silent) |
| `mixed` | global `Str` + `return {Str=g}` | `TABLE-RAN` | `GLOBAL-RAN` | ⚠ **flips** |
| `empty` | global `Str` + `return {}` | `MARKER` (nothing runs) | `GLOBAL-RAN` | ⚠ **flips** |
| `emptylist` | global `Str` + `return { {} }` | `MARKER` (nothing runs) | `GLOBAL-RAN` | ⚠ **flips** |
| `nilret` | global `Str` + `return nil` | **error** | `GLOBAL-RAN` | ⚠ **flips** |
| `num` | global `Str` + `return 5` | **error** | `GLOBAL-RAN` | ⚠ **flips** |

The ✗ rows are the bug as filed: q2 ignores the returned value entirely.

The ⚠ rows are why design question 1 in the plan is not rhetorical. Every one
of them is a filter that **works today** and would **change behavior** under
strict pandoc parity — the last two by starting to error. They are all odd
filters (why return a value *and* define globals?), but "odd" is not
"nonexistent", and the flip would be as silent for them as the original bug is
today.

---

## After the fix (2026-08-11)

Same command, same four documents, on the patched tree:

```
$ q2 render
Rendered 4 of 4 files to …/repro/_site

index.html       Marker: TABLE-FORM-RAN      <- was MARKER (ignored)
list-form.html   Marker: LIST-FORM-RAN       <- was MARKER (ignored)
control.html     Marker: FUNCTION-FORM-RAN   <- unchanged
walk-form.html   Marker: WALK-TABLE-RAN      <- unchanged
```

Both previously-silent forms now run, and neither of the two that already
worked regressed. The rendered HTML was inspected directly (grepped for the
marker), not inferred from the exit code.

`../pandoc-probes/run-parity-matrix.sh` against the same build agrees with
pandoc on every AST-shaped row; the three error rows now error in q2 too, with
a `Q-11-6` message naming the file and the offending type instead of pandoc's
`attempt to index a number value`.
