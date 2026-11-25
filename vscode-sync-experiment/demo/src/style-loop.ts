/** Pure parts of the unstyled-preview loop (`demo/style-loop.mjs`): reading a story's events and tallying many runs. */

export interface Hit {
  who: string;
  reasons: string[];
  /** Still bare two seconds after it was first seen. */
  persisted: boolean;
}
export interface LoopRun {
  run: number;
  preset: string;
  /** The story's launcher exited 0. */
  passed: boolean;
  hits: Hit[];
}

/** The unstyled previews recorded in one story's `events.jsonl`. */
export function previewHits(jsonl: string): Hit[] {
  const hits: Hit[] = [];
  for (const raw of jsonl.split('\n')) {
    let e: { source?: string; kind?: string; details?: { who?: string; reasons?: string[] } };
    try {
      e = JSON.parse(raw);
    } catch {
      continue;
    }
    if (e.source !== 'preview') continue;
    if (e.kind === 'unstyled') hits.push({ who: e.details?.who ?? '?', reasons: e.details?.reasons ?? [], persisted: false });
    else if (e.kind === 'unstyled-persisted') {
      const h = [...hits].reverse().find((x) => x.who === (e.details?.who ?? '?'));
      if (h) h.persisted = true;
    }
  }
  return hits;
}

export function formatLoopTable(runs: LoopRun[]): string {
  const presets = [...new Set(runs.map((r) => r.preset))];
  const rows = [['story', 'runs', 'unstyled', 'persisted', 'failed', 'by side']];
  for (const p of presets) {
    const rs = runs.filter((r) => r.preset === p);
    const withHit = rs.filter((r) => r.hits.length > 0);
    const side: Record<string, number> = {};
    for (const r of withHit) for (const w of new Set(r.hits.map((h) => h.who))) side[w] = (side[w] ?? 0) + 1;
    rows.push([
      p,
      String(rs.length),
      String(withHit.length),
      String(rs.filter((r) => r.hits.some((h) => h.persisted)).length),
      String(rs.filter((r) => !r.passed).length),
      Object.entries(side).map(([w, n]) => `${w}: ${n}`).join(', ') || '-',
    ]);
  }
  const width = rows[0]!.map((_, i) => Math.max(...rows.map((r) => r[i]!.length)));
  return rows.map((r) => r.map((c, i) => c.padEnd(width[i]!)).join('  ').trimEnd()).join('\n');
}
