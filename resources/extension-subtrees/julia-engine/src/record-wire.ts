/*
 * record-wire.ts
 *
 * q2 execution-record wire handling for the Julia engine (EXR-8 B1). Pure: no
 * socket, no julia, no quarto internals, so it is unit-testable on its own.
 *
 * Core sends `units` (EXR-7) when it wants a record. The engine then:
 *   - asks QuartoNotebookRunner to annotate its cells (`emitUnits`);
 *   - returns a deep copy of the raw notebook (taken before `toMarkdown`
 *     mutates it in place) plus a `recordInfo` describing it.
 * Without `units` everything is exactly as before.
 */

/**
 * Options that only change how a cell is presented, never what Julia computes. Declaring
 * them lets core read "in the hash" as "capture enforced it" (OPR-2): an option listed here
 * is applied by core's presenter, one not listed was applied at capture. The set is the
 * Jupyter engine's `DISPLAY_ONLY_KEYS` (crates/quarto-core/src/engine/jupyter/mod.rs), since
 * both run notebooks through the same presenter. `error` is deliberately absent: it decides
 * whether a failing cell aborts the render. `eval` and the figure-size keys change what runs.
 *
 * Cost: QuartoNotebookRunner hashes the whole front matter, and any chunk with a span, for
 * its own cache, so editing a display-only key can still make the runner re-run. The record
 * stays fresh in core; the cost is time, never stale output.
 */
const DISPLAY_ONLY_KEYS: string[] = [
  // Visibility.
  "echo",
  "output",
  "warning",
  "include",
  // Code display.
  "code-fold",
  "code-summary",
  "code-overflow",
  "code-line-numbers",
  "code-copy",
  // Captions, listing and layout.
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
  // Cell wrapper attributes the presenter writes (KNR-3 slice 7, D10).
  "panel",
  "column",
  "keep-hidden",
  // Image attributes the presenter writes (KNR-3 slice 7, D6, D10).
  "fig-env",
  "fig-pos",
  "out-width",
  "out-height",
  "out-extra",
  // Cell classes the presenter writes (KNR-3 slice 7, D10).
  "classes",
  "fig-column",
  "tbl-column",
  // Engine-neutral: whether core's presenter interleaves source and output. Julia does not
  // declare interleaving, so a cell is held unless the author sets this to true.
  "output-interleave",
];

/** The engine's declared execution-input hash rule (plan EXR-8, task B1a; OPR-3; version 2: OPR-5 excludes `execute: record-sources`). */
export const JULIA_HASH_DEFINITION = {
  id: "julia",
  version: 2,
  granularity: "document" as const,
  presentationCellOptions: [...DISPLAY_ONLY_KEYS],
  // `execute: record-sources` (OPR-5) says where a record comes from, so it is not an input.
  presentationExecuteKeys: [...DISPLAY_ONLY_KEYS, "record-sources"],
  documentKeys: [
    "julia",
    "format",
    "fig-width",
    "fig-height",
    "fig-format",
    "fig-dpi",
  ],
};

export interface RecordInfo {
  version: number;
  granularity: "unit" | "document";
  outputKind: "raw" | "rendered";
  alignment: "ids" | "positional";
  hashDefinition: typeof JULIA_HASH_DEFINITION;
  /** Present exactly when this engine honored core's `presentedBy: "core"` offer (D8). */
  presentedBy?: "core";
  /**
   * What kind of page-dependency data the outputs carry (OPR-7i). `"jupyter-outputs"`: the cell
   * outputs are nbformat outputs, and core derives requirejs, jQuery and hoisted libraries from
   * their MIME types. Present only together with `presentedBy`.
   */
  dependencyData?: "jupyter-outputs";
}

/**
 * The options sent to QuartoNotebookRunner. `handledLanguages` passes through
 * untouched (core already added the languages of spans no engine owns);
 * `ownedLanguages` is not used. Core-located `units` are forwarded as given.
 * Quarto 2's `executeEnv` (per-document variables and the project environment)
 * becomes QNR's per-run `env`, which QNR sets around cell and inline evaluation
 * and replaces on every run; `executeEnv` itself is not forwarded, and no `env`
 * key is sent when it is empty or absent.
 */
export function qnrRunOptions<T extends object>(
  options: T,
): Omit<T, "executeEnv"> & { emitUnits?: true; env?: Record<string, string> } {
  const { executeEnv, ...rest } = options as T & {
    executeEnv?: Record<string, string>;
  };
  const hasUnits = Array.isArray((rest as { units?: unknown }).units);
  const hasEnv = executeEnv !== undefined && Object.keys(executeEnv).length > 0;
  return {
    ...rest,
    ...(hasUnits ? { emitUnits: true as const } : {}),
    ...(hasEnv ? { env: { ...executeEnv } } : {}),
  } as Omit<T, "executeEnv"> & { emitUnits?: true; env?: Record<string, string> };
}

/**
 * What to add to the engine result for a record. Call BEFORE `toMarkdown`, which
 * mutates `nb` in place. Returns `{}` when core sent no units.
 */
export function buildRecordResult<N>(
  nb: N,
  units: unknown[] | undefined,
  options?: object,
): { notebook?: N; recordInfo?: RecordInfo } {
  if (!Array.isArray(units)) {
    return {};
  }
  // The acknowledgement mirrors the decision the engine acts on (`coreWillPresent`), so
  // core can present from the record exactly when the engine skipped its own markdown.
  const presents = options !== undefined && coreWillPresent(options);
  const presentedBy = presents ? { presentedBy: "core" as const } : {};
  // Declared only when core presents (OPR-7i, decision 8). When the engine presents itself,
  // `toMarkdown` has already put these page dependencies into the result's includes, and a
  // label would make core derive them a second time.
  const dependencyData = presents ? { dependencyData: "jupyter-outputs" as const } : {};
  return {
    notebook: structuredClone(nb),
    recordInfo: {
      version: 1,
      granularity: "unit",
      outputKind: "raw",
      // QNR's index is positional; it never sees core's unit ids.
      alignment: "positional",
      hashDefinition: JULIA_HASH_DEFINITION,
      ...presentedBy,
      ...dependencyData,
    },
  };
}

/**
 * Whether core presents this run's output (`presentedBy: "core"`, EXR-8 B4). It also
 * needs `units`: without them there is no record, so nothing for core to present from.
 * Then the engine skips `toMarkdown` and returns only the notebook; an older core sends
 * neither field and the engine presents as before.
 */
export function coreWillPresent(options: object): boolean {
  const o = options as { units?: unknown; presentedBy?: unknown };
  return Array.isArray(o.units) && o.presentedBy === "core";
}
