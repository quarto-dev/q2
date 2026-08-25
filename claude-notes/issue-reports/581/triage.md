# Issue #581 — inline `brand:` block in front matter always fails with Q-14-1

- **GitHub**: https://github.com/quarto-dev/q2/issues/581
- **Reporter**: @mcanouil (Mickaël Canouil), 2026-08-23
- **Triage date**: 2026-08-25
- **Worktree**: `.worktrees/issue-581` (branch `issue-581`, based on `main` @ `05b6fd75c`)
- **Braid strand**: bd-vk4olgv6
- **Scope**: both halves of the report — (a) inline `brand:` block in front matter fails with Q-14-1, (b) `brand: _brand.yml` in front matter emits a spurious Q-1-20 warning. The third item the reporter mentions (per-colour `light`/`dark`, GH #580) is already fixed and covered by `unified_brand_light_dark_renders_both_stylesheets` in `crates/quarto-core/tests/integration/brand_render.rs`; it is out of scope here.

## Summary

The reporter's analysis is correct in every particular, and both halves reproduce
exactly as described at `main` @ `05b6fd75c`. Front matter is converted with
`InterpretationContext::DocumentMetadata`, which parses every untagged string as
markdown into `ConfigValueKind::PandocInlines`; the brand deserializer's
YAML-rebuilding walker rejects that variant, so **any** inline `brand:` block in
front matter fails with Q-14-1, while the identical block in `_quarto.yml`
(ProjectConfig context, strings stay `Scalar`) renders fine. Real bug; the fix is
small and has an established home: the load-time key-path annotation table that
GH #457 introduced for exactly this class ("protecting a machine-facing key from
markdown"), extended with subtree matching. Filed as bd-vk4olgv6.

## Reproduction

Fixtures in this directory. All commands run from the repo root with the binary
built at `05b6fd75c`; outputs inspected by hand.

**(a) Inline block — fails (should render):**

```bash
cargo run --bin q2 -- render claude-notes/issue-reports/581/repro-inline.qmd
```

```text
Error: [Q-14-1] Invalid theme configuration
 5 │     background: "#b22222"
   │                 ────┬────
   │                     ╰────── brand block must be plain YAML, not Pandoc inlines/blocks
1 error
```

**(b) Brand-file form — renders but warns (should be silent):**

```bash
cargo run --bin q2 -- render claude-notes/issue-reports/581/repro-file.qmd
```

```text
Warning: [Q-1-20] Failed to parse metadata value as markdown
 3 │ brand: _brand.yml
   │        ╰──────────── This is the opening '_' mark.
1 warning
```

The render succeeds and the brand is applied — `repro-file_files/styles.css`
contains `b22222` (inspected).

**(c) Control — same block in `_quarto.yml` renders cleanly:**
`control-project/` here; `q2 render control-project` succeeds with no
diagnostics, and the compiled theme CSS
(`index_files/quarto/quarto-theme-*.css`) contains `b22222` (inspected).

## Localization

The reporter's `file:line` pointers all check out:

- `crates/pampa/src/pandoc/meta.rs:433-448` — untagged-string default: in
  `DocumentMetadata` context the string is parsed as markdown
  (`parse_yaml_string_as_markdown_to_config`), yielding
  `ConfigValueKind::PandocInlines` for every string leaf under `brand:`. On
  parse *failure* (the `_brand.yml` underscore) it warns Q-1-20 and falls back
  to an error-recovery `Span` whose plain text is the literal — which is why the
  path form limps through.
- `crates/pampa/src/pandoc/meta.rs:407-431` — the annotation-table consult
  (`meta_annotations::annotated_interpretation`) that replaces the untagged
  default per key path. This is the seam the fix uses.
- `crates/pampa/src/pandoc/meta_annotations.rs` — the table itself (bd-v7ixzsp5,
  GH #457). Its own docs say: "protecting a machine-facing key from markdown
  goes here". Current matching is **exact-length only** (`*` = exactly one
  segment); it cannot yet express "the whole subtree under `brand`".
- `crates/quarto-sass/src/config.rs:799-829` (`extract_single_brand_ref`) — path
  form goes through `config_value_as_text` → `as_plain_text()`, which tolerates
  `PandocInlines`; inline form goes through `config_value_to_yaml_value`.
- `crates/quarto-sass/src/config.rs:864-869` (`config_value_to_yaml_value`) —
  rejects `PandocInlines`/`PandocBlocks` with the Q-14-1 message, anchored at
  the first string leaf. This is where repro (a) dies.
- Consumers read only the **top-level** `brand:` key of merged metadata
  (`config.get("brand")` at `crates/quarto-sass/src/config.rs:304` and `:630`);
  there is no `format.*.brand` read, so one table entry suffices.
- Working model to copy from: `_quarto.yml` path,
  `InterpretationContext::ProjectConfig` (`crates/quarto-core/src/project/mod.rs:169`),
  which stores `Scalar(String)` leaves — the shape the brand deserializer wants.

A third, quieter defect falls out of the same root cause: a brand string value
that parses *successfully* as markdown is silently rewritten before
`as_plain_text()` flattening (e.g. emphasis markers are consumed). The load-time
fix below eliminates this too.

## Open questions — resolved during triage

- **Where should the fix live — pampa load time, or tolerate `PandocInlines` in
  quarto-sass?** Load time. `meta_annotations.rs`'s own decision table says
  machine-facing keys are protected there; a consumer-side
  `as_plain_text()` flatten in `config_value_to_yaml_value` would be lossy
  (markdown-mangled values accepted silently) and would leave the Q-1-20
  warning half unfixed. Keep the walker strict.
- **Which `Interpretation` for the subtree?** `PlainString`. It reproduces the
  known-good ProjectConfig behavior exactly, for both the path form and inline
  block leaves. `Path` was considered for the `brand: <file>` form but would
  newly enroll the value in metadata-merge path rebasing; the path-resolution
  contract (`claude-notes/designs/path-resolution-model.md`, consumption-site
  inventory) already records `brand:` in `quarto-sass/src/config.rs` as
  "project-root by construction", so `PlainString` + the existing
  project-dir join is per contract. No behavior change, no scope-out needed.
- **Does the annotation break `_quarto.yml` or explicit tags?** No. The table is
  consulted for *untagged* scalars in both contexts; in ProjectConfig context
  `PlainString` is a no-op. Explicit tags (`!md`, `!str`, …) take earlier
  branches and still win — an author writing `!md` inside `brand:` still gets
  inlines and still gets the (correct) Q-14-1 rejection.
- **Does `format.*.brand` need an entry?** No consumer reads it (see
  Localization); per the table's "small and boring" rule, skip it.
- **Should any field under `brand:` be excluded from the subtree rule —
  specifically `brand.meta`?** No — checked against the brand-yml spec
  (https://posit-dev.github.io/brand-yml/, per-field page
  `brand/meta.html`, 2026-08-25). `meta` is the spec's loose corner: "Both
  `name` and `link` are optional fields, and you can add additional fields
  as needed for your specific use case", and its example carries
  `description: |` prose and `founded: 1952`. But the spec defines **no
  markdown semantics anywhere** — `description: |` is a YAML block scalar,
  and every other consumer of a `_brand.yml` file (Quarto 1, Python, R
  tooling) reads it as plain YAML. Three reasons `meta` stays inside the
  PlainString subtree: (1) parity with the file form and with `_quarto.yml`
  is this issue's expected behavior, and both give literal strings for meta
  leaves; (2) excluding `meta` would leave its front-matter leaves as
  `PandocInlines`, which `config_value_to_yaml_value` rejects — the exact
  Q-14-1 crash, relocated into `meta`; (3) if Quarto ever renders
  `meta.description` as markdown, the right seam is the transform-time
  `MARKDOWN_CONFIG_PATHS` re-parse, which consumes `Scalar(String)` — so
  load-time PlainString is a precondition for that future, not an obstacle.
  An author who genuinely wants markdown in a custom meta field can still
  write `!md` (explicit tags beat annotations).
  Checking this surfaced a separate pre-existing gap: q2's `BrandMeta`
  (`crates/quarto-brand/src/types.rs:104-112`) is
  `#[serde(deny_unknown_fields)]` with only `name` + `link`, so the spec's
  own `meta.description`/`founded` example fails to deserialize even via
  `_quarto.yml` ("unknown field `description`, expected `name` or `link`" —
  reproduced, fixture `exp-meta/`). Filed as bd-8q5o86r1
  (discovered-from bd-vk4olgv6); orthogonal to this fix.
- **Was pre-flight verify green?** Effectively yes: one test failed in the full
  13k run — `quarto-core engine::ts_engine::tests::test_race_free_instance_exclusive`
  — and passed immediately in isolation (0.3s). Flaky under sibling-checkout
  load; filed as bd-xxpbo8cf (discovered-from bd-vk4olgv6).

## Fix plan (TDD)

Phase 1 — tests first, verify each fails before implementing:

1. `crates/pampa/src/pandoc/meta_annotations.rs` unit tests: a subtree pattern
   for `brand` matches `["brand"]`, `["brand","color","background"]`,
   `["brand","light"]`; does not match `["my","brand"]`, `["brandx"]`;
   existing exact-length entries unaffected.
2. `crates/pampa/src/pandoc/meta.rs` tests (module `tests`, ~line 646):
   DocumentMetadata conversion of `brand: {color: {background: "#b22222"}}`
   yields `Scalar` string leaves (not `PandocInlines`); `brand: _brand.yml`
   yields `Scalar("_brand.yml")` with **zero** diagnostics.
3. `crates/quarto-core/tests/integration/brand_render.rs` e2e regressions,
   modeled on `unified_brand_light_dark_renders_both_stylesheets` (the GH #580
   test, which already drives `render_to_file`):
   - exact GH #581 repro: front matter with inline
     `brand: {color: {background: "#b22222"}}` renders and the compiled theme
     CSS contains the color;
   - `brand: _brand.yml` front matter renders with no Q-1-20 (assert on the
     render result's collected diagnostics; if the harness doesn't expose
     them, capture warnings via the non-quiet path).

Phase 2 — implementation:

4. Extend `ANNOTATIONS` matching with a subtree form: a trailing `"**"` pattern
   segment matches zero or more remaining path segments (so `["brand", "**"]`
   covers `brand` itself and every descendant). Keep `*` = exactly one segment,
   exact-length otherwise, no suffix matching.
5. Add the entry `(&["brand", "**"], Interpretation::PlainString)`, with a
   comment citing GH #581 / bd-vk4olgv6.
6. Update the module docs' path-semantics section for `**`.

Phase 3 — verification:

7. `cargo nextest run --workspace` (pampa change can affect downstream crates).
8. `cargo xtask verify` — **full**, not `--skip-hub-build`: pampa is in the
   hub-client WASM closure.
9. End-to-end through the real binary: re-run the three repro commands above;
   (a) must render with the color in the theme CSS, (b) must be warning-free,
   (c) must stay unchanged. Inspect outputs, record in the session summary.

## Outcome / recommended next step

Filed **bd-vk4olgv6** (bug, p1) with the fix scope above. Discovered work:
**bd-xxpbo8cf** (flaky `test_race_free_instance_exclusive` under load) and
**bd-8q5o86r1** (`BrandMeta` rejects spec-legal extra meta fields).
Fix can proceed on this branch (`issue-581`) as a follow-up commit.

## Verification commands used

```bash
gh issue view 581 --repo quarto-dev/q2 --json title,body,author,createdAt,labels,comments
gh issue view 457 --repo quarto-dev/q2 --json title,state,body   # annotation-table precedent (closed)
gh issue view 580 --repo quarto-dev/q2 --json title,state,body   # per-colour light/dark (closed)
cargo xtask verify --skip-hub-build                              # pre-flight (1 flaky fail, see above)
cargo nextest run -p quarto-core -E 'test(test_race_free_instance_exclusive)'  # isolated: PASS
cargo xtask create-worktree --issue 581
cargo run --bin q2 -- render claude-notes/issue-reports/581/repro-inline.qmd   # Q-14-1 error
cargo run --bin q2 -- render claude-notes/issue-reports/581/repro-file.qmd     # Q-1-20 warning, renders
cargo run --bin q2 -- render claude-notes/issue-reports/581/control-project    # clean render
grep -rlo 'b22222' <render outputs>                                            # brand applied
```

## Cross-references

- bd-vk4olgv6 — this bug; bd-xxpbo8cf — flaky test discovered during pre-flight
- bd-8q5o86r1 — `BrandMeta` deny_unknown_fields vs spec's open `meta` (discovered)
- brand-yml spec: https://posit-dev.github.io/brand-yml/ (meta: `brand/meta.html`)
- bd-v7ixzsp5 / GH #456, #457 — the annotation table this fix extends
- bd-y89ihf0i — the `as_str()` vs `as_plain_text()` audit (same PandocInlines class, consumer side)
- GH #580 — per-colour light/dark (closed; test at `brand_render.rs:204`)
- `claude-notes/designs/path-resolution-model.md` — `brand:` is "project-root by construction"
- `CLAUDE.md` § metadata-as-str lint — related but not implicated (the walker here is not an `as_str()` read)
