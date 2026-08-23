# Provenance, Plan 2 of 3: consumers (`quarto-error-reporting`, q2)

**Epic:** `bd-mxa44voa`.
**Depends on:** Plan 1 (`2026-08-20-provenance-1-foundations.md`). **Read Plan
1's § Design before starting** — this plan uses `ProvenanceBuilder`, the
lockstep *walker*, *desync*, `Concat`/`Substring` and `strict-provenance`
without redefining them.
**Sibling:** Plan 3 = `2026-08-20-provenance-3-audit-and-fix.md`.

**Absorbed strand** — closed, its content is Phase 5 here:
`bd-chmbr0zl` (panic boundary around diagnostic rendering).
Also closes `bd-ariadne-config-span-char-boundary-panic-rkqmhzrg` (Phase 1).

Background, the bug class, and the design rationale live in Plan 1. This plan
is the consumer side: take the corrected provenance, use it, and delete the
workarounds that existed because it was wrong.

### The publish chain, stated once

**Four** crates.io releases sit between the phases below, and three of them are
Plan 1's. Getting this wrong means a phase built against a version that does
not exist yet.

```
Plan1-P1 publishes quarto-source-map 0.1.2   (behavior fixes only)
        ├──► Plan2-P1 publishes quarto-error-reporting 0.2.2   (this plan)
        └──► Plan1-P2 publishes quarto-source-map 0.1.3        (ProvenanceBuilder)
                     └──► Plan1-P2 publishes quarto-yaml 0.1.3
                                  └──► Plan2-P2 refreshes q2's lock to all four
```

**`ProvenanceBuilder` is not in 0.1.2.** Plan 1 deliberately holds it out —
0.1.2 carries the four behavior fixes, and the builder ships as **0.1.3**, as
Phase 2's last act, after its walker has driven it green against a path patch.
An earlier revision of this section said three releases and named 0.1.2 as the
version this plan consumes; both were wrong.

So **Phase 1** needs Plan 1's *Phase 1* published — not merely its Phase 0
evidence, and 0.1.2 is the right version there, since Phase 1 needs only the
floor. **Phase 2** additionally needs Plan 1's *Phase 2* published, because it
takes `quarto-source-map 0.1.3` **and** `quarto-yaml 0.1.3`. Phase 3 onward
needs `content_source_info` and `ProvenanceBuilder`, i.e. everything above.
`^0.1.0` would pull 0.1.3 by luck, but the refresh target and the confirmation
step must name it.

## EXECUTION STATUS — session 1 (2026-08-22)

**Phases 1 and 2 COMPLETE and review-clean. Phase 3 partially complete. Phases 4-7 not
started.** Ten commits on `feature/yaml-provenance`, every task reviewed, one fix round.

| | landed | commits |
|---|---|---|
| Phase 1 | `quarto-error-reporting` **0.2.2 published**; both crash tests re-anchored; zero-width label measured for both renderers | upstream repo, PR #5, merge `87f1d38a1` |
| Phase 2 | both lockfiles onto 0.2.2 / 0.1.3 / 0.1.3 | `f250c9bf0`, `77bd9d6c0`, `80aca04a7`, `9c19df14f`, `e86b9c10b` |
| Phase 3 | `Scalar` struct variant (207 sites); `resolve_span` piecewise; **the root-cause threading fix**; binder/renderer agreement | `6a5de44b6`, `c9a77d18c`, `b7067903d`, `6b132bccb`, `1b6d30c08`, `a23f25573` |

**Publish chain complete** — all four releases are live: `quarto-source-map` 0.1.2 and
0.1.3, `quarto-yaml` 0.1.3 (Plan 1's), `quarto-error-reporting` 0.2.2 (this plan's Phase 1).

**The epic now reaches q2**, which was Plan 1's outstanding hand-off obligation 10.

**Root-cause fix confirmed end-to-end through the real binary**, not just tests: on the
block-scalar fixture the two `Q-2-9` warnings now report `_quarto.yml:9:7` and `9:26`,
where they previously reported `8:10` and `9:14` — the first of those being the **wrong
line**, the only such misattribution in the epic.

Test baseline: **12879 passed / 198 skipped**. Snapshots moved: 3 (all intern-index
renumbering; no resolved position changed — verified by hand-tracing the byte ranges).

### Corrections to this plan made during execution — READ THESE BEFORE CONTINUING

Each is written in place at the item it affects, as an indented `> CORRECTED` block. They
are not stylistic; three of them change what you should build.

1. **Phase 3, "Dispose of the dead converter"** — the preferred option (delete +
   retarget `materialize.rs`'s `mod spans` at `yaml_to_config_value`) **cannot compile**:
   pampa depends on quarto-config, so the retarget needs a dependency cycle. Take the
   plan's own "keep it in lockstep" alternative; the correction says exactly what to do.
2. **Phase 3, "The binding regression"** — the predicted symptom ("renders with no source
   snippet at all") **does not manifest**. `MetadataMergeStage` pre-registers the config
   file earlier and unconditionally, masking it. The defect is latent, not live; the fix
   still stands on its own merits. Critically: **the CLI fixture is vacuous for this item**
   — it passes identically pre- and post-fix, verified by reverting.
3. **Phase 3, the desync report** — counts two causes of `None`; upstream documents
   **three**, and the missing one (hand-built node / unresolved alias) is not a bug. Scope
   and fallout handling are in the correction.
4. **Phase 4, the verbatim/zero-content tag rule** — do **not** store the stripped quotes
   as zero-content pieces. It would re-include the quotes in the hull and defeat the phase.
5. **Phase 2 was split in two** (`quarto-source-map` + `quarto-yaml` first, then
   `quarto-error-reporting` 0.2.2 once published) so the q2-side work did not block on a
   crates.io publish. Both halves are done.

### Two things this plan did not know about

- **q2 has a SECOND tracked lockfile.** `crates/wasm-quarto-hub-client/Cargo.lock` belongs
  to an independent nested workspace (a deliberate bare `[workspace]`) and was still on
  `quarto-source-map` 0.1.0 / `quarto-yaml` 0.1.2 / `quarto-error-reporting` 0.2.1 — so the
  **WASM / hub-client path** (what `q2 preview` and Quarto Hub run) was getting none of this
  epic. Now refreshed (`e86b9c10b`) and confirmed by a full 14-step `cargo xtask verify`
  that leaves it **unchanged** rather than regenerating it. The plan says "refresh q2's
  lockfile", singular, in Phase 2 and in Plan 1's obligation 10; there are two.
- **The incremental-rebuild hash must not see provenance.**
  `quarto-ast-reconcile/src/hash.rs` deliberately excludes `ConfigValue.source_info`; its
  `Scalar` arm must keep hashing **only** the `Yaml`. Hashing `content_source_info` would
  over-invalidate every incremental rebuild **with no test failing**. Honoured in
  `6a5de44b6`; keep it that way.

### Test-count arithmetic (so a future delta check is not misread)

An in-crate unit test under `crates/pampa/src/**` counts **twice** in
`cargo nextest run --workspace`: pampa has a `[[bin]]` target over the same source, so
nextest runs its lib tests in both `pampa` and `pampa::bin/pampa`. A task adding N pampa
in-crate tests and M integration tests moves the workspace count by **2N + M**. Crates
without a second target over the same source (e.g. `quarto-core`) do not double.

### Out-of-plan findings filed as braid strands

- **bd-78e1ahjc** — `quarto-error-reporting`'s `test_location_in_to_text_with_context`
  fails under `cargo test --no-default-features`. Pre-existing (reproduced at `4da3385`),
  unrelated to this epic; neither that repo's CI nor this plan exercises that configuration.
- **bd-8k41zq68** — `attach_config_source`'s read-and-register may be dead weight, since
  `MetadataMergeStage` already covers every candidate it can name. A design question, from
  correction 2 above.


## EXECUTION STATUS — session 2 (2026-08-22)

**Phases 1-4 COMPLETE and review-clean. Phases 5-7 not started.** Nine further commits on
`feature/yaml-provenance`, every task reviewed, four fix rounds.

| | landed | commits |
|---|---|---|
| Phase 3 (finished) | dead converter privatized + threaded in lockstep + bound; desync warning in **both** converters + the two `None` comments; `caption_inlines` / `fig-cap` | `2f2a4d2a9` `454f959d3` `33919d7cf` `876bc5081` `421c6532c` |
| Phase 4 | attribute decoder drives `ProvenanceBuilder`, `callout.rs` workaround **deleted**; obligation 8 discharged by measurement; `@quarto/annotated-qmd` 0.2.0 | `07d2c1ff5` `de3697610` `1dbfa7b2b` `4aa87230f` `3efcb2c48` `93b212200` `962525b3a` |

Test baseline: **12914 passed / 198 skipped** (Rust), **161 passed** (annotated-qmd node
suite). `npm run build:all` green. Snapshots moved: **1** (`table-caption-attr.snap`, one
source-ref, `"[30,70]"` -> `[30,70]`, quote exclusion one byte each end — the other 31 refs
byte-identical, verified by hand-derivation twice).

**The generality proof holds.** `callout.rs`'s length-arithmetic workaround is gone and
`ProvenanceBuilder` now has a second consumer in a completely different decoder, which is what
Phase 4 existed to demonstrate. Obligation 8 — the item with the worst failure mode in the epic
— is discharged with an **injection experiment**, not the plan's reachability argument: wrapping
diagnostic locations in a `Concat` corrupts `qmd-syntax-helper`'s output visibly (a splice at
byte 0; a `replace_range` percent-encoding ~340 bytes), and 5 of 7 new tests catch it.

### Corrections to this plan made in session 2 — READ THESE BEFORE CONTINUING

1. **Phase 3's "dispose of the dead converter" was unbuildable as written** (session 1 already
   recorded this). Taken as: demote to `pub(crate)`, thread provenance in lockstep, document
   the layering. The function and its helper were initially additionally `#[cfg(test)]`-gated,
   because a `pub(crate)` fn with no non-test caller trips `dead_code` under `-D warnings`.
   **Cost of that choice, recorded because it was invisible in the diff: the function was no
   longer type-checked by `cargo build --workspace`** — the hand-maintained lockstep with
   pampa's converter was enforced only by the test and `clippy --all-targets` builds.
   **Paid off in the final fix wave:** the gate is now `#[allow(dead_code)]` instead of
   `#[cfg(test)]` (same for its two helpers), so `config_value_from_yaml` and its lockstep
   partner are type-checked in every build, at the cost of one attribute. The desync-warning
   half (item 2 below) stays test-only either way — it has no non-test caller regardless of
   the gate.
2. **The desync warning lives in BOTH converters**, not only pampa's. `config_value_from_yaml`
   already takes a `diagnostics` collector, and this plan's own fallout list names
   `convert.rs`'s hand-built fixtures — which reach only quarto-config's converter.
3. **"Fixing the hand-built fixtures is a fidelity gain" is WITHDRAWN as a rationale.**
   Attaching `SourceInfo::for_test()` as content provenance silences the warning at exactly the
   synthetic fidelity the argument criticized; in a real string scalar content provenance
   *differs* from the raw span, and that difference is the entire epic. `None` was arguably the
   more honest signal for a hand-built node. The **instruction** stands (absorb in fixtures,
   never weaken the rule); the reason does not. Do not reuse it, here or in Plan 3.
4. **`resolve_span` refuses on a real, common shape.** A multi-line `#|` cell-options block is
   structurally gappy to `is_gapless`, which requires pairwise contiguity across *every* piece
   of the enclosing `Concat` — and each option line's `#|` marker sits in the gap. Measured
   blast radius: **test-only.** `span_assert` is behind a feature enabled only in
   `[dev-dependencies]`, so no rendered caret is affected. Accepted as a known limitation.
   **DECIDED in Phase 7 (R-9): HANDED TO PLAN 3**, with C7's measurement attached — see
   § Hand-off to Plan 3, item 2. Two facts this plan did not state, recorded with the
   decision: `is_gapless` is entirely **in-tree**
   (`crates/quarto-config/src/span_assert.rs:229`), so the hand-off is a *scheduling* choice
   and **not** gated on a fifth crates.io publish; and the blast radius stays **test-only**
   meanwhile, so no rendered caret is affected while it waits.
5. **An escaped attribute value is a top-level `Concat` of `Original` leaves — NOT
   `Substring{parent: Concat}`.** The decoder builds via `ProvenanceBuilder::in_file`, never
   `in_parent`. Two doc comments claimed otherwise and were corrected in `1dbfa7b2b`. If you
   find that phrasing anywhere else, it is stale. Both shapes make `resolve_byte_range` return
   `None`, but pattern-match against `Concat`.
6. **Phase 4's obligation-8 reachability argument reaches the right conclusion for the wrong
   reason.** The real separation is *inside* `pampa::readers::qmd::read`: the parse-error `Err`
   arm returns before `treesitter_to_pandoc` runs, so no `AttrSourceInfo` exists yet. "It uses
   its own `read` call" does not distinguish the two channels inside `read` — and under that
   framing the `q_2_28` exposure below is invisible.
7. **The TypeScript brief's central diagnosis was wrong, and its conclusion right for another
   reason.** Attribute values carry no `Substring` at all; but `Substring{parent: Concat}` does
   reach `annotated-qmd`'s reader via **YAML block scalars**, where every inline inside the
   scalar is a `Substring` of its `Concat`. The `Substring`-arm fix is justified on that basis.
8. **The TS `Concat` arm was ALSO wrong** — the plan and both briefs asserted it was already
   correct. It derived the exclusive end as `map(len-1).index + 1`, and that `+1` assumes the
   last content byte came from one source byte; false when the last piece is a replacement, so
   `both="\[x\]"` resolved to `[36,40]`, ending *inside* the escape. **This is the epic's
   central "content length is not source length" confusion for the fourth time, and the exact
   TypeScript mirror of upstream `quarto-source-map` commit `0c65d52`** that B1's tripwire pins.

### Latent exposures recorded, deliberately not fixed

- **`q_2_28.rs:59-61`** is the one `qmd-syntax-helper` conversion reading the **`Ok`-arm**
  warnings — the channel that runs *after* attribute decoding — and it is an `end_offset()`
  reader. Safe today **only** because `Q-2-28` is a corpus-only code with no Rust emission
  site. The day someone adds one with an attribute-derived location, that conversion splices at
  a content offset. A defensive "refuse to splice a non-`Original` span" guard is new work
  beyond obligation 8 and belongs to Plan 3.
- **`div_whitespace.rs:77`** — named as an example site by this plan — is **dead source**:
  absent from `conversions/mod.rs`, and it calls `read` with 4 arguments against a 6-parameter
  signature. Do not treat this plan's three named examples as the inventory.
- **The `callout.rs` deletion is unbindable by construction**, which is a different audit
  outcome from "unbound": given `Concat => resolve_byte_range == None`, no fixture can
  distinguish the block present from absent. Phase 6's row for it should say "no test possible;
  dead by the `Concat`/`None` invariant".

### Out-of-plan findings filed as braid strands (session 2)

- **bd-g7qh1ltt** (bug, p2) — `handleConcat` reconstructs the wrong *string* for any concat
  containing a replacement piece; root cause is the **wire format** (a piece is
  `[source_id, offset, content_length]`, so a replacement's decoded bytes are never
  transmitted). Ranges unaffected after `3efcb2c48`; `toMappedString` is public API. Latent.
- **bd-49cbyqbt** (bug, p2) — attribute **key** ranges start one byte early for any non-first
  attribute. Rust-side, pre-existing, in the key slot Phase 4 deliberately left raw.
- **bd-pncrhk4v** (chore, p3) — stale published `description` naming the removed
  `quarto-markdown-pandoc` binary; vestigial `ts-packages/annotated-qmd/package-lock.json`.

### A gating gap worth knowing

The **node suite carried 2 failing tests on this branch between `07d2c1ff5` and `3efcb2c48`.**
Phase 4's first task changed the Rust JSON writer's emitted spans and was explicitly forbidden
from touching TypeScript, so the staleness was expected — but `cargo nextest run --workspace`
does not run the node suite, so no per-task Rust gate could ever have surfaced it. When a
future plan splits a Rust change from its TypeScript consumer across tasks, the intermediate
commits are knowingly red in a suite no Rust gate runs.

## EXECUTION STATUS — session 3 (2026-08-22)

**ALL SEVEN PHASES COMPLETE. This plan is done.** Three further commits, plus this one.
(Session 2's status block above said "Phases 5-7 not started"; that sentence describes the
state when it was written, not the state now.)

| | landed | commits |
|---|---|---|
| Phase 5 | `catch_unwind` around the per-diagnostic render at **8** sites; `cfg(debug_assertions)` fault-injection seam; `bd-chmbr0zl` closed | `73673ba48` |
| Phase 6 | the shadowing audit: **20 rows**, 15 matched, 5 deviated, plus the one committed test the audit's row 19 required | `992813188` |
| Phase 7 | this reconciliation, the § Evidence for Phases 3-7, the three decisions, and the full-verify gate | this commit |

Test baseline: **12919 passed / 198 skipped** (Rust, from a full `cargo xtask verify` —
14/14 green, exit 0, the first full green in twelve commits), **161 passed** (annotated-qmd
node suite, run directly). Snapshots moved in Phase 7: **none**. Both lockfiles unchanged.

**Not handed to `finishing-a-development-branch`, deliberately.** `feature/yaml-provenance`
is the shared integration line for Plans 2 and 3; Plan 3 has not started, and the epic
`bd-mxa44voa` stays open. Nine items are routed forward in § Hand-off to Plan 3 — the most
important being that **the char-boundary snap's panic-prevention role is unwitnessed by any
test in either repo**, which is the epic's founding crash.

## EXECUTION STATUS — session 4, final fix wave (2026-08-23)

