import { describe, it, expect } from 'vitest';
import { sortProjectItems, type SortOrder } from './projectSort';

interface Item {
  description: string;
  lastAccessed: string;
}

const mk = (description: string, lastAccessed: string): Item => ({ description, lastAccessed });

const alpha = mk('Alpha', '2026-07-10T10:00:00.000Z');
const bravo = mk('bravo', '2026-07-12T10:00:00.000Z');
const charlie = mk('Charlie', '2026-07-08T10:00:00.000Z');

describe('sortProjectItems', () => {
  it('sorts newest first by lastAccessed', () => {
    const out = sortProjectItems([alpha, bravo, charlie], 'newest');
    expect(out.map((i) => i.description)).toEqual(['bravo', 'Alpha', 'Charlie']);
  });

  it('sorts oldest first by lastAccessed', () => {
    const out = sortProjectItems([alpha, bravo, charlie], 'oldest');
    expect(out.map((i) => i.description)).toEqual(['Charlie', 'Alpha', 'bravo']);
  });

  it('sorts by name case-insensitively (locale compare)', () => {
    const out = sortProjectItems([charlie, bravo, alpha], 'name');
    expect(out.map((i) => i.description)).toEqual(['Alpha', 'bravo', 'Charlie']);
  });

  it('returns a new array and leaves the input untouched', () => {
    const input = [alpha, bravo, charlie];
    const snapshot = [...input];
    const out = sortProjectItems(input, 'name');
    expect(out).not.toBe(input);
    expect(input).toEqual(snapshot);
  });

  it('handles empty lists', () => {
    for (const order of ['newest', 'oldest', 'name'] as SortOrder[]) {
      expect(sortProjectItems([], order)).toEqual([]);
    }
  });

  it('keeps items with identical timestamps without throwing', () => {
    const twin1 = mk('twin-1', '2026-07-01T00:00:00.000Z');
    const twin2 = mk('twin-2', '2026-07-01T00:00:00.000Z');
    const out = sortProjectItems([twin1, twin2], 'newest');
    expect(out.map((i) => i.description).sort()).toEqual(['twin-1', 'twin-2']);
  });
});
