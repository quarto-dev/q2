# Route structured diagnostics through discovery errors (bd-y56u1gl7)

**Strand:** bd-y56u1gl7 (discovered-from bd-sekn481x)
**Status:** done — all gates green (2026-08-08; workspace suite
11075 passed, `cargo xtask verify --skip-hub-build` passed)

## Situation

All project-discovery failures in `q2 render` are flattened to a
string at nine `.map_err(|e| DispatchError::Discover(e.to_string()))`
call sites in `crates/quarto/src/commands/render.rs` (lines 240–428).
For a structured `QuartoError::Parse` — the new Q-5-17
unknown-project-type error, or any `_quarto.yml` parse failure —
`to_string()` is the fully *rendered* diagnostic (ANSI colors,
Ariadne snippet and all), which then gets re-wrapped:

- **Text path** (`execute`, line ~668): `Err(anyhow!("{}", e))` →
  anyhow's top level prints `Error: ` + `Display for
  DispatchError::Discover` prints `Project discovery failed: ` + the
  embedded rendering starts with its own `Error: [Q-5-17]`. Net:
  `Error: Project discovery failed: Error: [Q-5-17] …` — double
  prefix plus a wrapper that adds nothing.
- **`--json-errors` path** (`emit_dispatch_error_json` →
  `dispatch_error_to_diagnostic`, line ~1404): everything becomes a
  generic **Q-7-8 "Project Discovery Failed"** diagnostic whose
  `problem` field contains the ANSI-escape-soaked rendered text.
  The real code (Q-5-17), the span, and the file are all buried in
  an opaque string — machine consumers can't discriminate. This is
  worse than cosmetic.

The right pattern already exists 60 lines away: for *pipeline-stage*
parse errors, `execute_single_doc` (line ~740) matches
`Err(QuartoError::Parse(parse_error))` and either
`eprintln!("{parse_error}")` + exit 1 (bare rendered diagnostic, no
wrapper) or `emit_parse_error_json(&parse_error, …)` (one JSON line
per contained diagnostic, real codes, real source context). Discovery
errors just never reach it.

## Proposed fix

Carry the `ParseError` structurally through `DispatchError` and route
it into the existing printers.

1. **New variant** `DispatchError::DiscoverParse(quarto_core::ParseError)`
   (`ParseError` is `Clone + Debug`; no payload-type gymnastics).
   Keep `Discover(String)` for the non-structured cases (runtime I/O
   errors from `path_exists`, etc.).
2. **One helper** `fn discover_error(e: QuartoError) -> DispatchError`
   used at the `ProjectContext::discover`-shaped call sites:
   `QuartoError::Parse(pe) => DiscoverParse(pe)`, everything else →
   `Discover(e.to_string())`. The `path_exists` sites keep the plain
   string mapping (their errors aren't `QuartoError`).
3. **Text path**: in `execute`'s classify error arm, mirror
   `execute_single_doc`: on `DiscoverParse(pe)`, `eprintln!("{pe}")`
   and `std::process::exit(1)` instead of bubbling through anyhow.
   Output becomes exactly the bare rendered diagnostic — single
   `Error: [Q-5-17]` header, snippet, info — nothing else.
4. **JSON path**: in `emit_dispatch_error_json`, on `DiscoverParse(pe)`
   call `emit_parse_error_json(&pe, None)` — real per-diagnostic
   codes and locations. Q-7-8 remains for genuinely unstructured
   discovery failures.
5. `Display for DiscoverParse` renders the `ParseError` bare (the
   diagnostic is self-describing); `dispatch_error_to_diagnostic`
   gets an arm returning the first contained diagnostic (fallback to
   a Q-7-8-shaped message if the vec is somehow empty).

Non-goals: the redundant re-`discover()` calls inside
`execute_single_doc`/`execute_project` (their parse failures are
caught by classification first); any change to the other Q-7-N
dispatch diagnostics.

## Test plan (TDD)

Tests live in the existing files for their layer.

1. **Unit (render.rs tests)**: `classify_inputs` on a project whose
   `_quarto.yml` has `type: posit-docs` yields
   `DispatchError::DiscoverParse(pe)` with `pe.diagnostics[0].code ==
   Some("Q-5-17")` (variant pin, per the module's convention).
2. **E2e text** (extend `tests/integration/unknown_project_type.rs`):
   stderr contains exactly one `Error:` occurrence and no
   `Project discovery failed`; still contains `Q-5-17` and the
   snippet filename.
3. **E2e json** (extend `tests/integration/json_errors.rs`): render
   the same fixture with `--json-errors`; the emitted line parses as
   JSON, has `code == "Q-5-17"` (not Q-7-8), and carries a location;
   no ANSI escapes in the payload.
4. Existing suites stay green (`cargo nextest run --workspace`);
   `--json-errors` shape for unstructured discovery errors (Q-7-8)
   unchanged.

## Work items

- [x] Phase 0: failing tests. Text e2e
      (`unknown_project_type_diagnostic_is_not_double_wrapped`) and
      json e2e (`discovery_parse_error_json_carries_real_code`)
      observed failing at runtime pre-fix (json failure captured the
      Q-7-8 envelope with ANSI-soaked `problem` verbatim); unit
      variant pin (`classify_project_with_unknown_type_yields_structured_parse_error`)
      red as a compile failure until the variant landed.
- [x] Phase 1: `DiscoverParse(ParseError)` variant +
      `discover_error` helper; the three `ProjectContext::discover`
      call sites converted (the six runtime-I/O sites keep
      `Discover(String)`).
- [x] Phase 2: `execute` prints `DiscoverParse` bare + exit 1
      (mirrors `execute_single_doc`); `emit_dispatch_error_json`
      routes it to `emit_parse_error_json`;
      `dispatch_error_to_diagnostic` kept total (first diagnostic,
      Q-7-8 fallback on empty).
- [x] Phase 3: 23/23 targeted tests green; `cargo xtask lint` clean;
      full workspace suite green.
- [x] Phase 4: manual e2e (below); `cargo xtask verify
      --skip-hub-build` — change confined to `crates/quarto` (git
      status confirms), not in the WASM closure.

## End-to-end verification record (2026-08-08)

Text mode (`q2 render <repro-dir>`, exit 1) — single header, no
wrapper:

```
Error: [Q-5-17] Unknown project type `posit-docs`
   ╭─[ <repro-dir>/_quarto.yml:2:9 ]
   ...snippet + info as before, nothing else...
```

JSON mode (`q2 render <repro-dir> --json-errors`, exit 1) — one
NDJSON line, real code + location, no ANSI:

```
{"code":"Q-5-17","kind":"error","title":"Unknown project type `posit-docs`","start_line":2,"start_column":9}
```

Output inspected directly on the standalone repro from bd-sekn481x.
