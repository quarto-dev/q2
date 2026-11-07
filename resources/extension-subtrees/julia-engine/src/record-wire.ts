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

/** The engine's declared execution-input hash rule (plan EXR-8, task B1a). */
export const JULIA_HASH_DEFINITION = {
  id: "julia",
  version: 1,
  granularity: "document" as const,
  presentationCellOptions: [] as string[],
  presentationExecuteKeys: [] as string[],
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
}

/**
 * The options sent to QuartoNotebookRunner. `handledLanguages` passes through
 * untouched (core already added the languages of spans no engine owns);
 * `ownedLanguages` is not used. Core-located `units` are forwarded as given.
 */
export function qnrRunOptions<T extends object>(
  options: T,
): T & { emitUnits?: true } {
  const hasUnits = Array.isArray((options as { units?: unknown }).units);
  return hasUnits ? { ...options, emitUnits: true } : { ...options };
}

/**
 * What to add to the engine result for a record. Call BEFORE `toMarkdown`, which
 * mutates `nb` in place. Returns `{}` when core sent no units.
 */
export function buildRecordResult<N>(
  nb: N,
  units: unknown[] | undefined,
): { notebook?: N; recordInfo?: RecordInfo } {
  if (!Array.isArray(units)) {
    return {};
  }
  return {
    notebook: structuredClone(nb),
    recordInfo: {
      version: 1,
      granularity: "unit",
      outputKind: "raw",
      // QNR's index is positional; it never sees core's unit ids.
      alignment: "positional",
      hashDefinition: JULIA_HASH_DEFINITION,
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
