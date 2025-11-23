/**
 * Tests for navigation-helpers.ts
 */

import { describe, it } from 'node:test';
import { strict as assert } from 'node:assert';
import { getListItems, getOrderedListItems, getDefinitionListEntries } from '../src/navigation-helpers.js';
import type { AnnotatedParse } from '../src/types.js';
import { asMappedString } from '@quarto/mapped-string';

describe('navigation-helpers', () => {
  describe('getListItems', () => {
    it('should extract items from a BulletList', () => {
      // Create a BulletList with 3 items: [1 block], [2 blocks], [1 block]
      const bulletList: AnnotatedParse = {
        kind: 'BulletList',
        result: [
          [{t: 'Plain', c: []}],  // Item 1: 1 block
          [{t: 'Para', c: []}, {t: 'Plain', c: []}],  // Item 2: 2 blocks
          [{t: 'Para', c: []}]   // Item 3: 1 block
        ],
        source: asMappedString('- item 1\n- item 2\n  more\n- item 3'),
        components: [
          // Item 1
          { kind: 'Plain', result: [], source: asMappedString('item 1'), components: [], start: 2, end: 8 },
          // Item 2
          { kind: 'Para', result: [], source: asMappedString('item 2'), components: [], start: 11, end: 17 },
          { kind: 'Plain', result: [], source: asMappedString('more'), components: [], start: 20, end: 24 },
          // Item 3
          { kind: 'Para', result: [], source: asMappedString('item 3'), components: [], start: 27, end: 33 }
        ],
        start: 0,
        end: 33
      };

      const items = getListItems(bulletList);

      assert.equal(items.length, 3);
      assert.equal(items[0].length, 1);
      assert.equal(items[0][0].kind, 'Plain');
      assert.equal(items[1].length, 2);
      assert.equal(items[1][0].kind, 'Para');
      assert.equal(items[1][1].kind, 'Plain');
      assert.equal(items[2].length, 1);
      assert.equal(items[2][0].kind, 'Para');
    });

    it('should handle empty BulletList', () => {
      const bulletList: AnnotatedParse = {
        kind: 'BulletList',
        result: [],
        source: asMappedString(''),
        components: [],
        start: 0,
        end: 0
      };

      const items = getListItems(bulletList);
      assert.equal(items.length, 0);
    });

    it('should handle BulletList with single-item', () => {
      const bulletList: AnnotatedParse = {
        kind: 'BulletList',
        result: [
          [{t: 'Plain', c: []}]
        ],
        source: asMappedString('- item'),
        components: [
          { kind: 'Plain', result: [], source: asMappedString('item'), components: [], start: 2, end: 6 }
        ],
        start: 0,
        end: 6
      };

      const items = getListItems(bulletList);
      assert.equal(items.length, 1);
      assert.equal(items[0].length, 1);
    });

    it('should throw error for wrong kind', () => {
      const notAList: AnnotatedParse = {
        kind: 'Para',
        result: [],
        source: asMappedString('paragraph'),
        components: [],
        start: 0,
        end: 9
      };

      assert.throws(
        () => getListItems(notAList),
        /Expected BulletList, got Para/
      );
    });
  });

  describe('getOrderedListItems', () => {
    it('should extract items from an OrderedList', () => {
      // Create an OrderedList with 2 items: [1 block], [2 blocks]
      const orderedList: AnnotatedParse = {
        kind: 'OrderedList',
        result: [
          [1, { t: 'Decimal' }, { t: 'Period' }],  // ListAttributes
          [
            [{t: 'Plain', c: []}],  // Item 1: 1 block
            [{t: 'Para', c: []}, {t: 'Plain', c: []}]  // Item 2: 2 blocks
          ]
        ],
        source: asMappedString('1. first\n2. second\n   more'),
        components: [
          // Item 1
          { kind: 'Plain', result: [], source: asMappedString('first'), components: [], start: 3, end: 8 },
          // Item 2
          { kind: 'Para', result: [], source: asMappedString('second'), components: [], start: 12, end: 18 },
          { kind: 'Plain', result: [], source: asMappedString('more'), components: [], start: 22, end: 26 }
        ],
        start: 0,
        end: 26
      };

      const items = getOrderedListItems(orderedList);

      assert.equal(items.length, 2);
      assert.equal(items[0].length, 1);
      assert.equal(items[0][0].kind, 'Plain');
      assert.equal(items[1].length, 2);
      assert.equal(items[1][0].kind, 'Para');
      assert.equal(items[1][1].kind, 'Plain');
    });

    it('should handle empty OrderedList', () => {
      const orderedList: AnnotatedParse = {
        kind: 'OrderedList',
        result: [
          [1, { t: 'Decimal' }, { t: 'Period' }],
          []
        ],
        source: asMappedString(''),
        components: [],
        start: 0,
        end: 0
      };

      const items = getOrderedListItems(orderedList);
      assert.equal(items.length, 0);
    });

    it('should throw error for wrong kind', () => {
      const notAList: AnnotatedParse = {
        kind: 'Para',
        result: [],
        source: asMappedString('paragraph'),
        components: [],
        start: 0,
        end: 9
      };

      assert.throws(
        () => getOrderedListItems(notAList),
        /Expected OrderedList, got Para/
      );
    });
  });

  describe('getDefinitionListEntries', () => {
    it('should extract entries from a DefinitionList', () => {
      // Create a DefinitionList with 2 entries:
      // - Term1 (2 inlines) : Definition1 (1 block), Definition2 (2 blocks)
      // - Term2 (1 inline) : Definition3 (1 block)
      const defList: AnnotatedParse = {
        kind: 'DefinitionList',
        result: [
          [
            [{t: 'Str', c: 'Term'}, {t: 'Str', c: '1'}],  // Term1: 2 inlines
            [
              [{t: 'Plain', c: []}],  // Definition1: 1 block
              [{t: 'Para', c: []}, {t: 'Plain', c: []}]  // Definition2: 2 blocks
            ]
          ],
          [
            [{t: 'Str', c: 'Term2'}],  // Term2: 1 inline
            [
              [{t: 'Para', c: []}]  // Definition3: 1 block
            ]
          ]
        ],
        source: asMappedString('Term1\n:   def1\n:   def2\n    more\n\nTerm2\n:   def3'),
        components: [
          // Entry 1: Term1
          { kind: 'Str', result: 'Term', source: asMappedString('Term'), components: [], start: 0, end: 4 },
          { kind: 'Str', result: '1', source: asMappedString('1'), components: [], start: 4, end: 5 },
          // Entry 1: Definition1
          { kind: 'Plain', result: [], source: asMappedString('def1'), components: [], start: 10, end: 14 },
          // Entry 1: Definition2
          { kind: 'Para', result: [], source: asMappedString('def2'), components: [], start: 19, end: 23 },
          { kind: 'Plain', result: [], source: asMappedString('more'), components: [], start: 28, end: 32 },
          // Entry 2: Term2
          { kind: 'Str', result: 'Term2', source: asMappedString('Term2'), components: [], start: 34, end: 39 },
          // Entry 2: Definition3
          { kind: 'Para', result: [], source: asMappedString('def3'), components: [], start: 44, end: 48 }
        ],
        start: 0,
        end: 48
      };

      const entries = getDefinitionListEntries(defList);

      assert.equal(entries.length, 2);

      // Entry 1
      assert.equal(entries[0].term.length, 2);
      assert.equal(entries[0].term[0].kind, 'Str');
      assert.equal(entries[0].term[1].kind, 'Str');
      assert.equal(entries[0].definitions.length, 2);
      assert.equal(entries[0].definitions[0].length, 1);  // Definition1: 1 block
      assert.equal(entries[0].definitions[0][0].kind, 'Plain');
      assert.equal(entries[0].definitions[1].length, 2);  // Definition2: 2 blocks
      assert.equal(entries[0].definitions[1][0].kind, 'Para');
      assert.equal(entries[0].definitions[1][1].kind, 'Plain');

      // Entry 2
      assert.equal(entries[1].term.length, 1);
      assert.equal(entries[1].term[0].kind, 'Str');
      assert.equal(entries[1].definitions.length, 1);
      assert.equal(entries[1].definitions[0].length, 1);  // Definition3: 1 block
      assert.equal(entries[1].definitions[0][0].kind, 'Para');
    });

    it('should handle empty DefinitionList', () => {
      const defList: AnnotatedParse = {
        kind: 'DefinitionList',
        result: [],
        source: asMappedString(''),
        components: [],
        start: 0,
        end: 0
      };

      const entries = getDefinitionListEntries(defList);
      assert.equal(entries.length, 0);
    });

    it('should handle DefinitionList with empty definitions', () => {
      const defList: AnnotatedParse = {
        kind: 'DefinitionList',
        result: [
          [
            [{t: 'Str', c: 'Term'}],  // Term: 1 inline
            []  // No definitions
          ]
        ],
        source: asMappedString('Term\n'),
        components: [
          { kind: 'Str', result: 'Term', source: asMappedString('Term'), components: [], start: 0, end: 4 }
        ],
        start: 0,
        end: 5
      };

      const entries = getDefinitionListEntries(defList);
      assert.equal(entries.length, 1);
      assert.equal(entries[0].term.length, 1);
      assert.equal(entries[0].definitions.length, 0);
    });

    it('should throw error for wrong kind', () => {
      const notAList: AnnotatedParse = {
        kind: 'Para',
        result: [],
        source: asMappedString('paragraph'),
        components: [],
        start: 0,
        end: 9
      };

      assert.throws(
        () => getDefinitionListEntries(notAList),
        /Expected DefinitionList, got Para/
      );
    });
  });
});
