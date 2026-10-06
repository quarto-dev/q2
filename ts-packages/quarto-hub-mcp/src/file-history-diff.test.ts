/**
 * Unit tests for formatUnifiedDiff (CAP-9): hunk grouping, context
 * windows, no-newline markers, and empty-text edges. The end-to-end
 * single-hunk path is pinned in file-history.test.ts.
 */

import { describe, it, expect } from 'vitest';
import { formatUnifiedDiff } from './file-history.js';

describe('formatUnifiedDiff', () => {
  it('splits distant changes into separate hunks', () => {
    const oldText = 'l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10\n';
    const newText = 'ONE\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nTEN\n';
    const { diff, addedLines, removedLines } = formatUnifiedDiff(oldText, newText, 'f.txt');
    expect(diff).toBe(
      '--- a/f.txt\n' +
        '+++ b/f.txt\n' +
        '@@ -1,4 +1,4 @@\n' +
        '-l1\n' +
        '+ONE\n' +
        ' l2\n' +
        ' l3\n' +
        ' l4\n' +
        '@@ -7,4 +7,4 @@\n' +
        ' l7\n' +
        ' l8\n' +
        ' l9\n' +
        '-l10\n' +
        '+TEN\n',
    );
    expect(addedLines).toBe(2);
    expect(removedLines).toBe(2);
  });

  it('merges nearby changes into one hunk', () => {
    const oldText = 'a\nb\nc\nd\n';
    const newText = 'A\nb\nc\nD\n';
    const { diff } = formatUnifiedDiff(oldText, newText, 'f.txt');
    // One change at line 1 and one at line 4: the 2-line gap is ≤ 2*context…
    expect(diff).toBe(
      '--- a/f.txt\n' +
        '+++ b/f.txt\n' +
        '@@ -1,4 +1,4 @@\n' +
        '-a\n' +
        '+A\n' +
        ' b\n' +
        ' c\n' +
        '-d\n' +
        '+D\n',
    );
  });

  it('flags a missing trailing newline on either side', () => {
    const { diff } = formatUnifiedDiff('old\n', 'new', 'f.txt');
    expect(diff).toBe(
      '--- a/f.txt\n' +
        '+++ b/f.txt\n' +
        '@@ -1,1 +1,1 @@\n' +
        '-old\n' +
        '+new\n' +
        '\\ No newline at end of file\n',
    );
  });

  it('handles empty-to-text and text-to-empty', () => {
    const grown = formatUnifiedDiff('', 'a\nb\n', 'f.txt');
    expect(grown.diff).toBe(
      '--- a/f.txt\n+++ b/f.txt\n@@ -1,0 +1,2 @@\n+a\n+b\n',
    );
    expect(grown.addedLines).toBe(2);
    expect(grown.removedLines).toBe(0);

    const shrunk = formatUnifiedDiff('a\nb\n', '', 'f.txt');
    expect(shrunk.diff).toBe(
      '--- a/f.txt\n+++ b/f.txt\n@@ -1,2 +1,0 @@\n-a\n-b\n',
    );
    expect(shrunk.addedLines).toBe(0);
    expect(shrunk.removedLines).toBe(2);
  });

  it('produces no hunks for identical texts', () => {
    const { diff, addedLines, removedLines } = formatUnifiedDiff('same\n', 'same\n', 'f.txt');
    expect(diff).toBe('--- a/f.txt\n+++ b/f.txt\n');
    expect(addedLines).toBe(0);
    expect(removedLines).toBe(0);
  });
});
