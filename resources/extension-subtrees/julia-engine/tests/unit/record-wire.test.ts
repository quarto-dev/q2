// Unit tests for the q2 execution-record wire handling (record-wire.ts, EXR-8 B0/B1):
//
//   - when core sends `units`, the run asks QNR to annotate its cells (`emitUnits`);
//   - `handledLanguages` keeps flowing to QNR unchanged, `ownedLanguages` plays no part;
//   - the returned notebook is a deep copy taken BEFORE toMarkdown mutates the original;
//   - without `units` the engine behaves exactly as before (no new keys, no notebook);
//   - `recordInfo` carries the Julia hash declaration decided in plan task B1a;
//   - core-located unit lines are forwarded untouched, never re-derived.
//
// record-wire.ts is a pure module: no socket, no julia, no quarto internals.

import assert from "node:assert/strict";
import { test } from "node:test";

import {
  JULIA_HASH_DEFINITION,
  buildRecordResult,
  coreWillPresent,
  qnrRunOptions,
} from "../../src/record-wire.ts";

// The display-only options Julia declares: the same set as the Jupyter engine's
// `DISPLAY_ONLY_KEYS` (crates/quarto-core/src/engine/jupyter/mod.rs), since both
// run notebooks through the same presenter. Pinned literally so a change is deliberate.
const DISPLAY_ONLY = [
  "echo",
  "output",
  "warning",
  "include",
  "code-fold",
  "code-summary",
  "code-overflow",
  "code-line-numbers",
  "code-copy",
  "fig-cap",
  "fig-subcap",
  "fig-scap",
  "fig-alt",
  "fig-align",
  "fig-link",
  "fig-cap-location",
  "tbl-cap",
  "tbl-subcap",
  "tbl-scap",
  "tbl-colwidths",
  "tbl-cap-location",
  "lst-cap",
  "lst-label",
  "cap-location",
  "layout",
  "layout-ncol",
  "layout-nrow",
  "layout-align",
  "layout-valign",
  "panel",
  "column",
  "keep-hidden",
  "fig-env",
  "fig-pos",
  "out-width",
  "out-height",
  "out-extra",
  "classes",
  "fig-column",
  "tbl-column",
];

const UNITS = [
  { id: "u-1", index: 0, kind: "cell", language: "julia", line: 7, owned: true },
  { id: "u-2", kind: "inline", language: "julia", line: 12, owned: true },
  { kind: "inline", language: "klingon", line: 13, owned: false },
];

function baseOptions(
  extra: Record<string, unknown> = {},
): Record<string, unknown> & { handledLanguages: string[] } {
  return {
    oneShot: false,
    handledLanguages: ["ojs", "mermaid", "dot", "klingon"],
    target: { input: "/abs/doc.qmd" },
    ...extra,
  };
}

test("engine_requests_emit_units_when_core_sends_units", () => {
  const out = qnrRunOptions(baseOptions({ units: UNITS }));
  assert.equal(out.emitUnits, true);
  // an empty list is still "core sends units"
  assert.equal(qnrRunOptions(baseOptions({ units: [] })).emitUnits, true);
});

test("engine_keeps_passing_handled_languages_to_qnr", () => {
  const handled = ["ojs", "mermaid", "dot", "klingon"];
  const withUnits = qnrRunOptions(baseOptions({ units: UNITS }));
  const without = qnrRunOptions(baseOptions());
  assert.deepEqual(withUnits.handledLanguages, handled);
  assert.deepEqual(without.handledLanguages, handled);
});

test("engine_ignores_owned_languages_on_the_wire", () => {
  const out = qnrRunOptions(
    baseOptions({ units: UNITS, ownedLanguages: ["python"] }),
  );
  // ownedLanguages neither changes handledLanguages nor turns anything on by itself
  assert.deepEqual(out.handledLanguages, ["ojs", "mermaid", "dot", "klingon"]);
  assert.equal(
    qnrRunOptions(baseOptions({ ownedLanguages: ["julia"] })).emitUnits,
    undefined,
  );
});

test("engine_returns_deep_copied_notebook_unchanged_by_to_markdown", () => {
  const nb = {
    cells: [{ cell_type: "code", source: ["1"], metadata: { quarto: { index: 0 } } }],
    metadata: { kernelspec: { language: "julia" } },
  };
  const { notebook } = buildRecordResult(nb, UNITS);
  // toMarkdown mutates the original in place
  nb.cells[0].source = ["MUTATED"];
  nb.cells.push({ cell_type: "markdown", source: [], metadata: {} } as never);
  nb.metadata.kernelspec.language = "mutated";
  assert.notEqual(notebook, nb);
  assert.deepEqual(notebook, {
    cells: [{ cell_type: "code", source: ["1"], metadata: { quarto: { index: 0 } } }],
    metadata: { kernelspec: { language: "julia" } },
  });
});

