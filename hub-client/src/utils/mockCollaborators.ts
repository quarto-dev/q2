/**
 * Mock collaborators for the projects-home UI exploration
 * (explore/projects-shelves-ui).
 *
 * The Figma design shows per-project facepiles (colored disks with initials)
 * on cards, shelf headers, and the Peek popover. Real contributor data needs
 * automerge-history attribution (a later design phase), so the exploration
 * spoofs a stable fake crew per project: seeded from the indexDocId, so the
 * same project always shows the same faces across reloads.
 */

export interface MockUser {
  name: string;
  initials: string;
  color: string;
}

const POOL: MockUser[] = [
  { name: 'Charlotte Wu', initials: 'CW', color: '#E8368F' },
  { name: 'Saima Khan', initials: 'SK', color: '#00BCD4' },
  { name: 'Gordon West', initials: 'GW', color: '#FF9800' },
  { name: 'Maya Patel', initials: 'MP', color: '#4CAF50' },
  { name: 'Leo Ferreira', initials: 'LF', color: '#3F51B5' },
];

function hashString(s: string): number {
  let h = 0;
  for (let i = 0; i < s.length; i++) {
    h = (h * 31 + s.charCodeAt(i)) | 0;
  }
  return Math.abs(h);
}

/**
 * Deterministic fake collaborators for a project. `self` (the real user) is
 * always first; 1–3 mock users follow, chosen by hashing the doc id.
 */
export function mockCollaborators(indexDocId: string, self?: MockUser): MockUser[] {
  const h = hashString(indexDocId);
  const count = 1 + (h % 3);
  const others = new Map<string, MockUser>();
  for (let i = 0; i < count; i++) {
    const pick = POOL[(h >> (i * 4)) % POOL.length];
    others.set(pick.initials, pick);
  }
  return self ? [self, ...others.values()] : [...others.values()];
}

/** Union of collaborators across several projects, capped for shelf headers. */
export function unionCollaborators(lists: MockUser[][]): MockUser[] {
  const m = new Map<string, MockUser>();
  for (const list of lists) {
    for (const u of list) {
      if (!m.has(u.initials)) m.set(u.initials, u);
    }
  }
  return [...m.values()];
}
