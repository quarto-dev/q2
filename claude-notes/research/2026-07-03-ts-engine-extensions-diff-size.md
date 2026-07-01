# TS Engine Extensions: where the branch's 76k-line diff lands

**Date:** 2026-07-03
**Branch:** `feature/ts-engine-extensions` (merge-base with `main`: `61e2d2276`, 2026-06-19)
**Method:** `git diff --numstat main...HEAD`, categorized by path pattern

## The branch's footprint

Measured against the merge-base, so that only the branch's own work is
counted (not what main has done since the branch diverged):

```
$ git diff --shortstat main...HEAD
370 files changed, 76130 insertions(+), 3391 deletions(-)
```

The branch's real footprint is ~76k inserted lines, and the question is what
those consist of.

## Where the real 76k lands

| Category | Lines added | Share |
|---|---:|---:|
| claude-notes plans & research docs (50 files) | ~23,000 | 30% |
| Rust source | ~16,200 | 21% |
| Tests (Rust + TS) | ~14,400 | 19% |
| TypeScript source | ~12,550 | 16% |
| Test fixtures + generated/bundled JS + lockfiles/Manifest | ~7,100 | 9% |
| Other (SDD task reports, e2e specs, CI config, snapshot.jsonl) | ~2,900 | 4% |

## Category detail

### Documentation is the single largest category (~23k, 30%)

Fifty files under `claude-notes/` — the plan1a/1b/1c engine-host plan
documents (each 1,000–1,700 lines), the julia and marimo compatibility
research notes, and the plan4c validation plans. This is a consequence of the
plan-driven workflow: the epic's design deliberation is checked in alongside
its code. It inflates the shortstat but is not engineering surface in the
review-burden sense.

### Rust source (~16.2k, 21%) is almost entirely quarto-core's engine subsystem

The breakdown by area within `crates/quarto-core/src/`:

| Area | Lines added |
|---|---:|
| `engine/` | ~9,960 |
| `stage/` | ~1,690 |
| `extension/` | ~1,065 |
| `project/` | ~795 |
| top-level (`pipeline.rs`, `render.rs`, …) | ~700 |

The four big engine files are `ts_process.rs` (+3,006, subprocess management
and demux for the Deno host), `ts_engine.rs` (+2,444, the `ExecutionEngine`
implementation), `ts_protocol.rs` (+1,744, pure serde wire types), and
`resolution.rs` (+1,413, multi-engine language-claim resolution). Outside
quarto-core: the `quarto` CLI adds ~690 (mostly the new `build-ts-extension`
command), `quarto-preview` ~540, and small touches elsewhere.

### TypeScript source (~12.5k, 16%) is the three new ts-packages

`quarto-api` (+6.6k — the Jupyter-to-markdown converter and markdown regex
utilities are the largest files), `quarto-engine-host-deno` (+3.7k, the host
runtime), and `quarto-types` (+2.4k, the TS mirror of the wire protocol).

### Tests (~14.4k, 19%) split roughly evenly between languages

TS tests total ~9.6k (`quarto-api` ~5.0k; `quarto-engine-host-deno` ~4.6k,
of which `host.test.ts` alone is 3,318 lines). Rust integration tests total
~4.5k in quarto-core, dominated by the julia/marimo/echo engine end-to-end
suites (~1.2k each, following the `tests/integration/` consolidation layout).
The test-to-production-code ratio is roughly 0.7 : 1.

### Generated and mechanical content is real but modest (~7.1k, 9%)

This is the honest answer to "is the big diff just generated code?" — no.
The genuinely generated/mechanical lines are: compiled extension bundles
checked in as test fixtures (`julia-engine.js` +1,378, `marimo-engine.js`
+722), a Julia `Manifest.toml` (+1,111), hand-written fixture sources
(~3.0k), and lockfile churn (~1.6k). Together about 9% of the branch.

## Summary

Of the 76k inserted lines, about 30% is planning and research documentation,
and about 9% is fixtures/bundles/lockfiles. The material engineering
contribution is **~29k lines of production code** (16.2k Rust + 12.5k
TypeScript) plus **~14.4k lines of tests**.

## Related

- WASM size: the branch's WASM-reachable code footprint is ~50–90 KB (the
  native subprocess machinery is cfg-gated out and verified absent by symbol
  scan). The 35 MiB precache-limit breach on this branch was a 51 KB overage
  against a zero-headroom limit; the reclaimable slack is the 3.18 MB debug
  name section tracked in bd-vm53h64q. See
  `.superpowers/sdd/1c-wasm-size-investigation.md` (gitignored diagnosis doc)
  and the stopgap commit `4d35857ea`.
