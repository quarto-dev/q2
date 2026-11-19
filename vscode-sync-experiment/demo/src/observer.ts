/**
 * The observer (events and narration). `deriveEvents` and `narrate` are pure; the polling loop that feeds them lives in
 * the launcher. Narration text is a pure function of an event.
 */

export interface Decision {
  kind: string;
  time_ms: number;
  files: string[];
  step: string | null;
  discarded: number | null;
}
export interface StatusBody {
  tracking: boolean;
  paused: boolean;
  pending: boolean;
  markers: string[];
  decisions: Decision[];
}
export interface DemoEvent {
  kind: string;
  summary?: string;
  details: Record<string, unknown>;
}

const sameDecision = (a: Decision, b: Decision) => a.kind === b.kind && a.time_ms === b.time_ms && a.files.join('\0') === b.files.join('\0');

export function deriveEvents(prev: StatusBody | null, next: StatusBody, o: { prevFiles?: string[]; nextFiles?: string[] } = {}): DemoEvent[] {
  const out: DemoEvent[] = [];
  const known = prev?.decisions ?? [];
  for (const d of next.decisions) {
    if (!known.some((k) => sameDecision(k, d))) out.push({ kind: `decision:${d.kind}`, details: { ...d } });
  }
  if (prev && !prev.paused && next.paused) out.push({ kind: 'paused', details: { markers: next.markers } });
  if (prev && prev.paused && !next.paused) out.push({ kind: 'resumed', details: {} });
  if (o.prevFiles && o.nextFiles) {
    for (const f of o.prevFiles) if (!o.nextFiles.includes(f)) out.push({ kind: 'file-removed', details: { file: f } });
    for (const f of o.nextFiles) if (!o.prevFiles.includes(f)) out.push({ kind: 'file-added', details: { file: f } });
  }
  return out;
}

const plural = (n: number, w: string) => `${n} ${w}${n === 1 ? '' : 's'}`;

export function narrate(e: DemoEvent): string {
  const d = e.details as Partial<Decision> & { markers?: string[]; file?: string };
  const files = d.files ?? [];
  switch (e.kind) {
    case 'decision:flipped':
    case 'decision:switched':
      return `The hub switched branches: ${plural(files.length, 'file')} followed (${files.join(', ')}).`;
    case 'decision:retired':
      return `The hub retired ${files.join(', ')} and discarded ${plural(d.discarded ?? 0, 'unsaved edit')}.`;
    case 'decision:moved-forward':
      return `The hub moved forward without swapping (${files.join(', ')}).`;
    case 'decision:seeded':
      return 'The hub seeded the project from the working tree.';
    case 'paused':
      return `Sync paused: a ${(d.markers ?? []).join(', ')} is in progress.`;
    case 'resumed':
      return 'Sync resumed.';
    case 'file-removed':
      return `${d.file} left the project.`;
    case 'file-added':
      return `${d.file} joined the project.`;
    default:
      return e.kind.startsWith('decision:') ? `The hub decided: ${e.kind.slice(9)}.` : e.summary ?? e.kind;
  }
}
