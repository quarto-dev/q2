import { Repo, type DocHandle } from '@automerge/automerge-repo/slim';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';
import { TextBindingCore, computeReplace, type BufferText } from './text-binding';
import { ensureWasm } from './wasm';

describe('computeReplace (the prefix and suffix diff)', () => {
  it.each([
    ['equal', 'abc', 'abc', null],
    ['insert in the middle', 'abcd', 'abXcd', { start: 2, end: 2, text: 'X' }],
    ['delete in the middle', 'abXcd', 'abcd', { start: 2, end: 3, text: '' }],
    ['replace in the middle', 'abcde', 'abXde', { start: 2, end: 3, text: 'X' }],
    ['append', 'abc', 'abcdef', { start: 3, end: 3, text: 'def' }],
    ['prepend', 'abc', 'xyabc', { start: 0, end: 0, text: 'xy' }],
    ['to empty', 'abc', '', { start: 0, end: 3, text: '' }],
    ['from empty', '', 'abc', { start: 0, end: 0, text: 'abc' }],
    // The suffix scan must not overlap the prefix: "aa" -> "aaa".
    ['repeated characters', 'aa', 'aaa', { start: 2, end: 2, text: 'a' }],
    ['repeated characters, shrinking', 'aaa', 'aa', { start: 2, end: 3, text: '' }],
  ])('%s', (_name, current, target, expected) => {
    expect(computeReplace(current, target)).toEqual(expected);
  });
});

/** An in-memory buffer; `applyReplace` can be made to yield, to model an editor apply. */
class FakeBuffer implements BufferText {
  constructor(public text: string) {}
  applyCalls: { start: number; end: number; text: string }[] = [];
  /** Runs once in the middle of the next apply (the editor is async). */
  duringApply: (() => void) | null = null;
  getText(): string {
    return this.text;
  }
  async applyReplace(start: number, end: number, text: string): Promise<void> {
    this.applyCalls.push({ start, end, text });
    await Promise.resolve();
    this.duringApply?.();
    this.duringApply = null;
    this.text = this.text.slice(0, start) + text + this.text.slice(end);
  }
}

let repo: Repo;
beforeAll(async () => {
  await ensureWasm();
});
afterEach(async () => {
  await repo?.shutdown().catch(() => {});
});

function makeHandle(text: string): DocHandle<{ text: string }> {
  repo = new Repo();
  return repo.create({ text });
}

const settle = () => new Promise((r) => setTimeout(r, 20));

