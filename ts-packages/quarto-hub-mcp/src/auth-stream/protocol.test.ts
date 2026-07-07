import { describe, it, expect, vi } from 'vitest';
import { parseCommand, runTokenStream, type OutFrame, type Token } from './protocol.js';

/** Build an async iterable from a fixed list of lines. */
async function* lines(...ls: string[]): AsyncIterable<string> {
  for (const l of ls) yield l;
}

describe('parseCommand', () => {
  it('recognizes a refresh command', () => {
    expect(parseCommand('{"type":"refresh"}')).toEqual({ type: 'refresh' });
    expect(parseCommand('  {"type":"refresh"}  \n')).toEqual({ type: 'refresh' });
  });

  it('ignores blank, non-JSON, and unknown commands', () => {
    expect(parseCommand('')).toBeNull();
    expect(parseCommand('   ')).toBeNull();
    expect(parseCommand('not json')).toBeNull();
    expect(parseCommand('{"type":"explode"}')).toBeNull();
    expect(parseCommand('42')).toBeNull();
  });
});

describe('runTokenStream', () => {
  const tok = (bearer: string): Token => ({ bearer, expiresAt: '2026-07-01T00:00:00.000Z' });

  it('emits the initial token before any command', async () => {
    const frames: OutFrame[] = [];
    await runTokenStream({
      getToken: async () => tok('initial'),
      forceRefresh: vi.fn(),
      input: lines(),
      emit: (f) => frames.push(f),
    });
    expect(frames).toEqual([
      { type: 'token', bearer: 'initial', expiresAt: '2026-07-01T00:00:00.000Z' },
    ]);
  });

  it('forces a refresh and emits the new token on a refresh command', async () => {
    const frames: OutFrame[] = [];
    const forceRefresh = vi.fn(async () => tok('refreshed'));
    await runTokenStream({
      getToken: async () => tok('initial'),
      forceRefresh,
      input: lines('{"type":"refresh"}'),
      emit: (f) => frames.push(f),
    });
    expect(forceRefresh).toHaveBeenCalledTimes(1);
    expect(frames.map((f) => (f.type === 'token' ? f.bearer : f.type))).toEqual([
      'initial',
      'refreshed',
    ]);
  });

  it('ignores unrecognized input lines (no extra frames, no refresh)', async () => {
    const frames: OutFrame[] = [];
    const forceRefresh = vi.fn(async () => tok('refreshed'));
    await runTokenStream({
      getToken: async () => tok('initial'),
      forceRefresh,
      input: lines('', 'garbage', '{"type":"other"}'),
      emit: (f) => frames.push(f),
    });
    expect(forceRefresh).not.toHaveBeenCalled();
    expect(frames).toHaveLength(1);
    expect(frames[0]).toMatchObject({ type: 'token', bearer: 'initial' });
  });

  it('emits an error frame and stops if the initial token cannot be obtained', async () => {
    const frames: OutFrame[] = [];
    const forceRefresh = vi.fn();
    await runTokenStream({
      getToken: async () => {
        throw new Error('reauth required');
      },
      forceRefresh,
      input: lines('{"type":"refresh"}'),
      emit: (f) => frames.push(f),
    });
    expect(frames).toEqual([{ type: 'error', message: 'reauth required' }]);
    // Fatal: we never start servicing commands.
    expect(forceRefresh).not.toHaveBeenCalled();
  });

  it('reports a failed refresh but keeps the stream alive', async () => {
    const frames: OutFrame[] = [];
    let calls = 0;
    const forceRefresh = vi.fn(async () => {
      calls += 1;
      if (calls === 1) throw new Error('network blip');
      return tok('recovered');
    });
    await runTokenStream({
      getToken: async () => tok('initial'),
      forceRefresh,
      input: lines('{"type":"refresh"}', '{"type":"refresh"}'),
      emit: (f) => frames.push(f),
    });
    expect(frames).toEqual([
      { type: 'token', bearer: 'initial', expiresAt: '2026-07-01T00:00:00.000Z' },
      { type: 'error', message: 'network blip' },
      { type: 'token', bearer: 'recovered', expiresAt: '2026-07-01T00:00:00.000Z' },
    ]);
  });
});
