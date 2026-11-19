/** Continuous invariant checks for free play (DM8). Pure: the observer builds the snapshot. */

export interface Snapshot {
  /** No git operation or swap in flight. Disk and follower checks apply only when quiet. */
  quiet: boolean;
  docs: { path: string; text: string }[];
  /** The hub's disk, and the doc its pointer names, by path. */
  hubDisk: Record<string, string>;
  pointerDocs: Record<string, string>;
  clones: { name: string; branch: string; hubBranch: string; diskRewritten: boolean }[];
  followers: { name: string; shownVersion: string; pointerVersion: string }[];
}

export interface Violation {
  id: 'conflict-marker' | 'disk-differs-from-pointer' | 'clone-disk-rewritten' | 'follower-stale';
  summary: string;
}

// A conflict marker is seven of < = > at line start followed by a space or end of line; a setext
// underline of `=======` alone is not one, so `=======` needs a `<<<<<<<` or `>>>>>>>` partner in the doc.
const OPEN = /^<{7}( |$)/m;
const CLOSE = /^>{7}( |$)/m;

export function checkInvariants(s: Snapshot): Violation[] {
  const out: Violation[] = [];
  for (const d of s.docs) {
    if (OPEN.test(d.text) || CLOSE.test(d.text)) out.push({ id: 'conflict-marker', summary: `conflict marker in ${d.path}` });
  }
  if (s.quiet) {
    for (const [p, text] of Object.entries(s.pointerDocs)) {
      if (s.hubDisk[p] !== text) out.push({ id: 'disk-differs-from-pointer', summary: `hub disk differs from the pointed doc for ${p}` });
    }
  }
  for (const c of s.clones) {
    if (c.branch !== c.hubBranch && c.diskRewritten) {
      out.push({ id: 'clone-disk-rewritten', summary: `${c.name} is on ${c.branch}, not ${c.hubBranch}, but its disk was rewritten` });
    }
  }
  if (s.quiet) {
    for (const f of s.followers) {
      if (f.shownVersion !== f.pointerVersion) out.push({ id: 'follower-stale', summary: `${f.name} shows ${f.shownVersion}, the pointer is at ${f.pointerVersion}` });
    }
  }
  return out;
}