The final whole-branch review (`final-review.md`, session 3's HEAD `02aced14a`) returned
"ready to hand on, with fixes": 2 Important, 6 Minor findings, plus record-keeping. All landed
in one wave (see `final-fix-report.md` for the full accounting):

- **FIX-1 (Important, the only silent-regression surface on the branch):** both
  `quarto-error-reporting` floors bumped `0.2.1` -> `0.2.2` (`Cargo.toml:125`,
  `wasm-quarto-hub-client/Cargo.toml:20`) — 0.2.2 carries the char-boundary snap that turns a
  wrong byte offset into a wrong caret instead of a process abort, and the two versions differ
  only inside a private function with no public-API change, so a 0.2.1 resolution would have
  compiled cleanly and silently reintroduced the abort. Also `wasm-quarto-hub-client/Cargo.toml:29`'s
  stale `quarto-source-map = "0.1.0"` floor corrected to `0.1.3` (compile-caught, but misleading).
  Both lockfiles verified unchanged.
- **FIX-2 (Important):** the fourth instance of the epic's own defect, at
  `website_post_render.rs:217` — see § Workarounds that collapse and Phase 6's § Evidence above.
- **FIX-3 (Minor, one-liners):** the malformed `Concat` fixture in `config_sources.rs`
  (`(piece_a, 0)` -> `(piece_a, 8)`); the stale TS docstring invariant in `source-map.ts`; the
  `span_assert.rs` `OutOfBounds` `Display` impl now labels the arithmetic-derived `end` as
  possibly approximate; `extract_quoted_text`'s doc comment now notes the unreachable
  `SuspiciousDefault`-shaped degenerate case; `hash.rs`'s `Scalar` arm comment now notes the
  `SourceInfo: !Hash` type-level enforcement (closing hand-off item (f)).
- **FIX-4 (Minor):** `config_value_from_yaml` and its two helpers regated from `#[cfg(test)]`
  to `#[allow(dead_code)]`, paying off the cost recorded in session 2's correction 1 and Phase
  3's § Evidence — the lockstep partner is now type-checked in every build.
- **FIX-5:** dead `div_whitespace.rs` deleted (`git rm`), closing hand-off item (b).
- **FIX-6:** this plan reconciled — hand-off item (d) reasoning sharpened (structural
  unreachability via `SourceContext: None`, not "nothing written yet"), item (e) re-scoped
  (determine which renderer still aborts before pinning or downgrading, not "add a witness"),
  a new named item (h) added for a caught-panic-on-error-severity-diagnostic test, item (b)
  removed as FIX-5 supersedes it.

**Out-of-plan finding filed as a braid strand:** **bd-rj2ikb0z** — `stage/context.rs:911`
builds a fresh `DiagnosticCollector` for `_variables.yml`'s `yaml_to_config_value` call and
drops it, discarding every diagnostic from that path, while the enclosing function already
holds `diagnostics: &mut Vec<DiagnosticMessage>`. Pre-existing and unrelated to provenance; it
surfaced only because this branch added a signal (the content-provenance desync warning) that
is invisible there. Belongs to no active plan in the epic — filed rather than fixed here.

Gates: `cargo build --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo nextest run --workspace`, `cargo xtask verify` (full), the annotated-qmd node suite, and
`cargo xtask lint` — see `final-fix-report.md` for pass/fail and the exact counts. Both
lockfile SHAs reported there.

## How the four defects relate

Useful orientation when reading the phases below, because each attacks a
different one:

| Repo | Defect | Consequence | Where fixed |
|---|---|---|---|
| `quarto-yaml` + q2 | decoded value paired with undecoded provenance | every quoted-scalar span is 1 byte left, and multi-line block scalars misreport the line — in **both** the front-matter and project-config paths | Plan 1 + Phase 3 here |
| `quarto-source-map` | `Location.column` is floored to a char boundary, `Location.offset` is not | callers reading `.offset` inherit an unsanitized value | Plan 1 Phase 1 |
| `quarto-error-reporting` | renderers slice at raw byte offsets with no boundary check | a wrong offset becomes a **process abort**, not a wrong caret | Phase 1 here (`4da3385`) |
| q2 | no panic boundary around diagnostic emission | one bad diagnostic kills an already-successful render and discards the rest of the stream | Phase 5 here |

The first is the **root cause**. The second is why the bad value survived
undetected. The third is why it was fatal rather than cosmetic. The fourth is
why the blast radius was the whole render.

## Workarounds that collapse

The clearest evidence that this is worth doing. **Three q2 authors
independently hit this bug, understood it, and routed around it** — each
paying in capability:

| Site | Workaround | Cost today | Disposition |
|---|---|---|---|
| `quarto-core/src/transforms/callout.rs:431-447` (inside `attribute_value_source`, `:401-448`) | derives the parent from lengths alone when nothing was collapsed; else falls back to the whole value span | loses precision on any escaped attribute | **deleted** (Phase 4) |
| `quarto/src/commands/use_cmd/config.rs:229` (`scalar_value_span`) | byte-compares the raw span's text against the decoded value, returns `None` on mismatch | **refuses to repoint the declaration at all** | **kept** — simplification optional (Phase 4), and **declined** in Phase 7 (R-8), routed to Plan 3. This disposition is therefore **unchanged** by Plan 2 |
| `quarto-core/src/cell_options/mod.rs:196-228` | avoids the bug structurally by never decoding | constrains its input format | **untouched** |

The third is cited in `bd-mxa44voa` as the exemplary case — and it is
exemplary *precisely because* it already uses the concat-piece approach Plan 1
generalizes. Plan 3's Phase 7 asserts these three dispositions; tell that
session if any of them changes.

Two more sites pair a decoded value with a raw span but do no sub-offset
arithmetic, so they are wrong-span rather than drifting:
`transforms/theorem.rs:344-360` and `transforms/proof.rs:181-197`. Phase 4
fixes them for free and **changes their output** — see that phase's fallout
list.

**A fourth site missed by this sweep, found by the final whole-branch
review:** `project/website_post_render.rs:217` (`copy_footer_images`) reads
`cv.as_plain_text()` and re-parses it against `&cv.source_info` under a
comment claiming to "parse the same way" as `ConfigMarkdownTransform` —
which was true before this branch and became false once
`ConfigMarkdownTransform::parse_scalar_string_in_place` started preferring
`content_source_info` (`config_markdown.rs:326`). Not in the table above
because it was not discovered until the final review; fixed in the final fix
wave to destructure `ConfigValueKind::Scalar { yaml, content_source_info }`
and use `content_source_info.as_ref().unwrap_or(&cv.source_info)`, mirroring
`parse_scalar_string_in_place`. **Unbindable by construction**, same audit
category as the `callout.rs` deletion above: no consumer reads those
inlines' spans (they feed only image-URL extraction, and `parse_diags` is
discarded), so no fixture can distinguish the corrected base from the raw
one. See Phase 6's § Evidence for the recorded-as-unbindable entry.

## What Plan 1 hands us

Plan 1's § Hand-off to Plan 2 lists **nine**, and all nine are ours. An earlier
revision of this table said eight and claimed the ninth self-routed to Plan 3 —
that was true of an older Plan 1, which has since moved the `incremental.rs`
item into its § Hand-off to *Plan 3* and added a new #8. Numbering below
follows Plan 1's current copy.

| # | Obligation | Phase |
|---|---|---|
| 1 | Thread content provenance through `ConfigValue` | 3 |
| 2 | Test the zero-width renderer label | 1 |
| 3 | Re-anchor the two upstream crash regression tests | 1 |
| 4 | Release `quarto-error-reporting` 0.2.2 carrying `4da3385` | 1 |
| 5 | Re-gate this plan's Phase 1 on Plan 1 **Phase 1** publishing 0.1.2 | 1 |
| 6 | Absorb the two q2-side reactions to the `Location.offset` floor | 2 |
| 7 | Specify the desync report as warning-level and non-fatal | 3 |
| 8 | **Re-check `qmd-syntax-helper` after the `AttrSourceInfo` meaning change** | 4 |
| 9 | Carry the `AttrSourceInfo` meaning change into `annotated-qmd` | 4 |

Obligation 3 is settled as a unit test; see § In-session verification.
Obligation 8 was added after this plan's owner confirmed "all eight", so it is
the one most likely to be dropped — it is the item with the worst failure mode
in the epic (see Phase 4).

## In-session verification

Some evidence here **cannot** be recorded as a permanent test: it exists only
in a state the plan destroys. Reverting a fix to watch a test go red is not a
test; neither is exercising a crash that the next phase makes unreachable. The
standing rule for this plan:

> Anything we cannot record as a permanent test, we verify **in session** and
> paste the observation verbatim into § Evidence. An in-session verification is
> a checklist item like any other, with a named expected observation. "We
> reasoned it must be fine" is not a discharge. **Anything we *can* record
> permanently, we do** — see § Evidence's "make these permanent".

Two verifications here are genuinely unrepeatable:

1. **Phase 1** — that the two `..._does_not_panic` tests bound *before* the
   floor. After Plan 1's 0.1.2 no `SourceInfo`-carried location can deliver a
   mid-character offset to a renderer.
2. **Phase 6** — the whole revert matrix. Reverts are inherently in-session.

Phase 4's pre-fix TS range is recorded in session *and* pinned by a permanent
test, so it is not in this list.

## Test seam spec (frozen)

One row per test, bound before any code is written. **A test whose revert hunk
is not named here is not specified** — the executor will otherwise invent a
harness. Once a row goes green its assertion surface is frozen; never edit an
assertion to make it pass.

Tiers here are: **unit** (in-crate `#[test]`), **cli** (drives the `q2` binary
through `crates/quarto/tests/integration/`), **node** (`node --import tsx
--test` in `ts-packages/annotated-qmd`).

**"Hull"**, used throughout this plan: the single `(start, end)` source range
covering a discontiguous `Concat`, obtained from the `map_offset` pair —
`map_offset(0)` for the start and `map_offset(length())` for the end. It exists
only when the pieces tile the source without gaps.

| # | Test | Tier | Assertion surface | Revert ⟨hunk⟩ → RED |
|---|---|---|---|---|
| T1 | snap helper widens to whole chars, incl. the re-anchored `21..28` pair | unit | `snap_span_to_char_boundaries` return value, literal offsets | the snap helper body (`diagnostic.rs:671-686`) |
| T2 | zero-width label renders, both renderers | unit | rendered text of a `7..8`-inside-`✨` diagnostic | **`Location.offset` floor** in `quarto-source-map` — cross-repo, needs a path override |
| T3 | exclusive-end pin | unit | `map_offset(length())` on the QMD writer's concat | `Concat` exclusive-end branch (`mapping.rs:64-70`) — cross-repo |
| T4 | quoted scalar, exact columns | cli | `start_column`/`end_column` in `--json-errors` | the three re-parse bases (`meta.rs:259/:303/:316`) |
| T5 | multi-line block scalar → `9:7`, `9:26` | cli | same | the carrier read in `config_markdown.rs:283-290` |
| T6 | front matter quoted `title:` | cli | same | the three re-parse bases (`meta.rs:259/:303/:316`) |
| T7 | plain + single-line block unchanged | cli | same | **none — see § Unbindable below** |
| T8 | caret underlines the right bytes | cli | rendered text path, not JSON | whichever `resolve_span` path Phase 3 takes |
| T9 | per-page config diagnostic still gets a snippet | cli | **snippet presence** — the config file's path in rendered output, not merely "a diagnostic printed" | `bind_source_candidates`' `root_file_id()` change |
| T10 | `fig-cap` inline resolves into the option line | cli | inline `SourceInfo` byte position | `caption_inlines` (`codeblock_shorthand.rs:644`) |
| T11 | attribute escape resolves to `86..92` | cli | inner node `SourceInfo` | the `AttrSourceInfo` content-provenance swap |
| T12 | serde round-trip keeps `{"Scalar": <value>}` and yields `None` | unit | serialized JSON shape + deserialized provenance | the hand-written `Serialize`/`Deserialize` arms (`config_value.rs:222`, `:305`) |
| T13 | TS range for a quoted and an escaped value | node | `getAnnotatedParseSourceFields` range | `resolveChain`'s `Substring` arm (`source-map.ts:301-315`) |
| T14 | `block-types.test.ts` asserts the unquoted value | node | `source.substring(value.start, value.end)` | same as T13 |
| T15 | injected panic: exit 0, others print, `_site/` written, named stderr line | cli | exit code + stderr + filesystem | the `catch_unwind` boundary |

### Vacuity checks (the assertion must still discriminate)

- **T5's discriminator is the line number, not just the column.** Today's
  wrong answer is `8:10`/`9:14`; the right answer is `9:7`/`9:26`. Assert
  **both** line and column — a column-only assertion would still pass if the
  fix corrected the column while leaving the line wrong, which is the exact
  symptom that makes this case worse than an off-by-one.
- **T9 must assert snippet presence, not diagnostic presence.** The regression
  is "the warning prints with no source snippet." A test that only checks the
  `Q-2-9` warning appeared passes in both states.
- **T2's binding is cross-repo and inverted.** The zero-width highlight only
  *exists* after the floor, so its revert lives in `quarto-source-map`, not in
  the crate that holds the test. Phase 6's "revert the floor → its own tests
  red" row does **not** cover it; add a row naming this test explicitly.
- **`accum/`'s third reported column numerically equals its second truth
  column** — both are `9:25`. A test asserting `25` for that element reads as
  fixed while sitting on the stale value. If T8's optional per-element check is
  added, assert all four positions as a set, or assert the third element by
  something other than its column.
- **T4 and T6 share one revert hunk**, so Phase 6's provenance row cannot tell
  a half-done Phase 3 from a complete one. Split the row: revert the three
  re-parse bases (reddens T4/T6) separately from the carrier read (reddens
  T5), or a Phase 3 that fixes only front matter still shows a fully red
  matrix.

### Unbindable and accepted-untested — logged, not silently omitted

- **T7 cannot be reddened by reverting our code**, by construction: it asserts
  values that are correct today and must stay correct. Bind it by **mutation
  instead of revert** — apply the content-provenance base to the plain-scalar
  path too and confirm T7 goes red. Record that mutation in § Evidence.
- **The two re-anchored `..._does_not_panic` tests are unbound after Phase 1**,
  and this is known in advance rather than discovered in Phase 6. Once the
  floor lands, no hunk's revert reddens them — that is precisely why their
  coverage moved to T1. Phase 6's closing rule ("any test that cannot be made
  to fail: fix it or delete it") would condemn them, so decide now: **keep as
  renamed smoke tests with an explicit accepted-unbound note in their doc
  comment**, naming T1 as where the real coverage went. Do not let Phase 6
  rediscover this.
- **Accepted-untested, with rationale:** the `None`-fallback inertness for CLI
  `-M`/Lua metadata (its safety comes from those producers carrying
  `Generated`, which offset arithmetic already refuses — a test would assert
  the absence of a symptom); the Lua-filter degradation path (same shape); and
  the `ConfigValueKind::Scalar` migration itself (compiler-guided across 206
  sites — the type system is the test).
- **If the `resolve_span` fallback path is taken**, T8 moves to rendered
  output and a *new* row is needed: the `Substring{parent: Concat}` arm
  reports `Concat` rather than `Generated`, reverted by that arm.

## Phases

### Phase 1 — release `quarto-error-reporting` 0.2.2

**Gate: Plan 1's Phase 1 must be *published* first** (obligation 5, and
§ The publish chain). Two items below need `quarto-source-map` 0.1.2 resolved
in this crate: the zero-width label only exists once `Location.offset` floors,
and the re-anchoring is a response to that same floor. A local
`[patch.crates-io]` override is fine for development, but the release must not
ship against an unpublished dependency.

Also do not start until Plan 1's Phase 0 has captured the crash evidence.

The fix is committed on `fix/diagnostic-span-char-boundary` (`4da3385`):
`snap_span_to_char_boundaries` (`src/diagnostic.rs:671-686`), snapping **four
offsets at three call sites** (`:878` covers both the ariadne `Report::build`
anchor and the ariadne main label — the code comment says "snap once, up
front"; `:937` the detail labels; `:1032` the annotate-snippets `clamp`
closure). Do not go looking for a fourth call.

**These first two items are order-dependent, and getting them backwards
destroys evidence that cannot be recovered.** The binding experiment must run
**while this repo is still on 0.1.0/0.1.1**. Both `..._does_not_panic` tests
build `SourceInfo::original(fid, 21, 28)`, and once 0.1.2's floor is in play
`map_offset` floors 21→19 *before the renderer is reached* — so reverting the
snap no longer panics and the test cannot be made red. That is the same
mechanism Plan 1's § Risks cost (c) describes, and it is why the re-anchoring
below exists. An earlier revision of this phase listed the lock update first;
it was wrong.

- [x] **First, in-session: bind the ariadne test while it still binds.**
      On the current lock (0.1.0), revert the snap call at `:878` only, keeping
      the tests, and confirm
      `ariadne_span_starting_inside_multibyte_char_does_not_panic`
      (`src/diagnostic.rs:1601`) goes red. Paste the failure into § Evidence.
      **Scope note:** Plan 1's Phase 0 bullets 3-6 revert each hunk
      individually and already bind the *annotate-snippets* test (`:1635`) and
      the four hunks by observed panic location. This item adds the one thing
      those do not — that the *ariadne* test goes red — so run it in the same
      sitting as Plan 1's Phase 0 rather than as a second expedition
      (§ In-session verification, instance 1)
- [x] **Then take 0.1.2 in this repo.** `Cargo.toml:28` needs no edit
      (`quarto-source-map = "0.1.0"`, and `^0.1.0` accepts 0.1.2), but
      `Cargo.lock` pins **0.1.0**. Run `cargo update -p quarto-source-map` and
      **commit the lockfile** — the release runs `cargo publish --locked`, so
      without this 0.2.2 ships built against 0.1.0, and the zero-width test
      below would measure the old behavior. Everything after this point in the
      phase requires the floor to be present
- [x] **Re-anchor the two tests (obligation 3).** Settled: unit-level coverage
      is the answer, because integration-level coverage is *unreachable* after
      the floor — every path from a `SourceInfo` to a renderer offset runs
      through `map_offset` → `offset_to_location`, including `Concat` (which
      recurses to `Original`), and the public entry `to_text_with_renderer`
      takes its span from `self.location`. Concretely: extend
      `snap_span_widens_to_whole_characters` with the offset pair the two
      integration tests used (`21..28` over
      `"text: <span>Ask AI \u{2728}</span>"` — one pair, shared by both
      tests), and **rewrite the two integration tests' names and doc
      comments** to say what they now cover (a span-carrying diagnostic still
      renders under each renderer) plus why they no longer cover the snap. Do
      not delete them and do not leave the names claiming a guarantee they lost
