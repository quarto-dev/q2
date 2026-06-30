# Task 2 Report — Parse `contributes.engines` + `.js` validation + warning emitter

## Status: DONE

## Commit

`7d0d047ab` on `feature/ts-engine-extensions`

## Files changed

- `crates/quarto-core/src/extension/read.rs` — added `parse_engines`, `parse_external_engine`,
  `parse_claims_map`, `parse_static_language_claim`, `parse_string_list`; updated
  `parse_contributes` to call them and extended the "at least one" check; 17 new tests.
- `crates/quarto-core/src/extension/types.rs` — added `engine_contribution_missing_fields_warning`
  and its `use quarto_error_reporting::DiagnosticMessage` import; 6 new tests.

## TDD RED → GREEN sequence

### Phase 1 — types.rs warning emitter (P1-11)

**RED**: Added 6 tests in `extension::types::tests` that call
`engine_contribution_missing_fields_warning`. Compile error:
```
error[E0425]: cannot find function `engine_contribution_missing_fields_warning` in this scope
   --> crates/quarto-core/src/extension/types.rs:395:17
```
(6 identical errors, one per call site)

**GREEN**: Implemented the function. Logic: match on `External`, collect which of
`name`/`claims`/`file_extensions`/`claims_files` are `None` (not `Some(empty)`), format a
`DiagnosticMessage::warning` naming those fields; return `None` for `Reorder` or a fully-declared
`External`.

### Phase 2 — read.rs engine parser (P1-8, P1-9, happy path, None/Some(empty), shorthand,
engines-only)

**RED**: Added 8 new tests in `extension::read::tests`. Compile warnings (unused imports for
`EngineContribution`/`ClaimKind`/`StaticLanguageClaim`) plus runtime failures — all
`test_engine_*` tests would panic or error because the engine parsing code didn't exist yet.

**GREEN**: Implemented the full engine parsing chain in `read.rs`.

## Test list and counts

```
cargo nextest run -p quarto-core -E 'test(extension::)'
68 tests run: 68 passed, 2517 skipped
```

New tests added (17 total):

**read.rs (11 new)**
- `test_engine_ts_path_rejected` (P1-8)
- `test_engine_uppercase_js_rejected` (P1-9a)
- `test_engine_mjs_path_rejected` (P1-9b)
- `test_engine_external_happy_parse`
- `test_engine_claims_present_but_empty_is_some`
- `test_engine_absent_optional_fields_are_none`
- `test_engine_claims_shorthand_forms`
- `test_engines_only_extension_is_valid`

**types.rs (6 new)**
- `warning_names_missing_name_field` (P1-11)
- `warning_names_missing_claims_field` (P1-11)
- `warning_names_missing_file_extensions_field` (P1-11)
- `warning_names_missing_claims_files_field` (P1-11)
- `no_warning_when_all_fields_present_even_empty` (P1-11, Some(empty) = declared)
- `no_warning_for_reorder_variant` (P1-11)

## Verification commands and output

```
cargo build -p quarto-core
   Finished `dev` profile [optimized + debuginfo] target(s) in 3.14s

cargo nextest run -p quarto-core -E 'test(extension::)'
   Summary [0.253s] 68 tests run: 68 passed, 2517 skipped

cargo nextest run --workspace --exclude wasm-qmd-parser
   Summary [69.746s] 10474 tests run: 10474 passed, 197 skipped
```

## Implementation notes

- `parse_static_language_claim` uses `yaml_rust2::Yaml` enum variants directly (Scalar arm
  pattern-matches on `Boolean(false)`, `Boolean(true)`, `Integer(n)`) since that's what the
  `ProjectConfig` interpretation context produces.
- The `fallback` key in `claims` gets special treatment: its object form is `{ priority?: int }`
  with kind implicitly `Fallback` (no `kind` field in the YAML). Other keys use the full
  `{ kind, priority?, whenClass? }` form.
- `file-extensions` and `claims-files` use the hyphen YAML keys; the struct fields use
  snake_case (`file_extensions`, `claims_files`).
- The "at least one sub-field" error message was extended to include `engines` in the list.
- `engine_contribution_missing_fields_warning` writes the message into `w.title` (the
  `DiagnosticMessage::warning(msg)` API) — tests assert on `w.title.contains("field-name")`.

## Concerns

None. All brief requirements implemented as specified.
