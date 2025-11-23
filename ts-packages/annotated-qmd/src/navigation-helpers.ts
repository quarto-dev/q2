/**
 * Navigation Helpers for AnnotatedParse List Structures
 *
 * Helper functions to navigate flattened list structures in AnnotatedParse.
 * BulletList, OrderedList, and DefinitionList flatten their nested structure
 * into the components array. These helpers reconstruct the logical structure
 * by correlating with the result field.
 */

import type { AnnotatedParse } from './types.js';

/**
 * Extract list items from a BulletList AnnotatedParse
 *
 * BulletList has structure: { c: Block[][] } where each Block[] is an item.
 * The components array flattens all blocks in document order.
 * This function groups them back by item.
 *
 * @param bulletList - AnnotatedParse with kind='BulletList'
 * @returns Array of items, where each item is an array of AnnotatedParse blocks
 * @throws Error if bulletList.kind is not 'BulletList'
 */
export function getListItems(bulletList: AnnotatedParse): AnnotatedParse[][] {
  if (bulletList.kind !== 'BulletList') {
    throw new Error(`Expected BulletList, got ${bulletList.kind}`);
  }

  // result field contains the original Pandoc structure: Block[][]
  const items = bulletList.result as unknown[][];
  if (!Array.isArray(items)) {
    throw new Error('BulletList result is not an array');
  }

  // Reconstruct items by slicing components based on item lengths
  const reconstructedItems: AnnotatedParse[][] = [];
  let componentIndex = 0;

  for (const item of items) {
    if (!Array.isArray(item)) {
      throw new Error('BulletList item is not an array');
    }

    const itemLength = item.length;
    const itemComponents = bulletList.components.slice(
      componentIndex,
      componentIndex + itemLength
    );
    reconstructedItems.push(itemComponents);
    componentIndex += itemLength;
  }

  return reconstructedItems;
}

/**
 * Extract list items from an OrderedList AnnotatedParse
 *
 * OrderedList has structure: { c: [ListAttributes, Block[][]] }
 * where Block[][] contains the items.
 * The components array flattens all blocks in document order.
 * This function groups them back by item.
 *
 * @param orderedList - AnnotatedParse with kind='OrderedList'
 * @returns Array of items, where each item is an array of AnnotatedParse blocks
 * @throws Error if orderedList.kind is not 'OrderedList'
 */
export function getOrderedListItems(orderedList: AnnotatedParse): AnnotatedParse[][] {
  if (orderedList.kind !== 'OrderedList') {
    throw new Error(`Expected OrderedList, got ${orderedList.kind}`);
  }

  // result field contains: [ListAttributes, Block[][]]
  const result = orderedList.result as unknown[];
  if (!Array.isArray(result) || result.length !== 2) {
    throw new Error('OrderedList result is not [ListAttributes, Block[][]]');
  }

  const items = result[1] as unknown[][];
  if (!Array.isArray(items)) {
    throw new Error('OrderedList items is not an array');
  }

  // Reconstruct items by slicing components based on item lengths
  const reconstructedItems: AnnotatedParse[][] = [];
  let componentIndex = 0;

  for (const item of items) {
    if (!Array.isArray(item)) {
      throw new Error('OrderedList item is not an array');
    }

    const itemLength = item.length;
    const itemComponents = orderedList.components.slice(
      componentIndex,
      componentIndex + itemLength
    );
    reconstructedItems.push(itemComponents);
    componentIndex += itemLength;
  }

  return reconstructedItems;
}

/**
 * Entry in a definition list
 */
export interface DefinitionListEntry {
  /** Term as array of AnnotatedParse inline elements */
  term: AnnotatedParse[];
  /** Definitions, where each definition is an array of AnnotatedParse block elements */
  definitions: AnnotatedParse[][];
}

/**
 * Extract definition list entries from a DefinitionList AnnotatedParse
 *
 * DefinitionList has structure: { c: [Inline[], Block[][]][] }
 * where each tuple is (term, definitions).
 * The components array flattens: term inlines, then all definition blocks.
 * This function reconstructs the logical structure.
 *
 * @param defList - AnnotatedParse with kind='DefinitionList'
 * @returns Array of entries with term and definitions
 * @throws Error if defList.kind is not 'DefinitionList'
 */
export function getDefinitionListEntries(defList: AnnotatedParse): DefinitionListEntry[] {
  if (defList.kind !== 'DefinitionList') {
    throw new Error(`Expected DefinitionList, got ${defList.kind}`);
  }

  // result field contains: [Inline[], Block[][]][]
  const entries = defList.result as unknown[];
  if (!Array.isArray(entries)) {
    throw new Error('DefinitionList result is not an array');
  }

  // Reconstruct entries by slicing components
  const reconstructedEntries: DefinitionListEntry[] = [];
  let componentIndex = 0;

  for (const entry of entries) {
    if (!Array.isArray(entry) || entry.length !== 2) {
      throw new Error('DefinitionList entry is not [Inline[], Block[][]]');
    }

    const [termInlines, definitions] = entry as [unknown[], unknown[][]];

    if (!Array.isArray(termInlines)) {
      throw new Error('DefinitionList term is not an array');
    }
    if (!Array.isArray(definitions)) {
      throw new Error('DefinitionList definitions is not an array');
    }

    // Extract term components
    const termLength = termInlines.length;
    const term = defList.components.slice(componentIndex, componentIndex + termLength);
    componentIndex += termLength;

    // Extract definition components (each definition is Block[])
    const reconstructedDefinitions: AnnotatedParse[][] = [];
    for (const definition of definitions) {
      if (!Array.isArray(definition)) {
        throw new Error('DefinitionList definition is not an array');
      }

      const defLength = definition.length;
      const defComponents = defList.components.slice(
        componentIndex,
        componentIndex + defLength
      );
      reconstructedDefinitions.push(defComponents);
      componentIndex += defLength;
    }

    reconstructedEntries.push({
      term,
      definitions: reconstructedDefinitions
    });
  }

  return reconstructedEntries;
}