- [x] **Test the zero-width label (obligation 2).** Measured by Plan 1 on
      content `"x = 'A✨B'"` (emoji at bytes 6..9) with
      `SourceInfo::original(fid, 7, 8)` — both ends inside the character:

      | | offsets reaching the renderer | rendered highlight |
      |---|---|---|
      | today | raw `7..8`; snap floors 7→6 and **ceils** 8→9 | `6..9`, the whole character |
      | after the floor | both arrive as `6`; the ceil finds 6 already on a boundary | `6..6`, **zero width** |

      Two things to pin in the assertion. At the `Location` level this is
      *consistency*, not regression — the columns already collapse to `6..6`
      today; the regression is purely at the rendered level, because the
      renderer ceils the **raw** offset and flooring upstream destroys the
      input its ceil needs. And ariadne's `Report::build` anchor is already
      `start..start`, so a zero-width label very likely renders — which is
      exactly the "very likely" this test exists to replace. Assert for both
      renderers
- [x] Bump crate version to `0.2.2`
- [x] `cargo test` green (this crate is not a nextest workspace), **plus**
      clippy on `--no-default-features`, ariadne-only, and
      annotate-snippets-only. The misplaced-`cfg_attr` bug found during
      development was visible *only* in the no-default-features configuration
- [x] PR → CI green → merge. **Merging to `main` publishes — there is no human
      release step.** Verified 2026-08-21 for *this* repo (Plan 1 verified the
      other two): it carries the same repo-agnostic `release.yml` using
      crates.io Trusted Publishing over OIDC with no stored token, and its
      `release` environment returns `protection_rules: []`, so no approval gate
      fires. The workflow only acts when the workspace version leads the
      registry
- [x] Close `bd-ariadne-config-span-char-boundary-panic-rkqmhzrg`

**Not a checklist item, a review note:** `annotate-snippets` is **not** the
safe sibling. It panics identically at
`annotate-snippets-0.12.16/src/renderer/source_map.rs:71` on the same input;
it simply was not the default renderer. Both paths are fixed and both are
covered — do not drop either as redundant during review.

### Phase 2 — q2 takes the updates

**Gate:** Plan 1's Phase 2 published `quarto-yaml 0.1.3`, and Phase 1 above
published 0.2.2. **The items below are order-dependent** — the pin must
precede the refresh, because the refresh is what would make the change
invisible.

- [x] **First: pin the `Concat` exclusive-end change before it lands
      silently.** Plan 1's third 0.1.2 behavior change (the exclusive-end
      branch uses the last piece's *source* length) alters a q2 production
      path: the QMD writer's provenance concat
      (`pampa/src/writers/qmd.rs:2876-2903`) pairs each block's
      `source_info()` with bytes *written*, and its only production consumer is
      `quarto-core/src/stage/stages/engine_execution.rs:733`. Measured:
      `crates/pampa/tests/integration/qmd_writer_source_info.rs` exercises only
      interior offsets, so **nothing currently reacts**. Add an exclusive-end
      assertion there against today's behavior, so the refresh below turns it
      red rather than passing unnoticed
- [x] Then refresh the lock to **`quarto-source-map 0.1.3`** (not 0.1.2 —
      that release deliberately excludes `ProvenanceBuilder`, which Phases 3
      and 4 require), `quarto-error-reporting 0.2.2`, and
      `quarto-yaml 0.1.3`. Note q2's lock currently pins
      `quarto-source-map 0.1.0` even though 0.1.1 is released, so it needs an
      explicit update regardless. Confirm the resolved version by name; `^0.1.0`
      would accept 0.1.2 and leave Phase 3 unable to compile
- [x] **No `Cargo.toml` edit for `quarto-yaml`.** Plan 1 decided the change is
      *additive* and ships as **0.1.3**, so `^0.1.2` accepts it and this is a
      lock refresh only
- [x] Confirm no `[patch.crates-io]` override for `quarto-error-reporting`
      remains. Plan 1's Phase 0 removes it as its exit condition; this is the
      confirmation. **Only the three added lines are local** — the section's
      four committed entries (`lua-src`, `tree-sitter-language`, `runtimelib`,
      `jupyter-protocol`) are load-bearing and must stay
- [x] **Absorb the two q2-side reactions to the floor (obligation 6).** Plan 1
      deliberately leaves both to us:
      - `pampa/tests/integration/test_location_health.rs:448` compares the two
        `offset_to_location` implementations' row/column; Plan 1 aligns the
        free `utils::offset_to_location` with `FileInformation`, so this **may**
        react. It only diverges for mid-char inputs, and whether any current
        fixture reaches one is **unknown until Plan 1's Phase 1 smoke step
        runs**. Do not plan for green or red — read the smoke output and
        **record what it showed in § Evidence**, so "nothing moved" is
        distinguishable from "nobody looked"
      - JSON-writer snapshots: `.offset` is emitted as `"o"`
        (`pampa/src/writers/json.rs:550`, `:555`, `:2005`, `:2014`, `:2258`,
        `:2267`) and read as `file_offset` at
        `quarto-core/src/engine/ts_engine.rs:689`. Neither snaps, which is the
        affirmative case for flooring — and the reason snapshots move.
        Per CLAUDE.md the review needs a **count, a summary of what changed,
        any surprises called out, and the affected file list**
- [x] `cargo nextest run --workspace` green

