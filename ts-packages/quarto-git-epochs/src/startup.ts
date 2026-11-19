/** Startup rule (G10): per-file plan from a persisted `{head, blobs}` record. Pure. */

export interface StartupRecord {
  head: string;
  blobs: Record<string, string>;
  /** Paths the peer left detached; they stay detached across a restart while their blob is unchanged. */
  detached?: string[];
}

export interface StartupInput {
  record: StartupRecord | null;
  head: string;
  /** Participating files and their blob ids at HEAD now. */
  blobs: Record<string, string>;
}

export type StartupCase = 'reconcile' | 'arrive-differing' | 'arrive-all';
export type FileStartup = 'reconcile' | 'arrive' | 'stay-detached';

export interface StartupPlan {
  case: StartupCase;
  files: Record<string, FileStartup>;
}

export function startupDecision(input: StartupInput): StartupPlan {
  const { record, head, blobs } = input;
  const files: Record<string, FileStartup> = {};
  if (!record) {
    for (const p of Object.keys(blobs)) files[p] = 'arrive';
    return { case: 'arrive-all', files };
  }
  const detached = new Set(record.detached ?? []);
  const headSame = record.head === head;
  for (const [p, blob] of Object.entries(blobs)) {
    const unchanged = headSame || record.blobs[p] === blob;
    if (detached.has(p) && unchanged) files[p] = 'stay-detached';
    else files[p] = unchanged ? 'reconcile' : 'arrive';
  }
  return { case: headSame ? 'reconcile' : 'arrive-differing', files };
}
