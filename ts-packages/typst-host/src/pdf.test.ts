import { describe, expect, it } from 'vitest';
import { countPdfPages } from './pdf.ts';
import { text } from './fixtures.test-util.ts';

describe('countPdfPages', () => {
  it('reads the root page tree and ignores intermediate nodes', () => {
    const pdf = text('%PDF-1.7\n1 0 obj\n<<\n  /Type /Pages\n  /Parent 9 0 R\n  /Count 1\n>>\nendobj\n2 0 obj\n<<\n  /Type /Pages\n  /Count 3\n  /Kids [1 0 R]\n>>\nendobj\n');
    expect(countPdfPages(pdf)).toBe(3);
  });
  it('returns undefined when there is no page tree', () => {
    expect(countPdfPages(text('not a pdf'))).toBeUndefined();
  });
});
