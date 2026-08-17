# Pandoc reference semantics for filter-script return values

Probes run against **pandoc 3.9.0.2** (the version pinned in
`crates/pampa/tests/lua-conformance/differential/ORACLE_VERSION`, so these
results can be turned into differential cases directly).

Input for every probe is `input.md`:

```
Marker: MARKER
```

Command: `pandoc -f markdown input.md -L <probe>.lua -t plain`

`./run-parity-matrix.sh` runs every probe through **both** pandoc and pampa and
prints the two columns side by side. It takes `PAMPA=<path>` to point at a
patched build, so the same script that documents the bug also checks the fix.
The q2 column as it stands today is in `../repro/OBSERVED-AT-HEAD.md`.

| Probe | Script shape | Pandoc output | Semantics |
| --- | --- | --- | --- |
| `tf.lua` | `return { Str = f }` | `Marker: TABLE-FORM-RAN` | Returned table is the filter. |
| `lf.lua` | `return { {Str=f}, {Str=g} }` | `Marker: LIST-FORM-RAN` | Array part → ordered list of passes, applied in order (`g` sees `f`'s output). |
| `mixed.lua` | global `Str` **and** `return { Str = … }` | `Marker: TABLE-RAN` | **Returned table wins; globals are ignored entirely.** |
| `empty.lua` | global `Str` and `return {}` | `Marker: MARKER` (unchanged) | An *empty* returned table still wins — no fallback to globals. |
| `emptylist.lua` | global `Str` and `return { {} }` | `Marker: MARKER` (unchanged) | Same, for a one-element list of an empty filter. |
| `hybrid.lua` | `return { Str = f, {Str = g} }` | `Marker: ARRAY-RAN` | Disambiguation is by array length: a non-empty array part makes it a *list*, and the named keys are ignored. |
| `trav.lua` | `return { traverse = 'topdown', Para = …, Str = … }` | `STR-TOPDOWN-PARA` | `traverse` is read off the returned table. |
| `listtrav.lua` | list whose first entry sets `traverse = 'topdown'` | `S-P1` | `traverse` is **per-entry**, not global to the list. |
| `nilret.lua` | global `Str` and explicit `return nil` | **error:** `attempt to index a nil value` | Pandoc counts stack values, not nil-ness — an explicit `return nil` is an error, *not* a fallback to globals. |
| `num.lua` | `return 5` | **error:** `attempt to index a number value` | Non-table returns are errors. |
| `fnret.lua` | `return function(x) … end` | **error:** `attempt to index a function value` | Ditto for a returned function. |

## The rule, stated once

1. If the script returns **no value at all** (falls off the end), the filter is
   built from the **globals**.
2. If it returns **any value**, that value is the filter and globals are never
   consulted:
   - `rawlen(t) == 0` → a single filter table;
   - `rawlen(t) > 0` → a list of filter tables, applied as successive passes;
   - anything not a table → a load-time error.

Pandoc's own error messages for case 2's failure mode are poor (`attempt to
index a nil value` names nothing). q2 can be strictly better here without
diverging on behavior — see the design questions in the plan.
