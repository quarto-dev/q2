import { describe, expect, it } from 'vitest';
import { bookInfoFrom } from './bookInfo';

describe('bookInfoFrom', () => {
  it('reads the resolver response: a chapter of a book', () => {
    expect(bookInfoFrom({ success: true, source: 'project', formats: [], book: { chapter: true, chapters: ['index.qmd', 'one.qmd'] } })).toEqual({
      chapter: true,
      chapters: ['index.qmd', 'one.qmd'],
    });
  });

  it('a page of the project that is not a chapter keeps chapter: false', () => {
    expect(bookInfoFrom({ success: true, source: 'document', formats: [], book: { chapter: false, chapters: ['a.qmd'] } })?.chapter).toBe(false);
  });

  it('is null outside a book, on a missing field, on a failed resolve and with no response', () => {
    expect(bookInfoFrom({ success: true, source: 'default', formats: [], book: null })).toBeNull();
    expect(bookInfoFrom({ success: true, source: 'default', formats: [] })).toBeNull();
    expect(bookInfoFrom({ success: false, error: 'x' })).toBeNull();
    expect(bookInfoFrom(null)).toBeNull();
  });
});