describe('TextBindingCore', () => {
  it('writes a remote change into the buffer as one replace over the changed span', async () => {
    const handle = makeHandle('hello world');
    const buffer = new FakeBuffer('hello world');
    const binding = new TextBindingCore(handle, buffer);
    binding.start();
    handle.change((d) => {
      d.text = 'hello brave world';
    });
    await settle();
    expect(buffer.text).toBe('hello brave world');
    expect(buffer.applyCalls).toEqual([{ start: 6, end: 6, text: 'brave ' }]);
    binding.dispose();
  });

  it('catches a buffer that lags the doc at bind time', async () => {
    const handle = makeHandle('new text');
    const buffer = new FakeBuffer('old text');
    new TextBindingCore(handle, buffer).start();
    await settle();
    expect(buffer.text).toBe('new text');
  });

  it('splices local edits into the doc, several changes in descending offset order', () => {
    const handle = makeHandle('0123456789');
    const buffer = new FakeBuffer('0123456789');
    const binding = new TextBindingCore(handle, buffer);
    binding.start();
    // One editor event, offsets against the pre-edit text, descending. The
    // editor has already applied it to the buffer when the event fires.
    buffer.text = '01ab234567X9';
    binding.onLocalEdit([
      { rangeOffset: 8, rangeLength: 1, text: 'X' },
      { rangeOffset: 2, rangeLength: 0, text: 'ab' },
    ]);
    expect(handle.doc()!.text).toBe('01ab234567X9');
  });

  it('does not echo a remote apply back into the doc (the echo guard)', async () => {
    const handle = makeHandle('abc');
    const buffer = new FakeBuffer('abc');
    const binding = new TextBindingCore(handle, buffer);
    binding.start();
    const before = handle.heads();
    // The editor reports the apply as a change event while the guard is up.
    buffer.duringApply = () => binding.onLocalEdit([{ rangeOffset: 3, rangeLength: 0, text: 'ZZ' }]);
    handle.change((d) => {
      d.text = 'abcd';
    });
    await settle();
    // The guarded event was dropped: the doc has only the remote edit.
    expect(handle.doc()!.text).toBe('abcd');
    expect(handle.heads()).not.toEqual(before);
    expect(binding.applying).toBe(false);
  });

  it('runs one more convergence pass when a keystroke landed mid-apply', async () => {
    const handle = makeHandle('abc');
    const buffer = new FakeBuffer('abc');
    const binding = new TextBindingCore(handle, buffer);
    binding.start();
    // Mid-apply the user's keystroke reaches the buffer but is dropped by the
    // guard, so the doc and the buffer disagree afterwards.
    buffer.duringApply = () => {
      buffer.text = 'k' + buffer.text; // the keystroke reaches the buffer ...
      binding.onLocalEdit([{ rangeOffset: 0, rangeLength: 0, text: 'k' }]); // ... but the guard drops it
    };
    handle.change((d) => {
      d.text = 'abcd';
    });
    await settle();
    // The extra pass makes the buffer converge on the doc (the keystroke is
    // lost, as today): two applies, the second one the convergence pass.
    expect(handle.doc()!.text).toBe('abcd');
    expect(buffer.text).toBe('abcd');
    expect(buffer.applyCalls).toHaveLength(2);
  });

  it('stops listening after dispose', async () => {
    const handle = makeHandle('abc');
    const buffer = new FakeBuffer('abc');
    const binding = new TextBindingCore(handle, buffer);
    binding.start();
    binding.dispose();
    handle.change((d) => {
      d.text = 'abcd';
    });
    await settle();
    expect(buffer.text).toBe('abc');
  });

  describe('mirror check (G27)', () => {
    it('accepts an edit when the pre-edit buffer equals the mirror of the doc', () => {
      const handle = makeHandle('hello world');
      const buffer = new FakeBuffer('hello world');
      const binding = new TextBindingCore(handle, buffer);
      binding.start();
      buffer.text = 'hello brave world';
      binding.onLocalEdit([{ rangeOffset: 6, rangeLength: 0, text: 'brave ' }]);
      expect(handle.doc()!.text).toBe('hello brave world');
    });

    it('does not splice an edit whose pre-edit buffer differs, and reconciles the buffer by content', async () => {
      const handle = makeHandle('hello world');
      const buffer = new FakeBuffer('hello world');
      const binding = new TextBindingCore(handle, buffer);
      binding.start();
      const before = handle.heads();
      // A late reload: the buffer had already been rewritten to other text when
      // this edit event (offsets against that other text) arrived.
      buffer.text = 'something else entirely';
      buffer.text = 'X' + buffer.text;
      binding.onLocalEdit([{ rangeOffset: 0, rangeLength: 0, text: 'X' }]);
      expect(handle.heads()).toEqual(before);
      expect(handle.doc()!.text).toBe('hello world');
      await settle();
      expect(buffer.text).toBe('hello world');
    });

    it('updates the mirror after each remote apply, so the next local edit is accepted', async () => {
      const handle = makeHandle('abc');
      const buffer = new FakeBuffer('abc');
      const binding = new TextBindingCore(handle, buffer);
      binding.start();
      handle.change((d) => {
        d.text = 'abcdef';
      });
      await settle();
      expect(buffer.text).toBe('abcdef');
      buffer.text = 'abcdefg';
      binding.onLocalEdit([{ rangeOffset: 6, rangeLength: 0, text: 'g' }]);
      expect(handle.doc()!.text).toBe('abcdefg');
    });

    it('accepts consecutive local edits (the mirror follows each accepted edit)', () => {
      const handle = makeHandle('a');
      const buffer = new FakeBuffer('a');
      const binding = new TextBindingCore(handle, buffer);
      binding.start();
      buffer.text = 'ab';
      binding.onLocalEdit([{ rangeOffset: 1, rangeLength: 0, text: 'b' }]);
      buffer.text = 'abc';
      binding.onLocalEdit([{ rangeOffset: 2, rangeLength: 0, text: 'c' }]);
      expect(handle.doc()!.text).toBe('abc');
    });

    it('still converges when a keystroke lands during an apply', async () => {
      const handle = makeHandle('abc');
      const buffer = new FakeBuffer('abc');
      const binding = new TextBindingCore(handle, buffer);
      binding.start();
      buffer.duringApply = () => {
        buffer.text = 'k' + buffer.text;
        binding.onLocalEdit([{ rangeOffset: 0, rangeLength: 0, text: 'k' }]);
      };
      handle.change((d) => {
        d.text = 'abcd';
      });
      await settle();
      expect(buffer.text).toBe('abcd');
      expect(handle.doc()!.text).toBe('abcd');
      // and a later keystroke is accepted
      buffer.text = 'abcde';
      binding.onLocalEdit([{ rangeOffset: 4, rangeLength: 0, text: 'e' }]);
      expect(handle.doc()!.text).toBe('abcde');
    });
  });
});
