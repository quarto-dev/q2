import type { ImportDiagnostic } from '../pandoc/importService';

export interface GroupedDiagnostics {
  errors: ImportDiagnostic[];
  warnings: ImportDiagnostic[];
  /** Info and note (a Rust `note` goes under info). Order within each group is the report's order. */
  info: ImportDiagnostic[];
}

/** Group a report by kind for display. */
export function groupDiagnostics(diagnostics: readonly ImportDiagnostic[]): GroupedDiagnostics {
  const g: GroupedDiagnostics = { errors: [], warnings: [], info: [] };
  for (const d of diagnostics) {
    if (d.kind === 'error') g.errors.push(d);
    else if (d.kind === 'warning') g.warnings.push(d);
    else g.info.push(d);
  }
  return g;
}