**Deleted item, recorded so it is not re-derived:** an earlier draft had "fix
the 15 test-only `new_scalar` / `new_scalar_with_tag` call sites." Plan 1's
additive decision removed it — the public constructors keep their signatures
and provenance attaches via `with_content_provenance`, so q2 has **zero**
call-site edits there. The old semver argument ("a breaking 0.1.3 would be
pulled in silently; 0.2.0 forces an explicit edit") is retracted upstream
with it.

### Phase 3 — q2 consumes `content_source_info` (the YAML path)

**Scope, decided 2026-08-21:** this phase fixes **both** YAML paths — the
deferred project-config path that `ConfigMarkdownTransform` re-parses, *and*
document front matter, which is re-parsed immediately inside
`yaml_to_config_value`. Front matter is the larger user surface (every quoted
`title:`, `description:`, caption drifts by one today) and costs the same
one-line source.

**Tests first.**

- [x] Failing test, in `crates/quarto/tests/integration/json_errors.rs`: a
      fixture with a **quoted** scalar containing raw HTML, asserting
      **exact** `start_column` / `end_column`. Nothing in q2 asserts on
      `start_column` today — measured — which is why this survived.
      **The recorded baseline "36/43 → 37/44" has no fixture attached and
      could not be reconstructed from either committed repro.** Either
      identify the document it came from and transcribe it, or derive fresh
      expected values from the fixture you write and record *those*. Do not
      copy 36/43 forward unexamined
- [x] Failing test: a **multi-line block scalar**, using the fixture
      transcribed in § Evidence below. **Measured by Plan 1 on 2026-08-21
      against the released `q2 0.24.0`** — exit 0, two `Q-2-9` warnings (the
      `<span id="y">` open and close tags), both genuinely on **line 9** at
      columns **7** and **26**, reported at **`8:10`** and **`9:14`**. The
      drift is a constant **12** = 2 preceding content lines × 6 bytes of
      stripped indent: 26 − 12 = 14, and 7 − 12 underflows past the start of
      line 9, which is why the first warning claims **line 8**. Observe red on
      both, then green at 9:7 and 9:26. This is the only symptom in the epic
      that misattributes a diagnostic to the wrong *line*, and until this
      measurement the cited pair had never been tied to a file
- [x] **DECLINED** (decision recorded, per this item's own instruction). Optional, if a per-element check is wanted: Plan 1 also measured
      `.scratch/ariadne-emoji-panic/accum/` (transcribed in its § Evidence), a
      two-span variant of the same shape giving **four** warnings at
      `8:10` / `9:13` / `9:25` / `9:42` against truth
      `9:7` / `9:25` / `9:37` / `9:54`. It shows the constant −12 per element
      rather than inferred from two points. `blockscalar/` stays canonical
      here; `accum/` is the arithmetic demonstration. **Optional — record
      "added" or "declined" at Phase 7 rather than leaving the box ambiguous**
- [x] Failing test: **front matter**. A quoted `title:` in a document's own
      front matter, asserting exact columns for an inline inside it. This is
      the path fixed at `meta.rs:259/:303/:316` below
- [x] Regression tests for a **plain** (unquoted) scalar and a **single-line
      block** scalar — both correct today and must stay correct. Guards
      against over-correcting
- [x] Add a **text-path** assertion pinning the caret position, not only the
      JSON columns. The absence of any such test is why this survived: the
      existing renderer tests assert on *style markers* (that ariadne drew its
      `╭` box, that annotate-snippets emitted `-->`) and every fixture is pure
      ASCII. Nothing asserted **which bytes get underlined**. See the
      `resolve_span` decision below for the mechanism

**The `resolve_span` decision (settle before writing the caret test).**

`assert_diagnostic_underlines` (`quarto-config/src/span_assert.rs:219`) is
built on `resolve_span` (`:147-166`), which short-circuits top-level
`SourceInfo::Concat` to `Err(SpanProblem::Concat)` at `:159` — and `Concat` is
exactly what corrected provenance produces. `Substring{parent: Concat}` is
worse: it falls through to `resolve_byte_range().ok_or(SpanProblem::Generated)?`
and reports **`Generated`**, sending a reader hunting for a filter-created
node. These are one decision, not two items:

- [x] **Preferred: teach `resolve_span` piecewise resolution** — resolve the
      two ends via `map_offset(0)` and `map_offset(length())`, the same
      mechanism Phase 4's hull needs, so one implementation serves both. Then
      both `Concat` and `Substring{parent: Concat}` resolve,
      `SpanProblem::Concat` becomes dead, and the caret test can use
      `assert_diagnostic_underlines` directly. This is the direction that
      keeps the helper useful as this epic makes `Concat` the common shape
      across the assertion helpers built on it in `listing/config.rs`,
      `callout.rs` and `materialize.rs` (7 direct callers; more reach it
      transitively through `assert_diagnostic_underlines` and
      `resolve_diagnostic_span`)
- [x] **NOT TAKEN — the preferred piecewise path landed (commit `b7067903d`); this fallback is mutually exclusive with it and must not also be done.** Fallback, if piecewise resolution proves wrong: a discontiguous span
      cannot be described by one `(start, end)` pair, so if asserting on the
      hull turns out to misrepresent what the caret covers, keep the refusal,
      **add the missing `Substring{parent: Concat}` arm** so the label says
      `Concat` rather than `Generated`, and have the caret test assert on
      rendered output instead. Record which path was taken and why in
      § Evidence — the two are mutually exclusive and a later reader must not
      try to do both

**Threading (obligation 1).**

- [x] Carry content provenance as
      **`ConfigValueKind::Scalar { yaml, content_source_info: Option<SourceInfo> }`**
      — inside the variant, not as a fourth field on `ConfigValue`. Rationale
      in Plan 1's § Hand-off; the load-bearing half is that provenance must not
      be *separable* from the value it describes, because config merging picks
      winners per key and a sibling field can be carried forward while `value`
      is replaced — producing a pair whose string came from one file and whose
      provenance resolves cleanly onto a real offset in another
- [x] **Budget the variant migration — it is the largest mechanical task in
      this epic and was previously invisible here.** `Scalar` is a **tuple**
      variant today, so making it a struct variant breaks every construction
      *and* every pattern match: **206 sites across 49 files** excluding
      `crates/*/tests/`, **240** including them (Plan 1's figures, confirmed
      independently twice). Plan for the sweep, its snapshot fallout, and the
      **WASM leg** — `wasm-quarto-hub-client` builds against these types on a
      separate target, so `cargo build --workspace` will not catch it and
      Phase 7's `cargo xtask verify` is the first thing that would.
      **Done when** `cargo xtask verify` (full) is green *and* the snapshot
      review below is filed — "planned the sweep" is not a done-condition
- [x] **Snapshot gate for the front-matter expansion.** Fixing
      `meta.rs:259/:303/:316` moves the span of every quoted `title:`,
      `description:` and caption in every fixture whose spans serialize — a
      larger movement than the JSON-writer `.offset` churn Phase 2 gates. Give
      it the same CLAUDE.md treatment: **count, summary of what changed,
      surprises called out, affected file list.** An earlier revision mentioned
      "snapshot fallout" only in passing here, while gating the smaller churn
      one phase earlier
- [x] Note for the desync report: content provenance is meaningful only for
      `Yaml::String`. A `Scalar(Yaml::Integer | Real | Boolean | Null)` carries
      `None` and **must not** trip the report below — the "None on a string
      scalar is a bug" rule is scoped to strings, and this is new user-visible
      output, so getting the scope wrong is noisy rather than silent
- [x] **Set it in one place: `pampa::pandoc::meta::yaml_to_config_value`,
      `meta.rs:242`** (`let source_info = yaml.source_info.clone();`). Derive
      the content provenance there and use it for **both** consumers of that
      binding:
      - store it in the `Scalar` variant, for the deferred project-config path
      - **pass it as the re-parse base at `:259` (`!md`), `:303` (annotated
        `Markdown`) and `:316` (`DocumentMetadata` default)** — the three
        immediate re-parses that fix front matter. Today each pairs the decoded
        `&s` with the node span, which is the bug
- [x] **Not** `quarto-config`'s `config_value_from_yaml` — that function has no
      production caller (its only call sites are its own tests, a
      `#[cfg(test)]` use at `materialize.rs:495`, and two locally-shadowed
      test helpers of the same name at `project_profile.rs:639` and
      `render_scripts.rs:712` whose bodies call the pampa converter). It is
      exported dead API
- [x] `meta.rs:34` and `:59` are **forwarders**, not sites:
      `parse_yaml_string_as_markdown_to_config` receives `source_info` as a
      parameter and hands it to `readers::qmd::read`. Nothing there can call
      `content_source_info()`
- [x] **Read the carried value in one place:** `parse_scalar_string_in_place`
      (`quarto-core/src/transforms/config_markdown.rs:284-290`)
- [x] **Dispose of the dead converter.** Delete
      `quarto-config/src/convert.rs`'s `config_value_from_yaml` (and its
      `pub use` at `lib.rs:57`), or keep it in lockstep. Deleting is
      preferred: kept, it is a public constructor that can produce
      YAML-rooted scalars with no content provenance — a hole in the invariant
      that compiles. Deleting requires **retargeting `materialize.rs`'s
      `mod spans` tests** (the bd-2mxo / bd-9yh3pzfu span-preservation tests —
      the module opens at `:485` and runs past `:615`, so budget the whole
      module) at `yaml_to_config_value`. Those tests currently guard a
      converter no render uses, so this is a fidelity gain — and the
      `resolve_span` decision above applies to them too

      > **CORRECTED 2026-08-22 — the preferred option above CANNOT BE BUILT as
      > written. Do not attempt it; take the "keep it in lockstep" alternative.**
      >
      > The retarget requires a quarto-config test to call
      > `pampa::pandoc::meta::yaml_to_config_value`, but **pampa depends on
      > quarto-config** (`crates/pampa/Cargo.toml:53`) — quarto-config sits *below*
      > pampa in the graph (its deps are only quarto-source-map, quarto-yaml,
      > quarto-pandoc-types, quarto-error-reporting, indexmap, thiserror,
      > yaml-rust2). So the retarget needs a quarto-config → pampa dev-dependency
      > cycle, dragging pampa's very large closure into quarto-config's test build.
      >
      > Nor can the tests move out to dodge it: all five `mod spans` tests call
      > `MergedConfig::new(..).materialize()` — they test **quarto-config's own**
      > materialize with real spans, so relocating them would exile a crate's tests
      > from the crate they test. And hand-building the values instead is explicitly
      > ruled out by `span_assert.rs`'s module docs (`:39-45`): synthetic
      > `SourceInfo::for_test()` spans make a wrong span indistinguishable from a
      > right one, "the bug is invisible by construction".
      >
      > **What to do instead** — this closes the hole this item actually names,
      > which is that it is a ***public*** constructor:
      >   (a) demote `config_value_from_yaml` from `pub` to `pub(crate)` and delete
      >       the `pub use` at `lib.rs:57`, so nothing outside quarto-config can
      >       reach it;
      >   (b) thread content provenance through it in lockstep with `meta.rs`'s
      >       read (the same one-line `content_source_info()` call), so it is not a
      >       fiction and the five span tests gain the same fidelity the retarget
      >       was for;
      >   (c) document it as crate-internal, with no production caller, required to
      >       stay in lockstep with `pampa::pandoc::meta::yaml_to_config_value`, and
      >       say in one line *why* it cannot simply delegate — so the next reader
      >       does not rediscover the layering.
      >
      > Verified while measuring this, so it need not be re-derived: the function has
      > **no production caller**. Its only callers are convert.rs's own 17 tests, the
      > `#[cfg(test)]` use in `materialize.rs`'s `layer()` helper (`:495`), and two
      > **locally-shadowed same-name test helpers** at
      > `quarto-core/src/project/project_profile.rs:639` and `render_scripts.rs:712`
      > whose bodies call **pampa's** converter, not this one. The plan is right that
      > it is dead API; only the disposal was unbuildable. Leave those two helpers
      > alone — they are unrelated despite the identical name, and they are exactly
      > what makes a grep-based audit here reach the wrong conclusion.
- [x] **Preserve the serialized wire shape.** `ConfigValueKind`'s
      `Serialize`/`Deserialize` are hand-written
      (`quarto-pandoc-types/src/config_value.rs:222`, `:305`) and the `Scalar`
      arm emits `{"Scalar": <value>}`. Keep that: drop provenance on
      serialize, `None` on deserialize. No `profile_version` bump is needed
      (`ConfigValue` is embedded in `DocumentProfile`, equality enforced at
      `document_profile.rs:842`). The loss is silent, so confirm no
      serialization boundary sits between producer and consumer — measured,
      none does: `pampa/src/readers/json.rs` constructs `Scalar` only on the
      Pandoc-JSON *input* path (`:2719-2867`), and the Lua bridge is Lua
      tables, not JSON. **Record that confirmation in § Evidence**

**The binding regression — do not ship Phase 3 without this.**

Measured by the Plan 3 session and re-verified here; the numbers are in
§ Evidence, so this does not depend on that session being consultable.
Threading keeps `ConfigValue.source_info` contiguous, but the *diagnostics*
produced by the re-parse take `content_source_info` as their parent, so their
locations are `Substring{parent: Concat}` for a multi-line block scalar.
Per-page diagnostics are bound at print time by `attach_config_source`
(`quarto/src/commands/render.rs:1184`, called at `:1270`) →
`bind_config_source` → `bind_source_candidates`, whose first statement is
`info.resolve_byte_range()?` (`quarto-core/src/config_sources.rs:90`) —
`None` for anything `Concat`-backed. The config file is then never registered
and the diagnostic renders with **no source snippet at all**. Today those two
`Q-2-9` warnings print a snippet with carets on the wrong line; after Phase 3
they would print with no snippet. That is a regression on the **block-scalar**
fixture (§ Evidence) — not the ariadne crash repro, which is a different file;
this plan uses "founding" for both and they are not interchangeable.

> **CORRECTED 2026-08-22, BY MEASUREMENT — the paragraph above is wrong about the
> symptom, and the error is load-bearing enough to fix in place rather than footnote.**
>
> The refusal is real: `resolve_byte_range()` genuinely returns `None` for these
> `Substring{parent: Concat}` locations, and `bind_source_candidates` genuinely
> registers nothing. **But the snippet renders anyway, so there is no live
> regression** — the defect is latent and *masked*.
>
> Why: `MetadataMergeStage`
> (`quarto-core/src/stage/stages/metadata_merge.rs:308-352`) **unconditionally**
> pre-registers `_quarto.yml`'s content into both `doc.ast_context.source_context`
> and `doc.source_context`, keyed by `quarto_yaml::file_id_for_filename` — the same
> hash-based `FileId` scheme the binder matches on — *earlier in the pipeline*, and
> for every candidate `attach_config_source` is actually given. That registration
> survives into `RenderOutput.source_context` (`pipeline.rs:870` →
> `stage/data.rs:453`) and seeds the coalesced group (`render.rs:1258`), so
> `ctx.get_file(file_id)` succeeds at print time regardless of the binder.
>
> Verified twice, mechanically, not by re-reading: reverting the fix makes a focused
> unit test against `bind_source_candidates` fail (`left: None, right:
> Some(.../_quarto.yml)`) while **the CLI fixture passes identically before and
> after** — direct proof that a CLI-level test cannot distinguish the two
> implementations here.
>
> Two consequences for anyone reading this section:
> 1. **Do not write the CLI fixture as red/green proof for this item** — it is
>    vacuous by masking, a third mechanism on top of the two this plan already warns
>    about (project-level fixture; asserting diagnostic-presence). The red/green
>    belongs in a unit test against the binder. The CLI fixture is still worth
>    keeping as a standing guard for the day the pre-registration is narrowed.
> 2. The fix still stands on its own merits, independent of the symptom: the binder
>    only ever wanted the id (it discarded the range), and `root_file_id()` is what
>    the **renderer** already uses (`quarto-error-reporting` `diagnostic.rs:819`,
>    `:1022`), so this makes binder and renderer agree instead of disagreeing.
>
> Also corrected: the candidate list at both real call sites (`render.rs:789-793`,
> `:884-888`) is `config_path + profile_config_paths + extension_manifest_paths`
> only. **`dir_layer` paths (`_metadata.yml`) are never passed to
> `attach_config_source`** — `MetadataMergeStage` registers them but nothing here
> ever matches them.
>
> The overlap between the two registration mechanisms — neither referencing the
> other, so narrowing one silently changes the other — is filed as **bd-8k41zq68**.

- [x] Change `bind_source_candidates` to obtain the file id from
      `info.root_file_id()` rather than `info.resolve_byte_range()?`. It
      discards the range anyway and wants only the id, and `root_file_id`
      handles both `Concat` and `Substring{parent: Concat}`
      (`quarto-source-map/src/source_info.rs:521-532`). This also makes the
      binder agree with the **renderer**, which already resolves the file via
      `root_file_id()` (`quarto-error-reporting/src/diagnostic.rs:819`,
      `:1022`) — today they disagree about how to obtain the same value
- [x] **Test on the right population.** `project_diagnostics` are
      pre-registered unconditionally by `config_source_context`
      (`render.rs:1172`, printed at `:1246-1248`) and need no binding, so a
      project-level fixture comes back green and proves nothing. The
      regression test needs a **per-page** config diagnostic from a multi-line
      block scalar — the same seam as the block-scalar test above
- [x] Scope check, **recorded in § Evidence**: only this one call site is
      exposed. The other binders act on `ConfigValue` spans, which stay
      contiguous. `rebase_source_candidates` (`config_sources.rs:140`, used
      from `website_post_render.rs`) is the exception — it genuinely needs the
      range to rebuild an `Original{fid,start,end}`, so `root_file_id` does not
      help it; it is on the inert path (see the ruled-out list below) and stays
      as is

**The two `None`s (obligation 7).**

- [x] `content_source_info()` returning `None` on a node q2 has already
      established is a string scalar is a **bug** — Plan 1 merged "not a
      scalar" and "derivation desynced" into one `None`, and both are bugs at
      that call site. Report it, but **warning-level and non-fatal**: Plan 1
      rejected `Err` precisely because a walker bug must not turn a working
      render into a hard failure, and a wrong caret beats no output

      > **CORRECTED 2026-08-22 — this counts TWO causes of `None`; the published
      > accessor documents THREE, and the missing one is not a bug.**
      >
      > `quarto-yaml` 0.1.3's `content_source_info()` doc comment
      > (`yaml_with_source_info.rs:192-216`) states `None` means, verbatim: *"this
      > node is not a scalar (ask `is_scalar` to tell that apart); **no derivation
      > ran (the node was built by hand, e.g. in a test, or is an unresolved
      > alias)**; or the lockstep derivation desynced."* The middle cause is absent
      > from the item above and is legitimate.
      >
      > - The **unresolved-alias** half is self-resolving and needs no special case:
      >   an unresolved alias is `Yaml::Alias`, not `Yaml::String`, so the
      >   `Yaml::String` scoping already excludes it (existing fixture:
      >   `pampa/src/pandoc/meta.rs:717`).
      > - The **hand-built-node** half is real but test-only. Measured: q2 has 17
      >   hand-constructed `YamlWithSourceInfo` sites, of which these build a
      >   `Yaml::String` and will trip a naive rule —
      >   `quarto-config/src/convert.rs:116` (the `make_scalar` helper, so every test
      >   using it), `convert.rs:197`, `pampa/src/pandoc/meta.rs:501`, `:517`, and
      >   the four `new_scalar_with_tag` string fixtures at `meta.rs:727`, `:747`,
      >   `:760`, `:773`.
      >
      > **Decided: absorb this in the FIXTURES, not in the rule.** Where a hand-built
      > string-scalar fixture trips the warning, attach provenance with
      > `with_content_provenance`, or assert the warning where the test is
      > specifically *about* the `None` path. **Do not** weaken the rule to "only warn
      > when we can prove a parse happened", and do not add a came-from-a-real-parse
      > flag. Production never hand-builds these nodes, so the rule is right for
      > production traffic and the noise is confined to those fixtures — which today
      > pair a decoded value with a synthetic `SourceInfo::for_test()` span anyway,
      > exactly the shape `span_assert.rs`'s module docs call
      > invisible-by-construction.
      >
      > **CORRECTED 2026-08-22 (session 2), by review: an earlier revision of this
      > paragraph said "fixing them is a fidelity gain and the warning is the forcing
      > function". WITHDRAWN — the instruction stands, the rationale does not.** What
      > landed attaches `SourceInfo::for_test()` *as* the content provenance, which
      > silences the warning at exactly the synthetic fidelity this paragraph
      > criticizes. In a real `Yaml::String`, content provenance **differs** from the
      > container's `source_info` — quote delimiters and block-scalar indent stripped,
      > which is the entire point of this epic — and a second, independently-synthetic
      > span does not reproduce that relationship; it makes the field `Some(garbage)`
      > rather than `None`. If anything `None` was the **more** honest signal for a
      > hand-built node: it is the literal truth that no derivation ran.
      > The instruction is still right, for a narrower reason: the warning exists to
      > police **production** traffic, none of the touched fixtures asserts anything
      > about `content_source_info`'s value or resolves its span, so the synthetic
      > value has zero blast radius — and the real rigor lives in the four dedicated
      > tests, which build `None` directly. **Do not reuse the "fidelity gain"
      > framing, here or in Plan 3.**
      >
      > Also required, or the scoping is untested: assert the **negative** — a
      > non-string scalar (`Integer`/`Boolean`/`Null`) with `None` provenance must
      > **not** warn. Without that, a later widening of the rule goes unnoticed, and
      > the scoping is the whole reason this is a plain internal diagnostic rather
      > than a catalog code.
- [x] **No `Q-` code for that report.** Decided here: it is an internal
      consistency failure a user cannot act on, so it gets a plain internal
      diagnostic rather than a catalog code. This is not only taste —
      `cargo xtask lint`'s `error-docs-page-missing` and
      `error-docs-sidebar-unlisted` would then require a
      `docs/errors/<subsystem>/<code>.qmd` page **and** an in-code-order
      sidebar entry in the same commit, mechanically enforced
- [x] `ConfigValueKind::Scalar { content_source_info: None }` at the
      `ConfigValue` layer is **not** a bug — CLI `-M`, Lua and defaults-file
      metadata have no YAML origin. Falling back to `source_info` is inert
      there *because* those producers carry
      `SourceInfo::generated(By::programmatic_config())`
      (`quarto-core/src/stage/stages/metadata_merge.rs:48`, `:71`, `:460`,
      `:463`), where offset arithmetic already yields `None`. **Put that
      reason in a code comment**, so the fallback is never extended to
      YAML-rooted values
- [x] Note the degradation path in the same comment, so it is not mistaken for
      a bug later: `UserFiltersStage::pre()`
      (`quarto-core/src/pipeline.rs:349`) runs before `ConfigMarkdownTransform`
      (`:1176`), and the Lua bridge discards provenance outbound
      (`pampa/src/lua/config_value.rs:150`) and rebuilds `Scalar` without it
      inbound (`:324-341`). A filter that touches `website.page-footer`
      therefore drops to today's behavior — a caret one byte left, not a
      crash. This is the safe direction, and a direct consequence of putting
      provenance *inside* the variant

**The third consumer.**

- [x] Fix `caption_inlines` (`quarto-core/src/crossref/codeblock_shorthand.rs`,
      fn at `:644`). `OptionValue.value_source` is set at `:551` from
      `entry.value.source_info` — a live quarto-yaml node — and used as the
      offset base for markdown-parsing the decoded value, so
      `fig-cap: "A *strong* claim"` drifts exactly like the config path. It
      needs no carrier: Plan 1's `content_source_info()` serves it directly.
      **It is also the only path that exercises a `Concat` parent**: cell
      options hands `parse_with_parent` a `SourceInfo::concat`
      (`quarto-core/src/cell_options/mod.rs:227-229`), so content provenance
      there is a Concat *inside* a Concat-parented parse. That is the case
      Plan 1's builder contract ("never call `resolve_byte_range` on the
      parent") exists for — test it here
- [x] Ruled-out sites, **listed in § Evidence** so nobody re-investigates:
      `project_resources.rs:215/224` (whole-node span, `as_plain_text`, no
      sub-offset arithmetic), `use_cmd/config.rs:229` (already defensive — see
      Phase 4), `lua/config_value.rs:620` (base is `filter_source_info(lua)` =
      `Generated` with no anchors — Plan 3), `theme_diagnostic.rs:331`
      (synthetic test span), `project/website_post_render.rs:217` (same shape
      as the `config_markdown` reader, on the same page-footer data, but its
      parse diagnostics are discarded and only image URLs are collected —
      inert today, and a latent second reader the moment anyone surfaces them)

### Phase 4 — q2's attribute path drives the builder

This is what proves the builder is general rather than YAML-shaped. It gives
`ProvenanceBuilder` a **second real consumer** in the same cycle it ships.

**Decided:** `AttrSourceInfo.attributes[i].1` **changes meaning** to content
provenance. No sibling field, no widening of the struct. Every Rust consumer
wants content — `callout.rs:427` (re-parse base), `theorem.rs:345` and
`proof.rs:182` (a span for a `Str` whose text is already decoded), and
`llms.rs:374-379`/`:1082` only filters the vec in parallel with `attr.2`.

**A note on when this is testable.** § Risks calls a `callout.rs` failure here
a stop-and-fix-Plan-1 signal — but by Phase 4, `ProvenanceBuilder` has already
shipped as `quarto-source-map` 0.1.3, so "fix Plan 1" means another release.
Drive `unescape_punctuation` against a local path override **during Plan 1's
Phase 2 development window**, which already runs under a `quarto-source-map`
path patch and already expects the API to change before it publishes 0.1.3.
*Not* during Plan 1's Phase 1 design-review item, as an earlier revision said:
that item is explicitly "on paper before writing it", and the builder does not
exist yet at that point — you cannot drive a decoder against an API that has
not been written.

- [x] Failing test first: a div attribute whose value contains a collapsed
      escape, asserting an inner node's `SourceInfo` resolves to the true byte
      position. Measured baseline from `bd-mxa44voa`: for
      `title="Use \`renv\` today"` the code span sits at inner bytes `4..10`,
      maps to `85..91`, and is actually at `86..92` — off by one *before* any
      escape is involved, and one more byte per collapsed escape
- [x] Drive `ProvenanceBuilder` so `AttrSourceInfo` carries content
      provenance. **The plumbing is larger than "drive it from
      `unescape_punctuation`."** That function
      (`pampa/src/pandoc/treesitter_utils/text_helpers.rs:41`) is private,
      takes a `&str`, and has exactly one caller — `extract_quoted_text` at
      `:28`, which discards offsets. `extract_quoted_text` in turn has **two**
      callers: the div-attribute value at `treesitter.rs:1208-1213` and the
      link/image `title` at **`:1301-1305`** — both producing
      `IntermediateBaseText(String, Range)`
      (`treesitter_utils/pandocnativeintermediate.rs:26`), and a
      `quarto_source_map::Range` is a start/end pair that **cannot express a
      `Concat`** — the same argument Plan 1 uses to reject the content-span
      design. The `SourceInfo` is not built until
      `commonmark_attribute.rs:44-49` (`SourceInfo::from_range`). So either the
      intermediate carries a `SourceInfo`, or the decoder returns a piece list
      bound later; `ProvenanceBuilder::in_file` exists for this shape.
      **Done when** the failing test above goes green — this item is the
      plumbing analysis behind it, not a separately checkable deliverable
- [x] **A third decoder shadows the identifier — out of scope, and it is not a
      defect.** `treesitter.rs:989` defines a **local closure also named
      `extract_quoted_text`** which open-codes the same strip-and-unescape for
      `shortcode_string`, feeding `IntermediateBaseText` at `:1006` to
      `process_shortcode_string` (`treesitter_utils/shortcode.rs:31`).
      It **cannot drift, by construction**: that function destructures the
      closure's range away — `let ... IntermediateBaseText(id, _) = ...` at
      `:36`, verified — and (per Plan 3, who traced the downstream half)
      recomputes the range from the whole node, with no
      `ShortcodeArg::String` consumer offsetting into it. So the closure's
      range arithmetic at `:1000-1005` is dead code; Plan 3 owns deleting it.
      Unlike the `title` caller it is also **not** reached by changing the
      shared function's return type, so it is separable on both counts.
      Recorded because a Phase 4 implementer greps `extract_quoted_text`, finds
      two definitions, and needs to know which one this phase means — and that
      the other one is not a bug they are declining to fix
- [x] **The `title` caller is in scope.** Decided here rather than left open:
      changing `extract_quoted_text`'s return type reaches
      `treesitter.rs:1301-1305` whether or not you use the provenance there, so
      the only real choice is use-it-or-discard-it, and discarding would
      deliberately leave a known-wrong span behind a change that already
      touched it. It is a **span-tightening, not a drift fix** — measured, the
      only consumer of `TargetSourceInfo.title`
      (`quarto-pandoc-types/src/attr.rs:121`) is the JSON writer's source-ref
      stream (`pampa/src/writers/json.rs:2079`), which does no sub-offset
      arithmetic, so today's span is quote-inclusive rather than drifting —
      the same shape as `theorem.rs`/`proof.rs`. Expect the same kind of
      fallout: any assertion pinning a link or image title's span moves.
      **It does not widen the 0.2.0 break** — measured, `annotated-qmd` reads
      `attrSource.kvs[i]` and never reads target/title spans, so the TS
      consumer is untouched by this half
- [x] **Obligation 8: re-check `qmd-syntax-helper` after the meaning change,
      and record the result.** § Risks rules it out *by reachability* — its
      diagnostics come from its own `pampa::readers::qmd::read` call, so it
      never sees this provenance. That conclusion is mine, it is expected to
      hold, and **nothing currently re-tests it**. It matters because its 23
      `start_offset()` sites across 22 files **write to the user's files**
      (`conversions/q_2_33.rs:74-75`, `attribute_ordering.rs:74`,
      `div_whitespace.rs:77`, …) using the accessor that is silently `0` on a
      `Concat`, and this phase changes what `attributes[i].1` means. Failure
      mode is **corrupted source files, not a bad caret** — the worst in the
      epic. Concretely: after the swap, confirm no diagnostic reaching
      `qmd-syntax-helper`'s conversions carries an attribute-derived span, and
      write that confirmation into § Evidence rather than re-deriving the
      reachability argument
- [x] **Tag verbatim by bytes, not by length.** Plan 1's walker had this bug
      and fixed it; our decoder must not reintroduce it. A piece is verbatim
      **iff its source run is byte-identical to its content run** — equal
      lengths are not sufficient, and a 1→1 piece with differing bytes tagged
      verbatim claims a byte-identity it lacks, which `preimage_in` would let
      the incremental writer act on by Verbatim-copying the wrong bytes. Only
      verbatim pieces coalesce. Zero-content pieces are **stored, never
      dropped** — reversed by Plan 1 on 2026-08-21 after measuring that
      dropping one leaves a source gap and `preimage_in` then returns `None`
      where storing gives `Some(4..14)`. Storing is what keeps the tiling
      gap-free, which the hull below depends on. Our decode is simpler than
      YAML's — `unescape_punctuation`'s cases are `\X`→`X` (2→1) and a
      preserved `\Y` (2→2, byte-identical), so no 1→1 non-identical case
      exists today — but emit pieces from the decode rather than inferring
      tags from lengths, so it stays true if the escape table grows

      > **WARNING added 2026-08-22 — do NOT apply "zero-content pieces are stored,
      > never dropped" to the stripped QUOTES. Doing so silently defeats this phase.**
      >
      > That rule comes from Plan 1's YAML walker, where dropping a zero-content piece
      > opened an *interior* source gap and made `preimage_in` return `None`. It is
      > correct there. Applied here it is tempting — emit `replacement(0..1, 0)` for
      > the opening quote and the same for the closing one — and wrong: a stored
      > **trailing** zero-content deletion puts the closing quote's source end at the
      > end of the last piece, so the hull's exclusive end lands *after* the closing
      > quote, re-including the quotes in the very span this phase exists to tighten.
      >
      > The correct rule for this case is the one this plan already states elsewhere:
      > quote stripping **trims the content range's ends**; it does not leave an
      > interior gap. Start the tiling after the opening quote and stop before the
      > closing one. Checked against the published builder: `finish()`'s
      > `debug_assert` requires only that *consecutive* pieces abut **each other**,
      > not that the first piece starts at the node's first byte — so this tiles
      > legally.
      >
      > Sanity check to actually run: for a quoted value with **no escapes at all**,
      > the resulting span must cover the value **without** its quotes. And note the
      > corollary — this plan predicts `theorem.rs`/`proof.rs` and link/image `title`
      > spans will "tighten to exclude quotes"; that prediction only holds if the
      > quotes are left out of the tiling as above. If those spans do **not** tighten,
      > this is the first thing to check.
- [x] **Delete the `callout.rs:431-447` workaround** — the
      `match value_source.resolve_byte_range()` block. Note the criterion
      precisely: the *function* `attribute_value_source` (`:401-448`)
      survives. It also does the key-index lookup (`:409-411`), the `Attr.2` ↔
      `AttrSourceInfo.attributes` positional-alignment guard for the
      duplicate-key bug bd-3aolj / bd-1e6a5 (`:413-425`), and the
      `Option` → `generated()` fallback. Only the length-arithmetic block is
      the provenance workaround. If *that* block cannot go, the builder is not
      general enough — stop, and see the note above about when this is
      testable
- [x] **Update the fallout, don't just expect it.** These are work items, not
      predictions: `callout.rs:718`, `:734`, `:750` call `resolve_span` on
      attribute-derived inline spans (their disposition follows the Phase 3
      `resolve_span` decision — they resolve if piecewise lands, else they need
      rewriting); `quarto-ast-reconcile/src/remap.rs:526`'s test-only
      `file_id_of` panics on non-`Original` and must accept the new shape; and
      `theorem.rs`/`proof.rs` spans tighten to exclude quotes, moving any
      location assertion over a theorem or proof `name=`. Reconcile itself is
      clean — `remap_file_ids` is variant-complete
      (`quarto-source-map/src/source_info.rs:485-511`)

**Obligation 8 — the TypeScript boundary.**

`AttrSourceInfo` is serialized as `"a"` (`pampa/src/writers/json.rs:694-708`)
and read by `ts-packages/annotated-qmd/src/block-converter.ts:287` and
`inline-converter.ts:322`, which pull `attrSource.kvs[i]` and resolve it
through `sourceReconstructor.getAnnotatedParseSourceFields`.

**Corrected diagnosis (an earlier draft of this plan had this wrong, and the
error was relayed to Plan 1 — do not reinstate it).** The earlier claim was
that `getSourceLocation` reads the serialized `r` field and therefore hands TS
a range starting at byte 0. It does not: `getSourceLocation`
(`source-map.ts:150-157`) delegates to `resolveChain`, whose `case 2: //
Concat` (`:317-375`) **already walks the pieces** — it builds the concat via
`toMappedString(id)` and returns the range from `map(0)` and
`map(len-1) + 1`. `info.r` is used only on error and empty-concat paths. So a
well-formed `Concat` resolves correctly today, and TS is already doing the
`map_offset`-pair thing this plan prescribes elsewhere.

**The real defect is `resolveChain`'s `Substring` arm** (`:301-315`):

```ts
range: [parentStart + localStart, parentStart + localEnd]
```

— affine composition over the parent's resolved range, which is precisely the
`preimage_in` bug Plan 1 fixes in Rust, in TypeScript. And
`Substring{parent: Concat}` is exactly the shape these value spans take once
provenance is correct, so this arm is where the work is.

**Decided 2026-08-21: TS moves to content semantics.** The quote-inclusive
behavior is the bug, not the contract. `@quarto/annotated-qmd` is a published
public package (`publishConfig.access: public`, v0.1.1), so this is a breaking
behavior change and gets a minor bump.

- [x] Fix `resolveChain`'s `Substring` arm to compose through the parent's
      mapping rather than affinely over its hull
- [x] Update `ts-packages/annotated-qmd/test/block-types.test.ts:428-437`,
      which today asserts `source.substring(value.start, value.end)` ∈
      `['"42"', '"test"']` with the comment "values include quotes in source".
      It must assert the **unquoted** value. This test is the reason the
      change is breaking rather than invisible
- [x] Regenerate the committed `ts-packages/annotated-qmd/examples/*.json` —
      they are Rust JSON-writer output carrying the `kvs` ids and ranges
      (`examples/README.md:33-35`), so Phase 4 makes them stale. Note the
      README's regeneration command names `--bin quarto-markdown-pandoc`,
      which no longer exists; the binary is `pampa`. Fix the README in passing
- [x] Bump `@quarto/annotated-qmd` to **0.2.0** — the 0.x signal for a
      breaking behavior change
- [x] Add a permanent test pinning the range for a quoted value and for an
      escaped value, and record the pre-fix range in § Evidence.
      **The runner is `node --import tsx --test test/*.test.ts`**
      (`package.json:30`) — this package has no vitest dependency and no
      vitest config; earlier drafts of this plan said "vitest" and were wrong
- [x] **Gap-free tiling is a precondition, not an incidental.** Measured: a
      gappy `Concat` has no single range at all. Our decode is gap-free —
      `\X`→`X` consumes the 2 source bytes it replaces, and quote stripping
      trims the content range's ends rather than leaving an interior gap — but
      state it as a requirement on the decoder, because a dropped zero-content
      piece would open a source gap and silently remove this remedy's
      precondition

- [x] **DECLINED, routed to Plan 3 (decision R-8, taken in Phase 7).**
      `use_cmd/config.rs:229` (`scalar_value_span`, verified still at that line)
      can be simplified to repoint declarations it currently refuses: a gap-free
      `Concat` does have a hull, obtainable from the `map_offset` pair. (Not from
      `preimage_in` — its `Substring` arm composes affinely,
      `source_info.rs:448-455`; Plan 1 makes it return `None` for a `Concat`
      parent in 0.1.2, so this is compiler-visible rather than a trap.) The
      function is correct today, merely limited — its cost is a *refusal* to
      repoint the declaration, not wrong output.
      **Deciding reason:** replacing the byte-comparison with a
      content-provenance read would add a **new consumer of content provenance
      after Phase 6's audit has run**, so it would exit Plan 2 unaudited by the
      very matrix built to catch that class. Declining is also
      disposition-preserving: § Workarounds that collapse says "Plan 3's Phase 7
      asserts these three dispositions; tell that session if any of them
      changes" — leaving it **kept** means that assertion still holds unchanged

### Phase 5 — panic boundary (`bd-chmbr0zl`)

Hardening, not a live defect. The specific panic is fixed; this addresses the
**class**. A diagnostic render that aborts an already-successful render, and
discards every diagnostic queued behind it, is disproportionate however the
bad offset arose.

Verified prerequisites, so the phase does not rest on assumption:

- `panic = "unwind"` holds. No profile in the root `Cargo.toml` sets
  `panic = "abort"` (`[profile.release]` sets only `strip`), and wasm32 sets
  `-C panic=unwind` explicitly in `.cargo/config.toml` for the Lua shim.
- `AssertUnwindSafe` is probably unnecessary. Neither **borrowed type** has
  interior mutability: `SourceContext`/`SourceFile`
  (`quarto-source-map/src/context.rs:11-33`) are plain data, and
  `CoalescedDiagnostic` holds no cells. The one exception, checked and
  discounted: `quarto-error-reporting` has
  `static CATALOG: OnceLock<Box<dyn CatalogProvider>>`
  (`src/catalog.rs:67`, `use` at `:15`) — a global static, not a field of
  either type we borrow across the boundary, so it does not make
  `&CoalescedDiagnostic` or `&SourceContext` non-`RefUnwindSafe`.
- Prior art to match: q2 already wraps per-document renders in
  `catch_unwind(AssertUnwindSafe(..))` at
  `quarto-core/src/project/pass2_renderer.rs:499` and
  `orchestrator.rs:1868`. They did not catch this panic because it fires in
  `print_render_diagnostics`, on main, after the render.

- [x] Add an env-gated fault-injection hook to q2's diagnostic-emission loop,
      `cfg(debug_assertions)`-gated so it **cannot** be armed in a release
      build
- [x] Failing test first: with the hook armed, assert **exit code 0**, that
      the *other* queued diagnostics still print, that `_site/` is still
      written, and that stderr carries an explicit
      `internal error rendering diagnostic <CODE>` line — the last of which is
      also what makes "surface loudly, do not swallow" testable rather than
      aspirational. Observe red. Note the default panic hook prints
      `thread '…' panicked at …` to stderr before `catch_unwind` returns — the
      assertion must tolerate it, and in production it will be there too,
      which is the intent
- [x] Implement `catch_unwind` around the **per-diagnostic** render at every
      site — **EIGHT sites, not five**, named by *function* rather than by line
      number (the line numbers this item originally carried have all drifted;
      counting by function is stable):
      **three in `print_render_diagnostics_text`** (coalesced pass-2 structured
      failures; the `project_diagnostics` loop; the coalesced per-page loop, with
      `attach_config_source`'s `&mut` mutation deliberately left *outside* the
      guarded closure) and **five in `print_render_diagnostics_json`** (pass-1
      failure diagnostics; the pass-2 failure with **no** structured diagnostics;
      the pass-2 failure **with** structured diagnostics; `project_diagnostics`;
      the per-page successful-render outputs). This item's original text —
      "`:1356` and `:1394` plus the project-level ones below them" —
      **undercounted the project-level JSON ones by one**. The JSON path reads
      `.column` and cannot panic today — that is an argument, not an exemption.
      Verified in the landed tree: `grep -c 'render_diagnostic_guarded('` = 8,
      3 above `print_render_diagnostics_json` and 5 below it.
      **Updated 2026-08-23 (Plan 3 Phase 6d): the count is now 9.** Plan 3 wrapped
      the ninth site, the pre-render `to_text(None)` loop over config-sourced
      diagnostics in `execute_project` (§ Hand-off item 5 / recommendations § 7);
      it is `render_diagnostic_guarded` for **uniformity**, not a fix — that path
      passes `ctx = None`, which never reaches a renderer, so it cannot panic
      today. **Carry the pattern, not just the number:** the count is of
      `render_diagnostic_guarded(` **with the trailing paren**. Grepping without
      it returns **12** on the same tree: the 9 call sites, plus the definition
      `fn render_diagnostic_guarded<T>(` (`:1290` — the tight grep misses it
      because of the `<T>`), plus two prose mentions in the fault-injection
      seam's doc comment (`:1228`, `:1232`). A reader who runs the looser grep
      will get 12 and "correct" a correct record. (An earlier revision of this
      note said 11; that was the pre-wrap figure, when there were 8 call sites.
      Re-measured on HEAD 2026-08-23.)
- [x] Verify the `UnwindSafe` obligations and **write the outcome into
      § Evidence**, including "not needed" if that is what it turns out to be.
      **Outcome: not needed** — see § Evidence, Phase 5
- [x] Assert a caught panic while printing a **warning** does not change the
      exit code. Sequencing already favors this:
      `print_render_diagnostics` runs after `_site/` is written and before
      `should_exit_nonzero` (`render.rs:836`/`:1010`, `:1026-1028`).
      **Anchors stale as of 2026-08-23** (noted, not rewritten — the claim is
      still true, only its line numbers drifted): on HEAD the project path is
      `execute_project` `:854`, printing at `:1024`, gating at `:1041`; the
      single-doc path is `:770`/`:836`/`:848`, unchanged
- [x] Close `bd-chmbr0zl` (closed 2026-08-22; the close reason records all three
      of the strand's own "things to check" as discharged)

### Phase 6 — shadowing audit

Each fix removes the evidence for the one below it:

```
Plan 1 (correct provenance)  ──shadows──►  the char-boundary snap
the char-boundary snap       ──shadows──►  the panic boundary
```

That is shadowing **in the field**, not in the tests. The governing principle
is that **each level's test injects the defect at that level's own input
boundary**, so no upstream fix can remove it — which holds for Phase 3/4 (real
fixtures), Phase 5 (deliberate fault injection), and the snap's unit test
(literal offsets, never touching `map_offset`).

**Decision taken, recorded so it is not relitigated:** after Plan 1 lands, q2
has no known producer of mid-character offsets, so there is deliberately **no
end-to-end crash test** in q2. Coverage for the char-boundary snap lives in
`quarto-error-reporting`'s own unit test, which constructs a mid-char span
directly — a real bad input at that crate's real API boundary.

**Superseded:** an earlier draft accepted that the two `..._does_not_panic`
integration tests would become vacuous and treated their vacuousness as a
matrix observation. Plan 1's obligation 3 rejected that — it is the epic's only
regression coverage for its founding crash — and Phase 1 above re-anchors them
at unit level instead. The matrix below no longer expects a vacuous pass.

This whole phase is in-session verification (§ In-session verification,
instance 2); reverts cannot be permanent tests. **Every change this plan makes
needs a row** — the closing item demands each test be shown to fail, and a
change with no row exits the audit unbound.

This list originally carried **14 items covering 15 rows** (rows 7 and 8 were
bundled into one), plus three process items. The audit as executed ran **20
rows**; the five with no item here — 10, 12, 14, 15, 19 — are added below as
their own items rather than left implicit, and row 8 is split out of row 7's.
Row numbers below are the audit's, and match `task-F-report.md`. See § Evidence,
Phase 6 for the full accounting.

- [x] **(row 1)** Revert **only the three re-parse bases** (`meta.rs:259/:303/:316`) →
      expect T4 (quoted column) and T6 (front matter) red, T5 still green.
      Split from the carrier revert below on purpose: bundled, the matrix
      cannot tell a half-done Phase 3 from a complete one.
      **PREDICTION WRONG.** Only the front-matter test reddened; the
      project-config quoted test stayed green. The fix is bound; the *expectation*
      was not right
- [x] **(row 2)** Revert **only the carrier read** (`config_markdown.rs:284-290`) →
      expect T5 (block scalar) red, T4/T6 still green. Prefer both of these to
      reverting Plan 1's quarto-yaml fix: they bind the code we wrote and need
      no cross-repo override.
      **PREDICTION WRONG.** **Both** the block-scalar test and the
      project-config quoted test reddened — so the two halves of Phase 3's
      provenance work are cleanly attributable, just not the way predicted
- [x] **(row 3)** **Mutate, don't revert, for T7**: apply the content-provenance base to
      the plain-scalar path as well → expect the plain / single-line-block
      regression assertions red. T7 cannot be bound by reverting our code,
      because it asserts values that are already correct.
      Outcome: both zero-drift guards **are** bound, but the mutation as
      performed was **not selective** (row 3's own label is "NO"); the guards'
      discrimination was supplied instead by row 2, under which both stayed
      green. Finding: the two guards sit on **different code paths**
      (`meta.rs`'s `markdown_base` vs `config_markdown.rs`'s `base`), so a
      mutation at one cannot redden the other
- [x] **(row 17)** Revert the `Location.offset` floor only → expect **T2 (the zero-width
      label test, in `quarto-error-reporting`)** red, in addition to
      `quarto-source-map`'s own tests. T2 lives in a different repo from the
      hunk that binds it, so it needs naming here or it is audited by nobody
- [x] **(row 4)** Revert the `bind_source_candidates` → `root_file_id()` change only,
      **keeping** the provenance swap → expect the binding regression test
      red. Reverting the swap instead makes it pass vacuously, since there is
      then no `Concat` to refuse. Confirmed, on the **unit** witness
      (`binds_a_concat_backed_source_info`); the CLI fixture
      `block_scalar_config_diagnostic_binds_concat_backed_source` passes in both
      states, which its own doc comment already discloses
- [x] **(row 5)** Revert the `caption_inlines` fix only → expect its `fig-cap` assertion red
- [x] **(row 6)** Revert the `resolve_span` change only (whichever of the two paths was
      taken) → expect the caret test red
- [x] **(row 7)** Revert the Phase 4 attribute swap only → expect Phase 4's Rust
      assertions red
- [x] **(row 8) The `callout.rs` deletion — a separate row from the attribute
      swap above, and ACCEPTED-UNBINDABLE BY CONSTRUCTION.** Split out because
      bundling it with row 7 hides the distinction: the attribute provenance
      swap **is** bound (nine reds), while the deletion of the
      `callout.rs:431-447` length-arithmetic block **cannot** be bound by any
      fixture. Given an escaped attribute value's provenance is a `Concat`,
      `resolve_byte_range` returns `None`, so the deleted block's `_` arm and its
      first branch both returned `value_source` unchanged in **all three** shapes
      the parser can produce (bare; quoted-no-escapes; quoted-with-escapes) —
      each measured by row 7's own reverts, not argued. That is a **different
      audit outcome from "unbound"**, and the closing rule below does not
      condemn it
- [x] **(row 13)** Revert the TS `Substring`-arm fix only → expect the annotated-qmd range
      test red
- [x] **(rows 14+15) The TS `Concat` case — PREDICTION WRONG, twice, and the
      instrument matters.** Two rows, run as two genuine commit-state reverts
      (`3efcb2c48^` = before `mapContentRange` existed at all, binding row 14's
      exclusive-end fix; `3efcb2c48` = the piecewise implementation without M-3,
      binding row 15's `lastPieceFileId` split). Ruling R-2 predicted the
      conflict-detection test would stay green under the whole-case revert — it
      reddened, because conflict detection *is itself part of* the piecewise
      rewrite being reverted. R-2 also held the two rows could not be separated
      without a hand-crafted patch — they separate cleanly, and per-test
      attribution was achieved
- [x] **(row 18)** Revert the char-boundary snap only → expect its **unit** test red
      (including the offset pair re-anchored into it in Phase 1). Red as
      predicted (`4..9` vs `3..9`) — **and** the same revert surfaced the audit's
      most important finding: the two `..._originally_mid_character_span` tests
      still render **without panicking** with the snap gone, so the snap's
      panic-prevention role is currently unwitnessed. Routed to Plan 3
- [x] **(row 20)** Revert the panic boundary only → expect the injection test red.
      3 of 4 red; `fault_injection_disarmed_by_default` correctly stayed green
      (it never arms the seam). **This row was missing from the audit's own
      brief** — the resolution artifact's un-rowed walk ended one commit before
      the audit's base, so nothing swept Phase 5 until the review caught it
- [x] **(row 9)** Revert the **desync warning report** only → expect its test red. It is
      injectable (feed a scalar whose provenance derivation is stubbed to
      `None`), therefore revertible, therefore it needs a row like everything
      else
- [x] **(row 11)** Revert the **verbatim-by-bytes tag rule** only (tag by length instead) →
      expect a test asserting a non-identical 1→1 piece is *not* verbatim to go
      red. If no such case exists in our escape table, say so here and mark
      this row accepted-unbindable rather than silently dropping it.
      **No such case exists** — re-read from `unescape_punctuation`'s current
      escape table: every 1→1 piece is trivially byte-identical, and the only
      non-identical case (`\X`→`X`) is 2→1. **Accepted-unbindable**; the rule
      stays prospectively load-bearing for any future 1→1 non-identical escape
- [x] **(row 16)** Revert the `Concat` exclusive-end fix only → expect the
      `qmd_writer_source_info.rs` assertion added in Phase 2 red
- [x] **(row 19) NEW ROW — the incremental-rebuild hash's provenance exclusion
      had no test that could catch its regression.** `hash.rs`'s
      `hash_config_value_kind` `Scalar` arm deliberately excludes
      `content_source_info`, and every `hash.rs` fixture reaches it through
      helpers that hard-code `content_source_info: None` — so the exclusion was
      unobservable by any existing test, including its nearest neighbour
      `meta_hash_excludes_source_info_and_key_source` (which stays green under
      the mutation). Now bound by a committed test,
      `meta_hash_excludes_scalar_content_provenance` (`992813188`).
      **Finding that came with it:** the specified mutation
      (`content_source_info.hash(hasher)`) **does not compile** —
      `quarto_source_map::SourceInfo` does not implement `Hash`, an
      undocumented **type-level guard** on the invariant. The row was bound with
      the nearest compiling mutation (hashing the provenance's `Debug` form)
- [x] **(rows 10 and 12) Two further un-rowed changes the audit added:**
      row 10, the non-string scalar carrying `content_source_info: None`
      (mutation row — bound); row 12, the link/image `title` leg added in D1's
      fix round (revert row — bound). Both matched expectation
- [x] **Where each revert runs, and what it costs.** The matrix spans three
      repos plus the TS package, so say it plainly rather than discovering it
      mid-audit: the provenance, binder, caption, `resolve_span`, attribute and
      TS reverts are in-tree; the snap revert runs in
      `~/src/quarto-error-reporting` alone; the floor and exclusive-end
      reverts are in `~/src/quarto-source-map` and require a temporary
      `[patch.crates-io]` override in whichever workspace runs the tests.
      Those overrides are scaffolding — Phase 7 confirms none survive
      (**confirmed** — see Phase 7's Evidence)
- [x] Record every observation in § Evidence — summarized there; the per-row
      observations live in full in `task-F-report.md` (1421 lines), which the
      Evidence section points at rather than transcribing
- [x] Any test that cannot be made to fail: fix it or delete it, and say which.
      **Which:** rows 8 and 11 are accepted-unbindable **by construction** (no
      discriminating input exists — not the same as unbound), and the two
      `..._originally_mid_character_span` smoke tests stay as smoke tests per
      Phase 1's explicit accepted-unbound carve-out. **No test in scope was
      found unbound-and-unexplained.** Row 19's invariant *was* untested; that
      is fixed and committed

### Phase 7 — land

- [x] Reconcile this checklist against reality before handoff — re-read it,
      verify each box against what actually landed, correct any that are
      wrong, and commit the updated plan. **Done; the corrections are listed in
      § Evidence, Phase 7.** This is the *third* reconciliation of this file —
      `b7c47195e` (session 1) and `fe0fb44b1` (session 2, through Phase 4) came
      before it — which is worth knowing: most boxes were already right, so the
      wrong ones were the easy ones to skim past
- [x] `cargo xtask verify` (full, not `--skip-hub-build`) green — the WASM leg
      matters here, because the `ConfigValueKind::Scalar` migration reaches
      `wasm-quarto-hub-client` through `quarto-pandoc-types`. **14/14 green,
      exit 0**; Rust tests **12919 / 198**. The previous full green was
      `1b6d30c08`, twelve commits earlier
- [x] `cd hub-client && npm run build:all`; and run the annotated-qmd tests
      directly with `node --import tsx --test test/*.test.ts` —
      `cargo xtask verify` *builds* ts-packages but does not run their tests.
      The hub-client build **is** covered, as verify's step 7 (WASM included) and
      step 8 (`test:ci`); the node suite was run separately: **161 / 161**
- [x] Confirm no `[patch.crates-io]` override for these crates remains, in any
      worktree used for the Phase 6 matrix — confirmed **by name** in all four:
      q2 root, the nested `wasm-quarto-hub-client` workspace, and both upstream
      checkouts (which have no patch section at all). **Neither lockfile moved**,
      checked *after* the full verify
- [x] Commit the plan. **`finishing-a-development-branch` is deliberately NOT
      run** — this item's original instruction is superseded: `feature/yaml-provenance`
      is the **shared integration line for Plans 2 and 3**, and Plan 3 has not
      started, so there is no branch to finish yet. Not pushed. The epic
      `bd-mxa44voa` stays **open** — Plan 3 is outstanding
- [x] Record the three open decisions this phase owed — **R-8** (`use_cmd/config.rs:229`:
      declined), **R-9** (`is_gapless`: handed to Plan 3 with C7's measurement),
      **R-10** (route the remaining findings) — each with its reason, and open
      § Hand-off to Plan 3 to carry them

## Risks

- **The fault-injection hook is a new permanent seam** in q2's diagnostic
  path. It must be impossible to arm in a release build.
- **`catch_unwind` is a no-op under `panic=abort`.** Verified unwind for every
  profile that matters (Phase 5), but re-check if a profile is ever added.
- **Phase 4 is the load-bearing test of Plan 1's API, and it runs after Plan 1
  has shipped.** If `callout.rs`'s length-arithmetic block cannot be deleted
  against correct provenance, the builder is not general enough — but by then
  fixing Plan 1 costs two more releases. That is why Phase 4 asks for the
  attribute decoder to be driven against a path override during Plan 1's
  design-review item.
- **Phase 4 breaks a published npm package.** `@quarto/annotated-qmd` v0.1.1
  is public, its documented invariant is
  `source.value.substring(start, end)`, and its own test pins quote-inclusive
  text. Moving to content semantics is deliberate (decided 2026-08-21) and
  carries a 0.2.0 bump, a test rewrite and regenerated example fixtures. This
  is the one place in this plan where getting it wrong produces a *wrong range
  in a shipped consumer* rather than a wrong caret.
- **The `ConfigValueKind::Scalar` migration is 206 non-test sites.** It is
  mechanical and compiler-guided, but it is the largest single piece of work
  here and the WASM target will not fail until `cargo xtask verify`.
- **`start_offset()` / `end_offset()` on a `Concat` stay unhardened, and that
  is a decision rather than an oversight.** They return 0 and the content
  length. An earlier round proposed a `debug_assert` in 0.1.2; Plan 1 declined
  and is right to. Those values are the *correct* answers in content space —
  `end_offset()` returning the content length is documented behavior — so an
  assert would fire on legitimate callers and cannot distinguish them from
  callers who conflated content space with file space. The defect lives at the
  call sites. Two live *drifting* Rust consumers are known:
  `bind_source_candidates` (fixed in Phase 3) and `use_cmd/config.rs:229`,
  which reads both and is safe only because of its read-back check. **Plus 23
  `start_offset()` reads across 22 files in `qmd-syntax-helper`**, ruled out by
  reachability rather than by inspection of each — and re-confirmed in Phase 4
  under obligation 8, because that crate writes to user files and this epic
  changes what the accessor is reading. (An earlier revision listed
  `getSourceLocation` here; that was a consequence of the retracted byte-0
  diagnosis — it is a TypeScript function and reads these accessors' *values*
  only through the serialized `r`, on error paths. See Phase 4.) Fix a third
  at its own call site if one appears.
- **Do not make `quarto-source-map`'s `Concat` arms consistent with each
  other.** Not our repo, but our Phase 3 removes one of the call sites that
  depends on the current asymmetry, so it is worth carrying. `resolve_byte_range`
  refuses (`None`) for a `Concat` while `preimage_in`, three functions away in
  the same file, returns a hull — and `resolve_byte_range`'s `Substring` arm is
  `parent_start + start_offset`, arithmetic **identical** to `preimage_in`'s.
  It is safe only because of that refusal, not because its arithmetic is
  better. Anyone "fixing" the apparent oversight by teaching
  `resolve_byte_range` to return a hull converts every one of its call sites
  from safe to silently wrong in a single commit, with no test failing.
  **The reverse direction is worse.** After 0.1.2, `preimage_in` refuses for a
  `Substring{parent: Concat}`, so git history shows it "used to" answer and
  restoring the hull reads as undoing an over-conservative change rather than
  proposing a new behavior — it arrives disguised as a revert. Its tell,
  per Plan 1: if the argument is *"we only return the hull when every piece is
  length-matched"*, that is a withdrawn proposal verbatim, and the
  counterexample is the 1→1 fold. Plan 1 records both directions as an explicit
  do-not; the membership test for the whole defect family is "someone wrote `+`
  over a parent willing to hand back a flattened range," which is why
  `map_offset` — which recurses instead of flattening — is not a member.
- **Two of this plan's verifications are unrepeatable.** If the Phase 1
  in-session revert is skipped, the binding evidence for the ariadne crash
  regression test is gone permanently — the floor makes it unreproducible.

## Hand-off to Plan 3

Decided in Phase 7 (decisions R-8, R-9, R-10). Everything below is **out of Plan 2** and
belongs to `2026-08-20-provenance-3-audit-and-fix.md`. The epic `bd-mxa44voa` stays **open**
for exactly this reason.

The unifying argument for items 1 and 2: each would add or change a **consumer of content
provenance after Phase 6's audit has already run**, so doing it here would exit Plan 2
unaudited by the very matrix built to catch that class of defect. Neither is blocked on a
crates.io publish.

1. **`use_cmd/config.rs:229` (`scalar_value_span`) — the declined simplification (R-8).**
   Today it byte-compares the raw span's text against the decoded value and returns `None` on
   mismatch, so its cost is a **refusal** to repoint a declaration, not wrong output. A
   gap-free `Concat` does have a hull via the `map_offset` pair, so the simplification is
   available; it is declined here on the post-audit argument above, and because leaving it
   **kept** preserves the three-way disposition that Plan 3's own Phase 8 (renumbered 2026-08-23) asserts.
2. **Narrowing `is_gapless` to the queried sub-range (R-9), with C7's measurement attached.**
   `resolve_span` refuses a real, common shape: a multi-line `#|` cell-options block is
   structurally gappy because `is_gapless` requires pairwise contiguity across *every* piece
   of the enclosing `Concat`, and each option line's `#|` marker sits in the gap. The
   post-audit argument applies **with more force** here, because the fix *is* content-space
   offset arithmetic — the class that has produced four bugs in this epic. Two facts to carry
   over: `is_gapless` is entirely **in-tree**
   (`crates/quarto-config/src/span_assert.rs:229`), so this is a **scheduling** choice and not
   gated on a fifth publish; and the blast radius is **test-only** (`span_assert` sits behind
   a feature enabled only in `[dev-dependencies]`), so no rendered caret is affected
   meanwhile.
3. **(a) The `q_2_28` splice-safety guard question.** `q_2_28.rs:59-61` is the one
   `qmd-syntax-helper` conversion reading the **`Ok`-arm** warnings — the channel that runs
   after attribute decoding — and it is an `end_offset()` reader. Safe today **only** because
   `Q-2-28` is a corpus-only code with no Rust emission site. A defensive "refuse to splice a
   non-`Original` span" guard is new work beyond obligation 8.
4. **(c) Whether `bd-g7qh1ltt` and `bd-49cbyqbt` belong inside the epic.** The first is
   `handleConcat` reconstructing the wrong *string* for any concat containing a replacement
   piece (root cause is the wire format; ranges are unaffected, `toMappedString` is public
   API); the second is attribute **key** ranges starting one byte early for any non-first
   attribute, in the key slot Phase 4 deliberately left raw.
5. **(d) The unguarded per-diagnostic `to_text()` at `render.rs:904`** — the **pre**-render
   loop over config-sourced diagnostics. The real reason this is safe is stronger than "nothing
   has been written yet": that call passes `None` as the `SourceContext`, and 0.2.2's
   `to_text_with_options` falls back to structured tidyverse text with **no source excerpt**
   when there is no location *or* no source context — so the byte-slicing path, the only known
   panic mechanism, is **structurally unreachable** there today. That is durable, and it tells
   the next author exactly what changes the calculus: **the day that call gains a real
   `SourceContext`, it needs the guard.**
6. **(e) THE MOST IMPORTANT: the char-boundary snap's panic-prevention role is
   unwitnessed, and its stated justification may already be stale.** With
   `snap_span_to_char_boundaries` reduced to a pass-through, two genuinely mid-character spans
   **rendered without panicking** against `ariadne 0.6.0` — while the snap's own doc comment
   asserts that **both** renderers panic (ariadne `write.rs`, annotate-snippets
   `renderer/source_map.rs`). So at least the ariadne half of that claim is **unconfirmed at
   the version we ship**. Do not carry this forward as "add a witness": Plan 3 must first
   determine **which renderer still aborts** on a mid-character span, then either (a) pin that
   renderer with a test — making the snap justified *and* bound — or (b) downgrade the snap's
   rationale to the widening behavior it demonstrably does provide. Carrying an unwitnessed
   guard whose stated reason may be obsolete is how the next "why is this code here" question
   gets answered wrongly.
7. **(f) `SourceInfo: !Hash` is an undocumented type-level guard** on the incremental-rebuild
   hash's provenance exclusion — the most obvious way to regress that arm cannot be written at
   all. Closed in the final fix wave: `hash.rs`'s `Scalar` arm comment now notes this.
8. **(g) The audit's row 3 covered two guards on different code paths** — `meta.rs`'s
   `markdown_base` vs `config_markdown.rs`'s `base` — so a mutation at one cannot redden the
   other guard. The row treated them as one site.
9. **(h) A test for a caught panic on an error-severity diagnostic.** This is the panic
   guard's most dangerous failure mode: it would rescue an exit code that must stay non-zero.
   The structural argument for why this can't happen is sound — `diagnostic_counts()` runs
   before printing, so an error-severity diagnostic has already been counted by the time
   `render_diagnostic_guarded` could swallow its render panic — but it is the one property here
   worth ~20 lines of test rather than an argument. (Final whole-branch review, deferred item
   8.)

   > **Correction, 2026-08-23 (Plan 3 Phase 6d, T9).** The record above is left as written; this
   > note corrects it rather than replacing it. **The stated ordering is backwards.** Printing
   > comes *first*: `print_render_diagnostics(&summary, …)` is `render.rs:836` and
   > `should_exit_nonzero(&summary, …)` — which calls `diagnostic_counts()` — is `:848`. The
   > project path has the same shape: `execute_project` (`:854`) prints at `:1024` and gates at
   > `:1041`. So "`diagnostic_counts()` runs
   > before printing" is false, and a reader who believed it would conclude that reordering the
   > two calls is safe.
   >
   > **The property that actually holds is immutability.** Both calls take `&summary`, and every
   > closure `render_diagnostic_guarded` runs borrows `&CoalescedDiagnostic` /
   > `&DiagnosticMessage` / `&SourceContext` only — the function requires `UnwindSafe` *without*
   > `AssertUnwindSafe` (the bound is in the signature at `:1290-1293`; the rationale is the doc
   > paragraph at `:1278-1283`), which is what makes that true at the type level. A
   > swallowed render panic therefore cannot remove a diagnostic from the summary the exit gate
   > counts. The item's conclusion is right; only its reason was wrong.
   >
   > The guard's own doc comment (`:1268-1270`, "after the pipeline run and before
   > `should_exit_nonzero`") was already correct and needed no change.
   >
   > **Discharged.** T9 —
   > `diagnostic_render_panic_boundary.rs::caught_panic_on_an_error_keeps_exit_code_nonzero` —
   > now pins it. It is labelled in-file as an **invariant pin, not a regression test**: no edit
   > to `render_diagnostic_guarded` can redden it, because the gate counts the summary rather
   > than what was printed. Its only hunk is the hypothetical refactor "compute the exit status
   > from the diagnostics that were actually printed".

## Evidence

_A phase is not done until its evidence is here._

### The block-scalar fixture (transcribed here and in Plan 1)

Phase 3's block-scalar test and the binding regression test both need this.
It is **not** the ariadne-emoji-panic repro — that one is a single-line
single-quoted navbar `text:` and cannot produce accumulating drift at all. It
lives untracked at `workspace-1/.scratch/blockscalar/_quarto.yml`, under a
`.git/info/exclude`'d directory in another worktree.

Plan 1 has since transcribed it too, along with a second `accum/` variant, so
it no longer depends on `.scratch/` surviving in any one worktree. The copy
below is deliberate redundancy, not a claim of exclusivity — an earlier
revision of this section said it existed nowhere else, which was true of an
earlier Plan 1 and is no longer.

```yaml
# _quarto.yml
project:
  type: website
website:
  title: "T"
  page-footer:
    center: |
      line one
      line two
      <span id="y">Footer</span>
```

```markdown
<!-- index.qmd -->
---
title: "Index"
---

body
```

`center: |` is line 6; the `<span id="y">Footer</span>` is on **line 9**, its
two tags at columns **7** and **26**. Both `Q-2-9` warnings belong there;
today they are reported at `8:10` and `9:14` — a constant drift of 12, being
2 preceding content lines × 6 bytes of stripped indent.

**Settled.** An earlier revision flagged that the three documents named three
different keys for this measurement (Plan 1's seam said navbar `text:`; this
plan and the epic said `page-footer.center`). Plan 1 built and measured both
candidates on 2026-08-21 and corrected its seam to `page-footer.center`; the
numbers above come from that run, which is the first time the epic's cited
pair was tied to a file rather than carried forward.

### Pre-execution baselines (`quarto-source-map` 0.1.1)

Measured by the Plan 3 review session, 2026-08-21, using a throwaway Rust test
that constructed the `SourceInfo` shapes below by hand and printed each
accessor's answer. Reproduced here because **that probe was reverted after
capture** — the code is no longer in the tree,
so this record plus the fixture spec below is the only reconstruction path.

**Read with the caveat:** the `Concat` was **hand-built**, not
`ProvenanceBuilder`-produced (the builder does not exist yet). These numbers
measure 0.1.1's behavior *given that shape*, not the eventual pipeline. The
shape is faithful to Plan 1's derivation — a 2→1 replacement flanked by
verbatim runs, gap-free — so the real builder is expected to produce it, but
that is inference, not measurement.

Fixtures, `fid = quarto_yaml::file_id_for_filename(<temp path>)`, candidate a
real file on disk, each `bind_config_source` call given a fresh
`SourceContext` with the file **not** pre-registered (modelled on
`quarto-core/tests/integration/project_profile_overlays.rs:447-465`):

- **A** — gap-free, models `'it''s'`:
  `concat(vec![(Original{fid,1,3}, 2), (Original{fid,3,5}, 1), (Original{fid,5,6}, 1)])`;
  content 4 bytes, source extent 1..6, piece 2 the doubled quote
- **B** — gappy, models a naive block scalar with the indent dropped as a
  standalone deletion: `concat(vec![(Original{fid,6,10}, 4), (Original{fid,12,15}, 3)])`,
  source gap at 10..12
- **C** — `substring(A, 0, 4)`, the whole content. **This is the shape a
  diagnostic carries after Phase 3.**

```
1.  A.resolve_byte_range()          = None
2.  C.resolve_byte_range()          = None
3.  C.root_file_id()                = Some(FileId(9415328668825900988))
4.  A.preimage_in(fid)              = Some(1..6)
5.  B.preimage_in(fid)              = None
6.  C.preimage_in(fid)              = Some(1..5)      # WRONG; truth is 1..6
7a. C.map_offset(0, &ctx)           = Some(offset: 1, row: 0, column: 1)
7b. C.map_offset(4, &ctx)           = Some(offset: 6, row: 0, column: 6)
8.  bind_config_source(&C, [path])  = None
9.  bind_config_source(&Original(1,6), [path]) [control] = Some("…/probe.yml")
10. C.start_offset() = 0   C.end_offset() = 4   C.length() = 4
    A.start_offset() = 0   A.end_offset() = 4   A.length() = 4
```

What each line is load-bearing for: (8) vs (9) is the Phase 3 binding
regression. (6) vs (7a/7b) is why a hull must come from the `map_offset` pair.
(4) vs (5) is why gap-free tiling is a decoder requirement. (10) is why
content coordinates must never be read as file offsets.

**Make these permanent** — tracked as items in the phases that own them, not
here: 7a/7b wherever a hull is computed (Phase 4), 8/9 as the binding
regression test (Phase 3), 10 as the TS-facing assertion (Phase 4). Line 6 is
a permanent test too, of the *opposite* value: Plan 1 fixes `preimage_in`'s
`Substring`-over-`Concat` arm in 0.1.2, so the wrong `Some(1..5)` becomes
`None` — assert the `None`. Plan 1 owns that assertion in its own Phase 1;
do not duplicate it here.

### Phase 1
**Phase 1 is COMPLETE.** `quarto-error-reporting` **0.2.2 published** to crates.io
2026-08-22 (PR posit-dev/quarto-error-reporting#5, merge `87f1d38a1`, CI green on all four
checks, release workflow run 32577564699 succeeded). Commits in that repo:
`66d115c` (lockfile -> quarto-source-map 0.1.3), `5e48166` (re-anchor), `a3d5d5a`
(zero-width test), `922b09c` (cut 0.2.2).

**The unrepeatable binding experiment ran first, and its evidence is banked.** On the old
lock (quarto-source-map 0.1.0), reverting only the `:878` snap call made
`ariadne_span_starting_inside_multibyte_char_does_not_panic` go **red**:

```
end byte index 21 is not a char boundary; it is inside '\u{2728}' (bytes 19..22 of string)
   at ariadne-0.6.0/src/write.rs:84:59, in Report::get_source_groups
```

reached via `render_ariadne_source_context` (`diagnostic.rs:968`) <- `render_source_context`
(`:710`) <- `to_text_with_renderer` (`:469`). **Zero frames inside quarto-source-map** — the
abort is ariadne's own `str` slicing, which is why the snap belongs at the renderer. The
file was then restored and the test confirmed green again.

Worth recording: the panic names **"end** byte index 21", though 21 is the span's *start*.
So 21 arrived as the END of a zero-width range — it fired on ariadne's `Report::build`
**anchor**, independently confirming the anchor is `start..start` and that the single `:878`
call is load-bearing for the anchor, not just the main label.

**Obligation 3 (re-anchoring) discharged.** Both tests renamed to
`ariadne_renders_diagnostic_with_originally_mid_character_span` /
`annotate_snippets_renders_diagnostic_with_originally_mid_character_span`, doc comments
rewritten to state what they now cover, why the snap coverage is gone (the 0.1.2+ floor
snaps 21->19 in `map_offset` before any renderer is reached), and where it went — with an
explicit accepted-unbound note. No test deleted. `snap_span_widens_to_whole_characters`
extended with the same `21..28` pair over the same content; asserted `snap2(21, 28) == 19..28`
(21 floors to 19; 28 is already a boundary inside the trailing ASCII `</span>`).

**Obligation 2 (zero-width label) discharged BY MEASUREMENT, replacing the plan's "very
likely renders".** On `"x = 'A\u{2728}B'"` with `original(fid, 7, 8)` (both ends inside the
character), after the floor both ends arrive as 6, so the highlight is zero-width. **Both
renderers render it:**

| | zero-width `(7,8)` | whole-character `(6,9)` |
|---|---|---|
| ariadne | a bare `│` marker | `┬─` |
| annotate-snippets | a single `^` | `^^` |

The tests key on that marker **shape**, not on the message text. Discrimination was verified
**twice independently** (implementer and reviewer each swapped the fixture to `(6,9)`,
observed the failure, and restored). One trap found doing it: ariadne's zero-width marker
glyph `│` is the same character as its own gutter, so a naive trim of the marker row
picks up the gutter and false-passes; the assertion slices past the gutter explicitly.

Gates: `cargo xtask verify` 6/6, `cargo test --locked`, `cargo test --all-features --locked`,
and clippy on `--no-default-features`, ariadne-only and annotate-snippets-only (all
`--all-targets --locked -D warnings`), plus `cargo package --locked`.

Strand `bd-ariadne-config-span-char-boundary-panic-rkqmhzrg` **closed** (comment
`c-pc5pb5mj`). `bd-chmbr0zl` (the panic-boundary *class*) deliberately left open — that is
Phase 5.

**Out-of-plan, filed as bd-78e1ahjc:** `test_location_in_to_text_with_context` fails under
`cargo test --no-default-features` (no renderer feature). Pre-existing — reproduced at
`4da3385` before any of this work, verified twice. Neither that repo's CI nor this plan runs
`cargo test` in that configuration (the three extra legs here are clippy-only).

### Phase 2
**Phase 2 is COMPLETE**, and it was **split in two** (ruling: the plan's single item took
all four versions, which would have blocked Phases 3-7 on a crates.io publish needing
approval; nothing in Phase 3 or 4 needs 0.2.2).

- `f250c9bf0` — **tripwire first**, on the OLD lock: a test pinning the pre-0.1.2 `Concat`
  exclusive-end `map_offset` value, with a deliberately non-degenerate fixture (last piece's
  written length 4 vs its `source_info` span length 100, asserted with `assert_ne!` so it
  cannot silently degenerate into the verbatim case where both branches agree). Observed
  `Some(offset: 10, row: 0, column: 10)`; **predicted** `Some(offset: 106, ...)` post-refresh
  (`6 + 100 = 106`, delta 96 = 100 - 4).
- `77bd9d6c0`, `80aca04a7` — refresh to `quarto-source-map` **0.1.3** (declaration bumped
  from `"0.1.0"`, since Phases 3-4 need `ProvenanceBuilder` which 0.1.2 deliberately
  excludes) and `quarto-yaml` **0.1.3** (lock only; `^0.1.2` already accepted it). **The
  tripwire moved to exactly 106**, offset and column — the cross-task prediction/observation
  handshake worked. Test renamed `concat_exclusive_end_maps_via_source_length`, since the old
  name described behaviour that is now false.
- `9c19df14f`, `e86b9c10b` — `quarto-error-reporting` **0.2.2** once published, in the root
  lock **and in the nested one** (see below).

`cargo tree -i quarto-source-map`: exactly one node (0.1.3) — 0.2.2's `^0.1.0` unifies onto
it. `[patch.crates-io]` confirmed by name in **both** manifests (root: `lua-src`,
`tree-sitter-language`, `runtimelib`, `jupyter-protocol`; nested: `lua-src`,
`wasm-bindgen-futures`, `tree-sitter-language`) — no `quarto-*` override in either. That
discharges the plan's explicit confirmation item for two workspaces where it only knew of one.

**Obligation 6, both halves, measured against the PUBLISHED crates (not a path patch):**
- `pampa/tests/integration/test_location_health.rs:448` — **GREEN**, 12/12
  `test_location_health` tests pass. Recorded with the right framing: green means this
  suite's `Location` values already sit on char boundaries, **not** that the two
  `offset_to_location` implementations agree in general.
- JSON-writer `"o"` snapshots — **explicit ZERO**, proven by
  `git diff <base>..<head> --stat -- '**/snapshots/**' '*.snap'` being empty, not merely by
  nothing looking different.

**Finding the plan did not have: q2 has a SECOND tracked lockfile.**
`crates/wasm-quarto-hub-client/Cargo.lock` belongs to an independent nested workspace
(`crates/wasm-quarto-hub-client/Cargo.toml:54` declares a bare `[workspace]` deliberately)
and was still on `quarto-source-map` 0.1.0 / `quarto-yaml` **0.1.2** / `quarto-error-reporting`
0.2.1 — so the **WASM / hub-client path**, which is what `q2 preview` and Quarto Hub run,
was getting none of this epic. It was also silently regenerated by every WASM build, since
that leg is not `--locked`. Refreshed in `e86b9c10b` (lock only — no manifest edit needed;
verified, not assumed: `^0.2.1` accepts 0.2.2, `^0.1.0` accepts 0.1.3, and `quarto-yaml`
arrives transitively via the `quarto-core` path dep whose `workspace = true` resolves against
the **root** workspace's `^0.1.2`).

Confirmed by a full `cargo xtask verify` (all 14 steps, including step 7 hub-client+WASM which
compiles `wasm-quarto-hub-client` itself, step 12 hub MCP tests, step 13 preview SPA):
**green, and `git status` completely clean afterwards with that lockfile UNCHANGED.** That
last clause is the actual success criterion — "verify is green" alone would prove nothing,
because verify is what regenerates the file.

### Phase 3
**Phase 3 is COMPLETE.** _(This paragraph read "PARTIALLY COMPLETE" until Phase 7's
reconciliation; the three items it listed as not done were finished in session 2 and are
recorded at the end of this subsection.)_ Session 1 landed the `Scalar` struct variant,
`resolve_span` piecewise resolution, the threading fix itself, and the binder change;
session 2 landed the dead converter, the desync report, and `caption_inlines`.

**The `ConfigValueKind::Scalar` migration** (`6a5de44b6` shape + helpers + serde + hash arm
+ wire-shape test; `c9a77d18c` the sweep). Touched **~207 non-test / 241 total sites across
58 files**, vs the plan's predicted 206/240 across 49+8. The plan's figure came from grepping
the literal `ConfigValueKind::Scalar(`, which cannot see sites reached through an alias:
`quarto-core/src/stage/stages/include_resolve.rs` (`use ConfigValueKind as K`) and
`pampa/src/lua/config_value.rs` (`use ConfigValueKind::*`) surfaced only as compile errors.
**For any future "N sites" estimate in this epic, the compiler is the census, not grep.**
Done-condition met: full `cargo xtask verify` (14/14, including the WASM leg — the gate
`cargo build --workspace` cannot give). Snapshots: zero.

**`resolve_span` piecewise** (`b7067903d`, plus fix round `6b132bccb`). Resolves `Concat` and
`Substring{parent: Concat}` via the `map_offset(0)`/`map_offset(length)` pair; never uses
`preimage_in` (0.1.3 makes its `Substring`-over-`Concat` arm return `None`, so it is
structurally incapable of this job). `SpanProblem::Concat` **kept, not deleted**, with its
meaning narrowed to "**gappy** `Concat`, or a chain bottoming out in one, or two ends in
different files". Added `peel_generated`, so a `Generated` span with a valid invocation anchor
resolves through the anchor instead of refusing. Two pre-existing refusals
(`Generated`-with-no-anchor, `NoContent`) had **no direct test at all** before this task and
now do.

**A third instance of this epic's central confusion, found in review.** That task's first
draft walked concat piece boundaries using each piece's **declared per-piece `length`** (a
*content* length) and misclassified the plan's own gap-free `A` fixture as **gappy** — because
piece 1 is `Original(3,5)` declared with concat-length 1, a 2-byte source span folded to 1
content byte. The fix measures each piece's own **source** extent
(`piece.source_info.map_offset(0)` / `map_offset(piece.source_info.length())`). This is
exactly upstream `0c65d52`'s defect — solved there for the *last* piece only — generalized to
every boundary. So "content length is not source length" has now produced **three** separate
bugs in this epic (upstream's exclusive-end branch, this gap detection, and the drift the
threading fixes). That recurrence is the strongest argument for the § Risks decision to leave
`start_offset()`/`end_offset()` unhardened and fix call sites instead.

Gap detection deliberately **over-approximates**: a `Substring` over *part* of a gappy
`Concat` is reported gappy even if its own sub-range avoids the gap. Conservative in the safe
direction, and harmless here because YAML content provenance tiles gap-free by construction —
so if a later phase sees `SpanProblem::Concat` from its own spans, that is a signal **its
provenance has a gap**, not a helper limitation.

**One Critical caught in review and fixed** (`6b132bccb`): the rewrite made `OutOfBounds` stop
firing when a span's **start** offset alone was past EOF, reporting `Generated` instead —
because `map_offset(0)` returns a bare `None` for an out-of-range offset
(`file_info.rs:86-88`) and the new `(None, _)` arm never considered "past EOF" as a candidate.
That is precisely the anti-pattern the task was fixing (`Generated` sends a reader hunting for
a filter-created node that does not exist), reintroduced one shape along. It slipped past the
task's own test because `out_of_bounds_is_reported` uses an **in-bounds** start with only the
end past EOF, landing in a different arm that handles it correctly — two independent
`OutOfBounds` paths, one tested.

**The threading fix** (`1b6d30c08`) — the root cause. `content_source_info` is read **once**
in `meta.rs`, between the `source_info` clone and the line that partially moves the yaml node,
and used for both consumers: stored in the `Scalar` variant (scoped to `Yaml::String`) and
passed as the re-parse base at all three immediate re-parses (the `!md` tag, the annotated
`Markdown` interpretation, the `DocumentMetadata` default), falling back to the node span when
`None`. Carrier read in `config_markdown.rs`'s `parse_scalar_string_in_place`.

**Confirmed end-to-end through the real binary**, on the block-scalar fixture:

```
Warning: [Q-2-9] HTML element converted to raw HTML
   ╭─[ <dir>/_quarto.yml:9:7 ]
 9 │       <span id="y">Footer</span>
   │       ──────┬──────
```

Both warnings now at **`9:7`** and **`9:26`**; previously **`8:10`** and **`9:14`** — the
first of those being the **wrong line**, the only such misattribution in the epic. The
block-scalar test asserts **both line and column** for both warnings, which is the required
vacuity guard: a column-only assertion would pass if the fix corrected the column and left the
line wrong.

The quoted-scalar expected values were **derived fresh** from a purpose-written fixture and
the derivation recorded, per this plan's instruction not to carry the unattached
"36/43 -> 37/44" baseline forward. A reviewer re-indexed both fixture lines by hand and
reproduced them independently.

**Snapshot gate: 3 files moved, and the explanation matters more than the count.** All three
are fixtures with **plain (unquoted)** front-matter scalars — which this plan says must stay
correct — so this looked like the over-correction the regression guards exist to catch. It is
not. `quarto-yaml` derives content provenance for *every* real-parse scalar, and even where
content and raw spans cover identical bytes the content node is a **structurally distinct**
`SourceInfo`; the JSON writer interns every distinct node, so the tree gains one
numerically-equivalent node per re-parsed scalar and every subsequent intern index renumbers.
**No resolved position changes** — a reviewer confirmed this by hand-tracing the `r:[...]`
byte ranges up each chain (`Str` -> `MetaInlines` -> block -> file) and finding them
byte-for-byte identical, and the plain-scalar guard passes with unchanged expected positions.
Separately, insta's `assertion_line` bookkeeping dropped from `002.snap` and `003.snap`; that
metadata was already inconsistently present across that directory beforehand.

The optional four-warning `accum/` variant was **declined**, with rationale (the existing
coverage already demonstrates per-preceding-line accumulation, and the brief flagged a real
trap: the third element's reported column numerically equals the second element's *truth*
column, so a naive per-element assertion reads as fixed while sitting on a stale value).

**Serialization-boundary confirmation** (the plan asked for this to be recorded): no boundary
sits between producer and consumer. `pampa/src/readers/json.rs` constructs `Scalar` only on
the Pandoc-JSON **input** path (`:2719-2867`), and the Lua bridge uses Lua tables, not JSON.

**The binder change** (`a23f25573`) — see the corrected "binding regression" item above for
the important part: the predicted symptom does **not** manifest, because `MetadataMergeStage`
masks it. The fix stands on its own merits (the binder only ever wanted the id, and
`root_file_id()` is what the renderer already uses), and the red/green proof is a focused unit
test against the binder rather than the CLI fixture, which passes identically in both states.
Scope check recorded: the other binders act on `ConfigValue` spans, which stay contiguous;
`rebase_source_candidates` genuinely needs the range to rebuild an `Original` and is
**unmodified** — a grep confirms exactly one remaining `resolve_byte_range` call in that file,
inside it. Corrected detail: the candidate list at both real call sites is
`config_path + profile_config_paths + extension_manifest_paths`; **`dir_layer` paths are never
passed to `attach_config_source`**.

**Phase 3's remaining three items, landed in session 2.**

- **The dead converter** (`2f2a4d2a9`, plus fix round `454f959d3` and the tautology drop
  `33919d7cf`). Taken as the corrected
  item directs: `config_value_from_yaml` demoted to `pub(crate)`, provenance threaded in
  lockstep with pampa's converter, the layering documented. Initially additionally
  `#[cfg(test)]`-gated, because a `pub(crate)` fn with no non-test caller trips `dead_code`
  under `-D warnings` — **at the cost, recorded because it was invisible in the diff, that the
  function was no longer type-checked by `cargo build --workspace`**; only the test and
  `clippy --all-targets` held the lockstep. **Paid off in the final fix wave:** regated to
  `#[allow(dead_code)]` (same for its two helpers), so the function and its lockstep partner
  are type-checked in every build. The threading was **flagged as unbound by the task itself** and closed in the
  fix round with two `convert.rs` tests parsing real YAML: the positive
  (`k: "hello"` → `content_source_info` resolves to `hello` *without* the quotes) reddens
  under a revert of the threading hunk — run, not asserted; the negative (`k: 42` → `None`) is
  a **scoping guard**, not binding evidence, and Phase 6 treats it as a mutation row (row 10).
- **The desync report** (`876bc5081`). A warning-level, code-less
  (`code: None`) `DiagnosticMessage` in **both** converters — `pampa::pandoc::meta` and
  `quarto_config::convert` — fired when a node already established to be a `Yaml::String`
  returns `None` from `content_source_info()`. Four new tests (a positive/negative pair per
  crate); revertibility verified by hand. Fallout absorbed at **8 named locations** (the two
  `convert.rs` test helpers + 6 inline `meta.rs` call sites, 14 call instances across 13
  tests) — matching the plan's predicted 8 exactly. The two documented `None`s were written as
  comments at `parse_scalar_string_in_place`, both accepted-untested, with every cited line
  number re-verified against the tree.
- **`caption_inlines` / `fig-cap`** (`421c6532c`). `parse_cell_options` now takes
  `OptionValue.value_source` from `entry.value.content_source_info()` with a raw-span
  fallback — no new carrier field, since `content_source_info()` already exists on
  `YamlWithSourceInfo`. Two tests, both bound by revert (`"*stron"` vs `"strong"`); the second
  confirms the load-bearing `Substring{parent: Concat}` shape by pattern-matching the raw
  span rather than assuming it. **Ruled-out sites, per that item's instruction:** every other
  `value_source` reader was checked and every one wants the content base, so none needed a
  separate change. Snapshots: none. This task is where the `is_gapless` limitation was
  measured — see session 2's correction 4 and R-9.

### Phase 4
**Phase 4 is COMPLETE and review-clean.** Commits `07d2c1ff5` `de3697610` `1dbfa7b2b`
`4aa87230f` `3efcb2c48` `93b212200` `962525b3a`. _(This subsection read `_(pending)_` until
Phase 7's reconciliation; the phase's own numbers were recorded in § EXECUTION STATUS —
session 2 and are consolidated here, where the plan says evidence lives.)_

**The generality proof — the reason the phase exists.** q2's attribute decoder now drives
`ProvenanceBuilder`, `AttrSourceInfo` carries content provenance, and
`callout.rs`'s length-arithmetic workaround is **deleted**. The builder therefore has a second
consumer in a completely different decoder from Plan 1's YAML path, which is what Phase 4 was
for. The zero-content-piece trap named in the plan was not fallen into: verbatim pieces are
tagged **by bytes**, not by length.

**End-to-end through the real binary, inspected** (not inferred from tests). Fixture
`::: {.callout-note title="\# Say \"hi\" now"}`, where `\#` collapses to `#` and the value
then parses as block content, so `Q-2-44` lands *on* the span this phase changed:

```
   ╭─[ diag.qmd:5:27 ]
 5 │ ::: {.callout-note title="\# Say \"hi\" now"}
   │                           ────────┬────────
```

Column **27** — the first byte inside the opening quote, underline stopping before the
closing quote. Under the revert: column **26**, underlining both delimiters. That
one-column, two-character difference is the whole user-visible effect.

**Obligation 8 discharged by an injection experiment, not by the plan's reachability
argument.** Census confirmed at **23 `start_offset()` sites across 22 files** plus **2
`end_offset()` sites** in `qmd-syntax-helper` (the plan's count was right; one of its three
named example sites was not — see § Latent exposures). Seven new tests share one fixture
carrying every construct whose value slot changed. Binding was **measured**: wrapping every
diagnostic location on both arms of `pampa::readers::qmd::read` in a
`SourceInfo::concat(...)` — precisely the shape the decoder now produces — visibly corrupts
`qmd-syntax-helper`'s output (a splice at byte 0; a `replace_range` percent-encoding ~340
bytes), and **5 of the 7 tests catch it**. No production code changed in that crate.

**The TypeScript boundary.** `@quarto/annotated-qmd` moved to content semantics and bumped to
**0.2.0** (breaking: an attribute value's range no longer includes its quotes). Pre-fix
ranges, measured verbatim before any edit: `both="\[x\]"` resolved to `[36,40]` — ending
*inside* the escape — against the correct `[36,41]`, and every block-scalar inline from the
first collapsed piece onward drifted by the two indent bytes (`Str "line"` → `[24,28]`,
source text `"  li"`, vs the correct `[26,30]`). **Both** TS arms were wrong, contrary to the
plan and both briefs: the `Substring` arm (composed affinely) *and* the `Concat` arm (whose
`map(len-1).index + 1` assumed the last content byte came from one source byte). Node suite
**161/161** after the two fix rounds, up from 156 pre-existing (2 of which were red on this
branch — see § A gating gap worth knowing) plus 5 new.

**Snapshot movement: 1 file.** `crates/pampa/snapshots/json/table-caption-attr.snap`, exactly
one number — source-ref index 2, the `tbl-colwidths` value — `[104,113]` → `[105,112]`,
i.e. `"[30,70]"` → `[30,70]`: quote exclusion, one byte each end, derived from the fixture's
real bytes twice independently. The other 31 source refs and the AST body are byte-identical.
**No** snapshot moved for the escape-collapse case, because no snapshot fixture in the tree
has an escaped attribute value — which is why that case needed unit tests instead.

**Rust test-count delta at D1**, against the live baseline 12889/198: **12904**, i.e. `+15` =
`2×7 + 1` (seven new pampa in-crate tests count twice via pampa's `[[bin]]` target; one
`quarto-core` in-crate test counts once). No skips moved.

### Phase 5
**Phase 5 is COMPLETE.** Commit `73673ba48`. Full detail in `task-E1-report.md`.

`render_diagnostic_guarded` wraps `catch_unwind` around the **per-diagnostic** render — not
the loop — at **eight** sites: three in `print_render_diagnostics_text`, five in
`print_render_diagnostics_json`. (The checklist item said five; see its correction. Per-loop
guarding was rejected because it would still discard everything queued behind a bad
diagnostic, which is the defect.) On a caught panic the diagnostic is dropped and a loud
`internal error rendering diagnostic <CODE>` line replaces it on stderr — asserted by the
tests, so a *swallowing* boundary cannot pass.

**`UnwindSafe`: NOT NEEDED — and this is compiler-proven, not argued.** The helper's signature
is `render: impl FnOnce() -> T + std::panic::UnwindSafe`, a plain **non-asserted** bound with
**no `AssertUnwindSafe` anywhere**, and all eight call sites compile against it as written
(`cargo build -p quarto --bin q2` and `cargo clippy -p quarto --all-targets -- -D warnings`
both clean). So the plan's "probably unnecessary" prerequisite is settled affirmatively by the
type system rather than by inspection: every wrapped closure only borrows
`&CoalescedDiagnostic` / `&DiagnosticMessage` / `&SourceContext` or moves plain
`String`/`PathBuf`, none of which carries interior mutability. The one `&mut` in the
neighbourhood — `attach_config_source(&mut group, ..)` at the third text site — is called
**before** entering the guard, so the mutable borrow never crosses the boundary. A future edit
that drags one across now fails to build, which is a better guarantee than
`AssertUnwindSafe` would have given.

**The fault-injection seam is structurally unarmable in release**, verified two ways rather
than by pointing at the `cfg`: `strings target/release/q2 | grep -c
QUARTO_FAULT_INJECT_DIAGNOSTIC_RENDER` → **0** (debug: 2) and `nm target/release/q2 | grep -c
fault_inject_diagnostic_render` → **0** (debug: 1). Both the env-var literal and the symbol
are absent, not merely inert. The env var selects the **Nth** guarded render by a process-wide
counter, so a test needs "one of several queued diagnostics panics" rather than control over
real diagnostic codes.

**TDD red observed on a genuine pre-fix state**, not on the implementation: the fix was
stashed back to `fe0fb44b1` and a *bare, unguarded* `panic!` inserted at one text site, giving
`thread 'main' panicked … ` in the child `q2` process and a failed
`output.status.success()` — i.e. a warning-only render turned into a non-zero exit, exactly
the defect. 3 of 4 tests red; after restoring the real implementation, 4/4 green. Test-count
delta `+4` against 12914/198 → **12918/198**, all four in
`crates/quarto/tests/integration/diagnostic_render_panic_boundary.rs`. `cargo xtask lint`
clean (1037 files). Snapshots: none.

**Manually exercised end-to-end**, armed: `QUARTO_FAULT_INJECT_DIAGNOSTIC_RENDER=0 q2 render .`
→ exit **0**, the default panic hook's own message on stderr (tolerated by design), then
`internal error rendering diagnostic Q-16-3`, then the *surviving* second warning, then the
normal "Rendered 2 of 2 files" line, with both `_site/a.html` and `_site/b.html` written.

`bd-chmbr0zl` **closed**. Two items its close reason defers to review, recorded here so they
are not lost: the internal-error line names the diagnostic **code** but not the **document**,
and **6 of the 8 sites have no fixture reaching them**. Also a deliberate deviation from the
strand's text: the internal-error line is **not** routed through the normal diagnostic channel
for `--json-errors` consumers (the strand said "arguably"), because a new `Q-` code would
require a docs page plus an in-code-order sidebar entry in the same commit per
`cargo xtask lint`, and it is not a user-actionable catalog error.

### Phase 6
**Phase 6 is COMPLETE.** The audit's own record is `task-F-report.md` (1421 lines, one section
per row, with verbatim commands and output), reviewed in `task-F-review.md` and re-reviewed
after one fix round in `task-F-rereview.md` (verdict: all findings addressed). Only the
headline results are transcribed here; the per-row observations are deliberately not, because
the plan is a durable record and not a transcript.

**Result: 20 rows. 15 matched expectation, 5 deviated (rows 1, 2, 14, 15, 19). No test in
scope was found unbound-and-unexplained.** In every deviation the *prediction* was wrong, not
the coverage — none is an unbound test. Two rows (8, the `callout.rs` deletion; 11, the
verbatim-by-bytes tag rule) are **accepted-unbindable by construction**, which is a different
audit outcome from "unbound": in each case the code cannot produce the discriminating input.
The two `..._originally_mid_character_span` smoke tests remain accepted-unbound per Phase 1's
carve-out, verified vacuous by direct measurement rather than accepted on faith.

**Post-audit addition (final fix wave, after Phase 7 closed this phase):** the final
whole-branch review found a fourth site of the epic's own defect pattern that the audit's 20
rows did not cover, because it was not discovered until after Phase 6 ran —
`project/website_post_render.rs:217` (`copy_footer_images`), see § Workarounds that collapse.
Fixed in the same wave. It joins rows 8 and 11 as **accepted-unbindable by construction**: no
consumer reads those inlines' spans (they feed only image-URL extraction and the parse
diagnostics are discarded), so no fixture can distinguish the corrected `content_source_info`
base from the raw `cv.source_info` one. Recorded honestly as unbindable rather than passed off
as covered — the same audit outcome category as row 8, not a fifth row retroactively added to
the 15+5=20 count above.

**How the plan's 15 rows became the audit's 20**, stated precisely, because the first version
of this paragraph got it wrong in a way this plan's own checklist contradicts. _(It claimed
rows 10, 12, 19 **and 20** had no item, and that rows 14/15 "split the TS `Concat` case in
two"; corrected in the same session, verified against the checklist at `992813188`.)_ The
plan's list carried **14 items covering 15 rows** — rows 7 and 8 were bundled into one — plus
three process items. So:

- **Rows 10, 12, 14, 15 and 19 had no item in the plan's list at all.** Rows 14 and 15 split
  nothing: the pre-edit list ran straight from the TS `Substring` item (row 13) to the
  char-boundary snap (row 18), with no TS `Concat` item between them. They are new content.
- **Row 8 was split out of row 7's bundled item**, in the same commit as this reconciliation.
- **Row 20 already had its own item in the plan** — "Revert the panic boundary only → expect
  the injection test red", which the reconciled checklist reuses rather than adds. What row 20
  was missing from is the **audit's brief**, a different document: the resolution artifact's
  un-rowed walk ended one commit before the audit's base, so nothing swept Phase 5. The review
  caught it, and because the *plan* had always listed it, it was executed as a real revert
  rather than a citation.

15 + 5 = 20.

**The audit's methodological finding, which is the most transferable thing in it.** The
rows-14+15 combined leg was first run with a **hand-adapted transplant** of the pre-fix body
into a function signature that did not exist pre-fix. It produced the same red/green set as
the later clean revert — but **invented wrong values**: `{fileId: 0, start: 0, end: 5}` for
*both* cross-file tests, where a true whole-file state revert
(`git checkout 3efcb2c48^ -- src/source-map.ts`, byte-identity confirmed by hash) yields
`{fileId: 0, start: 6, end: 8}` and `{fileId: 0, start: 10, end: 10}`. Same set of reds,
different numbers — so a red emitted by invented code proves nothing about the code we
shipped. **This is exactly why the instrument is a revert and not a reconstruction.** The
authoritative values are the clean revert's; row 14's discriminating value (`both → [36,40]`
vs expected `[36,41]`) was byte-identical under both, which is what makes the divergence
legible rather than merely alarming.

**Restoration, which is the audit's real deliverable alongside the observations.** All three
repos clean afterwards, at baseline HEADs (`~/src/quarto-error-reporting` `922b09c6c`,
`~/src/quarto-source-map` `09ec6d117`); q2 at `992813188` = baseline + row 19's test commit,
the only intended change; **both** q2 lockfiles byte-identical to the pre-audit checksums;
no `[patch.crates-io]` override for `quarto-source-map` or `quarto-error-reporting` anywhere
(only the four pre-existing root entries and the nested workspace's three); no `TASK-F`
marker or scratch identifier surviving in any repo. Workspace suite after row 19's commit:
**12919 passed / 198 skipped**, the `+1` being row 19's single added test.

**Three findings routed onward** (see § Hand-off to Plan 3): the char-boundary snap's
panic-prevention role is **unwitnessed**; `SourceInfo: !Hash` is an undocumented type-level
guard; row 3's two guards sit on different code paths, so a mutation at one cannot redden the
other.

### Phase 7
**Phase 7 is COMPLETE**, at plan HEAD `992813188` + this commit. Full detail in
`task-G1-report.md`.

**`cargo xtask verify` — FULL, not `--skip-hub-build` — GREEN, all 14 steps, exit 0.** This
is the gate no earlier phase in this plan ran; the previous full green was `1b6d30c08`,
**twelve commits** earlier. It matters because the `ConfigValueKind::Scalar` struct-variant
migration reaches `wasm-quarto-hub-client` through `quarto-pandoc-types`, and that target is
not built by `cargo build --workspace`.

| step | leg | result |
|---|---|---|
| 1 | custom lints + clippy (`-D warnings`) | ✓ — **1037 files checked**, no violations |
| 2 | Rust formatting | ✓ |
| 3 | Rust workspace build, warnings denied | ✓ |
| 4 | tree-sitter grammar tests | ✓ |
| 5 | Rust tests | ✓ — **12919 passed / 198 skipped**, 0 failed (1 slow, 1 leaky) |
| 6 | ts-packages workspaces build + MCP module-graph smoke | ✓ |
| 7 | **hub-client build, including WASM** | ✓ — 1935 modules, built in 23.29s |
| 8 | hub-client tests (`test:ci`) | ✓ |
| 9 | trace-viewer SPA build | ✓ |
| 10 | trace-viewer tests | ✓ |
| 11 | shared `preview-*` package tests | ✓ |
| 12 | hub MCP package tests | ✓ |
| 13 | q2-preview-spa build | ✓ — 460 modules |
| 14 | Playwright E2E | skipped (`--e2e` not set) |

The **12919 / 198** figure is exactly Task F's post-row-19 workspace baseline, so nothing
strayed or duplicated between Phase 6's close and this run.

**The annotated-qmd node suite, run directly** (`node --import tsx --test test/*.test.ts` in
`ts-packages/annotated-qmd`): **tests 161, pass 161, fail 0**, 20 suites. Run explicitly
because `cargo xtask verify` *builds* ts-packages but does not run their tests, and **no Rust
gate runs this suite at all** — which is why it sat silently red on this branch for five
commits mid-Phase-4 (§ A gating gap worth knowing). Its state must never be inferred from
anything else being green.

**`cargo xtask lint`** re-run standalone: `All checks passed! (1037 files checked)`.

**Neither lockfile moved.** Both byte-identical to the pre-Phase-6 baseline *after* a full
verify — which is the load-bearing form of the claim, because verify is exactly what
regenerates them, and the WASM leg is not `--locked`:

```
ccc01dd2c8cc77a0d1199fe7efcace923f31e31c  Cargo.lock
d632527b1bf8c98fda4faa75330e2fb57bb0399e  crates/wasm-quarto-hub-client/Cargo.lock
```

**No `[patch.crates-io]` override for `quarto-source-map` or `quarto-error-reporting`
survives anywhere**, checked by name in all four places the Phase 6 matrix touched: q2's root
manifest (only the four committed, load-bearing entries — `lua-src`, `tree-sitter-language`,
`runtimelib`, `jupyter-protocol`), the nested `crates/wasm-quarto-hub-client` manifest (only
`lua-src`, `wasm-bindgen-futures`, `tree-sitter-language`), and both upstream checkouts, which
have **no patch section at all** and are clean at their baseline HEADs (`922b09c` "Cut
quarto-error-reporting 0.2.2", `09ec6d1` "Release 0.1.3"). The nested workspace resolves the
right versions through plain semver deps, not a patch: both locks carry
`quarto-error-reporting 0.2.2`, `quarto-source-map 0.1.3`, `quarto-yaml 0.1.3`.

**Snapshot movement: NONE.** `git diff --stat 992813188 -- '**/snapshots/**' '*.snap'` is
empty, and `git status` shows only this plan file — no code changed in this phase, by design.
