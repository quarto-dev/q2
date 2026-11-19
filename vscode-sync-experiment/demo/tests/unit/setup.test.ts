import { describe, expect, it } from 'vitest';
import { setupCommands, missingToolAdvice, type Which } from '../../src/setup';

describe('setup (DM10)', () => {
  it('lists the rv sync, rv library, and uv sync invocations with their directories', () => {
    expect(setupCommands({ root: '/r', journal: '/r/journal' })).toEqual([
      { cmd: 'rv', args: ['sync'], cwd: '/r' },
      { cmd: 'rv', args: ['library'], cwd: '/r' },
      { cmd: 'uv', args: ['sync'], cwd: '/r/journal' },
    ]);
  });

  it('prints exact install advice for each missing tool, none when all are present', () => {
    const all: Which = () => true;
    expect(missingToolAdvice(all)).toEqual([]);
    const none: Which = () => false;
    const advice = missingToolAdvice(none).join('\n');
    expect(advice).toContain('brew install --cask r');
    expect(advice).toContain('rv');
    expect(advice).toContain('brew install uv');
    expect(advice).toContain('cargo build --bin q2');
    const onlyUv: Which = (t) => t !== 'uv';
    expect(missingToolAdvice(onlyUv)).toHaveLength(1);
  });
});