test("engine_without_units_behaves_exactly_as_before", () => {
  const options = baseOptions({ ownedLanguages: ["julia"] });
  const out = qnrRunOptions(options);
  assert.ok(!("emitUnits" in out));
  assert.deepEqual(out, options);
  assert.deepEqual(buildRecordResult({ cells: [] }, undefined), {});
});

test("engine_returns_hash_definition_in_record_info", () => {
  const { recordInfo } = buildRecordResult({ cells: [] }, UNITS);
  assert.ok(recordInfo);
  assert.equal(recordInfo.outputKind, "raw");
  assert.equal(recordInfo.alignment, "positional");
  assert.deepEqual(recordInfo.hashDefinition, JULIA_HASH_DEFINITION);
  assert.deepEqual(JULIA_HASH_DEFINITION, {
    id: "julia",
    version: 1,
    granularity: "document",
    presentationCellOptions: DISPLAY_ONLY,
    presentationExecuteKeys: DISPLAY_ONLY,
    documentKeys: ["julia", "format", "fig-width", "fig-height", "fig-format", "fig-dpi"],
  });
});

test("declaration_lists_display_only_options_and_keeps_error_hashed", () => {
  for (const key of ["echo", "output", "include", "warning"]) {
    assert.ok(JULIA_HASH_DEFINITION.presentationCellOptions.includes(key), key);
    assert.ok(JULIA_HASH_DEFINITION.presentationExecuteKeys.includes(key), key);
  }
  // `error` decides whether a failing cell aborts the render: it must stay hashed
  assert.ok(!JULIA_HASH_DEFINITION.presentationCellOptions.includes("error"));
  assert.ok(!JULIA_HASH_DEFINITION.presentationExecuteKeys.includes("error"));
  // an option the engine runs on can never be display-only
  for (const key of ["eval", "cache", "fig-width", "fig-height", "fig-format", "fig-dpi"]) {
    assert.ok(!JULIA_HASH_DEFINITION.presentationCellOptions.includes(key), key);
  }
});

test("record_info_acknowledges_core_presentation_exactly_when_offered", () => {
  const ack = buildRecordResult({ cells: [] }, UNITS, { presentedBy: "core", units: UNITS });
  assert.equal(ack.recordInfo?.presentedBy, "core");
  // units without the offer: the engine presented, so no acknowledgement (and no key)
  const noOffer = buildRecordResult({ cells: [] }, UNITS, { units: UNITS });
  assert.ok(noOffer.recordInfo);
  assert.ok(!("presentedBy" in noOffer.recordInfo));
  // the old two-argument call shape still works and does not acknowledge
  const legacy = buildRecordResult({ cells: [] }, UNITS);
  assert.ok(legacy.recordInfo);
  assert.ok(!("presentedBy" in legacy.recordInfo));
  // any other value is not an offer
  const other = buildRecordResult({ cells: [] }, UNITS, { presentedBy: "engine", units: UNITS });
  assert.ok(!("presentedBy" in other.recordInfo!));
  // no units: no record at all
  assert.deepEqual(buildRecordResult({ cells: [] }, undefined, { presentedBy: "core" }), {});
});

test("engine_uses_core_located_lines", () => {
  // core re-located these lines into the text this engine received; the engine
  // forwards them and never recomputes a position
  const run = qnrRunOptions(baseOptions({ units: UNITS }));
  assert.deepEqual(run.units, UNITS);
  assert.deepEqual(
    (run.units as typeof UNITS).map((u) => u.line),
    [7, 12, 13],
  );
  const { recordInfo } = buildRecordResult({ cells: [] }, UNITS);
  assert.ok(recordInfo);
});

test("core_presents_only_when_it_asks_and_sends_units", () => {
  // presentedBy "core" with units: the engine returns the notebook and no markdown
  assert.equal(coreWillPresent({ units: UNITS, presentedBy: "core" }), true);
  // an older core sends neither
  assert.equal(coreWillPresent({}), false);
  // units without the request: the engine still presents, as before
  assert.equal(coreWillPresent({ units: UNITS }), false);
  // the request without units would leave nothing to present from
  assert.equal(coreWillPresent({ presentedBy: "core" }), false);
  // any other value is not a request core presents
  assert.equal(coreWillPresent({ units: UNITS, presentedBy: "engine" }), false);
});
