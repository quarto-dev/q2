/*
 * is-marimo-cell.ts
 *
 * Detects whether a QuartoMdCell is a marimo code block.
 */

import type { QuartoMdCell } from "@quarto/types";

export function isMarimoCell(cell: QuartoMdCell): boolean {
  if (typeof cell.cell_type !== "object" || !("language" in cell.cell_type)) {
    return false;
  }
  const lang = cell.cell_type.language;
  // Handle {python.marimo}/{sql.marimo} syntax.
  if (lang === "python.marimo" || lang === "sql.marimo") {
    return true;
  }
  // Handle class syntax and legacy language-outside-braces syntax.
  if (lang === "python" || lang === "sql") {
    const firstLine = cell.sourceVerbatim.value.split('\n')[0] || '';
    return /\.marimo/.test(firstLine);
  }
  return false;
}

/**
 * Whether `execute()` should treat `cell` as owned by marimo for this
 * render — i.e. execute it and splice in the rendered output, rather than
 * passing it through unchanged.
 *
 * This is a NEW, net-new predicate (q2 plan4c 4c0-eng, B1) — do NOT fold its
 * logic into `isMarimoCell(cell)`. `isMarimoCell`'s only call site
 * (`marimo-engine.ts:271` at the time of writing) passes no ownership
 * information, and `isMarimoCell` alone is correct for every
 * `.marimo`-tagged cell shape (those are unconditionally marimo's, with no
 * presence-gating).
 *
 * `cellOwnedByMarimo` adds exactly one more case on top of `isMarimoCell`:
 * a bare `{sql}` cell (language `"sql"`, not `.marimo`-tagged) is owned only
 * when the caller's `handledLanguages` does NOT list `"sql"`. q2's
 * `handledLanguages` is the **leave-alone set** (q2's
 * `EngineResolution::handled_languages_for`, `resolution.rs:292`: q2's
 * built-in HANDLED_LANGUAGES ∪ languages owned by OTHER engines) — an
 * engine infers "assigned to me" as the complement of that set, not by
 * looking for its own name inside it. So `"sql"` absent from
 * `handledLanguages` means q2 did NOT leave sql alone for some other
 * engine, i.e. marimo owns it for this render (Option B — bare sql is an
 * Interop language that rides along only when marimo is already present as
 * a primary engine elsewhere in the doc; see claimsLanguage's `{kind:
 * "interop"}` return for bare sql). This is sound because q2's resolver
 * assigns every language present in the document an owner, or hard-fails,
 * before execute() ever runs — there is no "nobody owns this language"
 * case left standing by the time `handledLanguages` reaches here. Fixed
 * 2026-07-02 (q2 plan4c FINDING #4): the previous positive reading
 * (`handledLanguages.includes("sql")`) had it backwards — that evaluated
 * `true` only when q2 had left sql alone for some OTHER engine, which is
 * exactly the case where marimo does NOT own it.
 */
export function cellOwnedByMarimo(
  cell: QuartoMdCell,
  handledLanguages: string[],
): boolean {
  if (isMarimoCell(cell)) {
    return true;
  }
  if (typeof cell.cell_type !== "object" || !("language" in cell.cell_type)) {
    return false;
  }
  return (
    cell.cell_type.language === "sql" && !handledLanguages.includes("sql")
  );
}
