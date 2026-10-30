# Issue #965 — Incremental writer drops a backslash hard break's newline when a sibling word is edited

- **GitHub**: https://github.com/quarto-dev/q2/issues/965
- **Reporter**: @davidrsch (David), 2026-10-29
- **Triage date**: 2026-10-30
- **Worktree**: none, by request. Topic branch `issue-965` in the main checkout, based on `main` @ `024cbe4a8`.
- **Braid strand**: bd-o662eis5 (incidental: bd-einzpv4r)
- **Scope**: the whole issue, which is a single report. The fix is not applied here; it was only trialled and reverted.

## Summary

Reproduced at HEAD. This is a real data-corruption bug, the reporter's root cause is correct, and their proposed fix works in a trial. The impact is wider than reported: a hard break followed by emphasis loses the emphasis, because the surviving backslash escapes the delimiter.

## Reproduction

`repro.qmd` is the issue's input. `repro_probe.rs` parses each input, appends `Z` to the first `Str`, runs `compute_reconciliation` and `incremental_write`, then re-parses the output. Its header gives the commands to re-run it.

| Case | Input | Output at HEAD | Inline shape preserved |
| --- | --- | --- | --- |
| issue | `Last line\⏎next line.` | `LastZ line\next line.` | no |
| two breaks | `Last\⏎mid\⏎end.` | `LastZ\mid\end.` | no |
| break before emphasis | `Last\⏎*next* line.` | `LastZ\*next* line.` (emphasis lost) | no |
| two-space break | `Last line␠␠⏎next line.` | correct | yes |
| soft break (control) | `Last line⏎next line.` | correct | yes |
| blockquote | `> Last line\⏎> next line.` | correct | yes |
| bullet list | `- Last line\⏎  next line.` | `* LastZ line\⏎  next line.` | yes |
| inside emphasis | `*Last line\⏎next line.*` | correct | yes |

The list's `-` becoming `*` shows that case is re-serialized as a whole block rather than spliced, which is why it escapes the bug. The blockquote and emphasis cases were only checked by their output; the path they take was not traced.

## Localization

- `crates/pampa/src/pandoc/treesitter_utils/postprocess.rs:1554`: Step 0 drops a `SoftBreak` that follows a `LineBreak` and discards its span. This is the defect.
- `crates/pampa/src/writers/incremental.rs:814`: the `KeepBefore` arm of `assemble_inline_content` concatenates each kept inline's own span with no gap filling, which exposes the defect.
- Model to copy: `merge_strs`, `postprocess.rs:1976`, folds a merged-away node's span in with `combine`.

Two-space breaks are unaffected because that `LineBreak` is built from a single node that already includes the newline (`crates/pampa/src/pandoc/treesitter.rs:963`).

## Open questions — resolved during triage

**Does the proposed fix work?** Yes. With the `SoftBreak` span combined into the `LineBreak`, all eight probe cases produced correct output with the inline shape preserved.

**What else does it disturb?** `cargo nextest run --workspace` under the trial: 17539 of 17540 passed. The one failure is `annotated_qmd_fixture_guard`, for fixtures `inline-types` and `zero-width`. Both gain `Concat` entries for hard breaks, so the failure is expected from the fix. They must be regenerated and their diffs reviewed, as the guard's message requires.

**Does a `Concat` on a body node break the "all-`Original`" pin (`preview_untransformed_baseline_body_pool_is_all_original_own_file`, `crates/quarto-core/src/pipeline.rs:5045`)?** The pin stayed green because its fixture has no hard break. Its message reads as a claim about every body node, but unpatched `a\_b c` already yields `Concat` body entries, so this fix adds a new instance of an existing shape rather than a new one. The bytes the `LineBreak` copies are its own, so the copy site's safety argument holds. The pin's wording overstating its reach is existing and is not part of this fix.

**Do the TS consumers accept the regenerated fixtures?** Not established. The `annotated-qmd` suite is already red at HEAD for an unrelated reason (below), so it gives no signal either way. `hub-client`'s WASM tests were not run under the trial, so the fix must run them with a rebuilt bundle.

## Outcome / recommended next step

Filed bd-o662eis5 (P1: silent content change on edit). Fix scope:

1. Regression tests in `crates/pampa/tests/integration/incremental_writer_tests.rs`, covering at least the issue, two-break and break-before-emphasis cases.
2. In Step 0, combine the dropped `SoftBreak`'s span into the preceding `LineBreak`.
3. Regenerate and review `ts-packages/annotated-qmd/examples/{inline-types,zero-width}.json`.
4. Run full `cargo xtask verify` with a rebuilt WASM bundle.

Incidental: filed bd-einzpv4r. `npm test` in `ts-packages/annotated-qmd` fails 83 of 161 on clean HEAD (`Unknown SourceInfo type 4`), and neither `verify` nor the `ts-test-suite` workflow runs it.

Pre-flight notes: `verify --skip-hub-build` first failed here on a stale local WASM bundle, then on stale `node_modules`. It passed after `npm run build:wasm` and `npm install`, so HEAD itself is green.

## Verification commands used

```bash
gh issue view 965 --repo quarto-dev/q2 --json title,body,author,createdAt,labels,comments
cargo xtask verify --skip-hub-build
npm run build:wasm                      # from hub-client/
npm install
cargo nextest run -p pampa --test integration issue_965_probe --no-capture
cargo nextest run --workspace --no-fail-fast    # under the trial fix
cargo nextest run -p pampa --test integration annotated_qmd_example_fixtures_match_live_writer
npm test                                # from ts-packages/annotated-qmd/, on clean source
```

## Cross-references

- bd-o662eis5: this bug
- bd-einzpv4r: `annotated-qmd` suite red at HEAD
- bd-g7qh1ltt: `handleConcat` bug in `annotated-qmd`, for replacement-bearing concats; a hard break's concat is not one
- bd-6392eba3: incremental writer helpers that misread `Concat` spans
- `claude-notes/research/2026-08-21-provenance-audit-findings.md`
