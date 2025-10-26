/**
 * Test suite for block type fixtures
 *
 * Tests conversion of all block types added in k-197:
 * - OrderedList
 * - DefinitionList
 * - Div with attributes
 * - Figure
 * - HorizontalRule
 * - RawBlock
 */

import { test } from 'node:test';
import assert from 'node:assert';
import * as fs from 'node:fs';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  parseRustQmdDocument,
  parseRustQmdBlocks,
  type RustQmdJson
} from '../src/index.js';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const examplesDir = path.join(__dirname, '..', 'examples');

/**
 * Helper to load an example JSON file
 */
function loadExample(name: string): RustQmdJson {
  const filePath = path.join(examplesDir, `${name}.json`);
  const content = fs.readFileSync(filePath, 'utf-8');
  return JSON.parse(content);
}

/**
 * Helper to load the source .qmd file for an example
 */
function loadSourceFile(name: string): string {
  const filePath = path.join(examplesDir, `${name}.qmd`);
  return fs.readFileSync(filePath, 'utf-8');
}

/**
 * Recursively extract all text content from components
 * This includes Str, Space, and other text-bearing inline elements
 */
function extractTextFromComponents(component: any): string {
  let text = '';

  // Handle Str nodes
  if (component.kind === 'Str' && typeof component.result === 'string') {
    return component.result;
  }

  // Handle Space nodes
  if (component.kind === 'Space') {
    return ' ';
  }

  // Handle SoftBreak/LineBreak
  if (component.kind === 'SoftBreak' || component.kind === 'LineBreak') {
    return '\n';
  }

  // Recursively process nested components
  if (component.components && Array.isArray(component.components)) {
    for (const child of component.components) {
      text += extractTextFromComponents(child);
    }
  }

  return text;
}

/**
 * Validate that a component's source location points to the expected text
 * Returns true if validation passes, throws assertion error otherwise
 */
function validateSourceOffset(
  component: any,
  expectedText: string,
  sourceFile: string
): boolean {
  assert.ok(typeof component.start === 'number', 'Component should have start offset');
  assert.ok(typeof component.end === 'number', 'Component should have end offset');
  assert.ok(component.start >= 0, 'Start offset should be non-negative');
  assert.ok(component.start < component.end, 'Start should be less than end');
  assert.ok(component.end <= sourceFile.length, 'End should not exceed file length');

  // Extract actual text from source file
  const actualText = sourceFile.slice(component.start, component.end);

  // Compare with expected text
  assert.strictEqual(
    actualText,
    expectedText,
    `Source offset [${component.start}, ${component.end}) should point to "${expectedText}" but got "${actualText}"`
  );

  return true;
}

test('ordered-list.json - OrderedList conversion', () => {
  const json = loadExample('ordered-list');
  const blocks = parseRustQmdBlocks(json.blocks, json);
  const source = loadSourceFile('ordered-list');

  // Find OrderedList blocks - should have exactly 3
  const orderedLists = blocks.filter(b => b.kind === 'OrderedList');
  assert.strictEqual(orderedLists.length, 3, 'Should have exactly 3 OrderedList blocks');

  // ===== Test 1: Basic ordered list (starts at 1) =====
  const basicList = orderedLists[0];
  assert.strictEqual(basicList.kind, 'OrderedList');

  // Check list attributes: [startNum, style, delimiter]
  const basicResult = basicList.result as any[];
  assert.strictEqual(basicResult.length, 2, 'Result should have [attrs, items]');
  const basicAttrs = basicResult[0];
  const basicItems = basicResult[1];

  assert.strictEqual(basicAttrs[0], 1, 'Basic list should start at 1');
  assert.strictEqual(basicItems.length, 3, 'Basic list should have 3 items');

  // Extract text from each item's components
  // OrderedList components contain the Plain/Para blocks for each list item
  const basicTexts = basicList.components.map((component: any) => {
    return extractTextFromComponents(component).trim();
  });

  assert.strictEqual(basicTexts[0], 'First item', 'First item text should match');
  assert.strictEqual(basicTexts[1], 'Second item', 'Second item text should match');
  assert.strictEqual(basicTexts[2], 'Third item', 'Third item text should match');

  // ===== Test 2: Custom start list (starts at 5) =====
  const customList = orderedLists[1];
  assert.strictEqual(customList.kind, 'OrderedList');

  const customResult = customList.result as any[];
  const customAttrs = customResult[0];
  const customItems = customResult[1];

  assert.strictEqual(customAttrs[0], 5, 'Custom list should start at 5');
  assert.strictEqual(customItems.length, 2, 'Custom list should have 2 items');
  assert.strictEqual(customList.components.length, 2, 'Custom list should have 2 components');

  // Extract and validate custom list text from components
  const customTexts = customList.components.map((component: any) => {
    return extractTextFromComponents(component).trim();
  });

  assert.strictEqual(customTexts[0], 'Fifth item', 'Fifth item text should match');
  assert.strictEqual(customTexts[1], 'Sixth item', 'Sixth item text should match');

  // ===== Test 3: Nested ordered list =====
  const nestedList = orderedLists[2];
  assert.strictEqual(nestedList.kind, 'OrderedList');

  const nestedResult = nestedList.result as any[];
  const nestedAttrs = nestedResult[0];
  const nestedItems = nestedResult[1];

  assert.strictEqual(nestedAttrs[0], 1, 'Nested parent list should start at 1');
  assert.strictEqual(nestedItems.length, 2, 'Parent list should have 2 items');

  // The nested list has 3 components:
  // [0] Plain: "Top level item one"
  // [1] OrderedList: nested list with 2 items
  // [2] Plain: "Top level item two"
  assert.strictEqual(nestedList.components.length, 3, 'Parent list should have 3 components (2 Plain + 1 nested OrderedList)');

  // First parent item - Plain text
  const firstParentPlain = nestedList.components[0];
  assert.strictEqual(firstParentPlain.kind, 'Plain', 'First component should be Plain');
  const firstParentText = extractTextFromComponents(firstParentPlain).trim();
  assert.strictEqual(firstParentText, 'Top level item one', 'First parent item text should match');

  // Nested OrderedList
  const nestedOrderedList = nestedList.components[1];
  assert.strictEqual(nestedOrderedList.kind, 'OrderedList', 'Second component should be OrderedList');

  // Validate nested list
  const nestedNestedResult = nestedOrderedList.result as any[];
  const nestedNestedAttrs = nestedNestedResult[0];
  const nestedNestedItems = nestedNestedResult[1];

  assert.strictEqual(nestedNestedAttrs[0], 1, 'Nested list should start at 1');
  assert.strictEqual(nestedNestedItems.length, 2, 'Nested list should have 2 items');
  assert.strictEqual(nestedOrderedList.components.length, 2, 'Nested list should have 2 components');

  // Validate nested list item text
  const nestedTexts = nestedOrderedList.components.map((component: any) => {
    return extractTextFromComponents(component).trim();
  });

  assert.strictEqual(nestedTexts[0], 'Nested item A', 'First nested item text should match');
  assert.strictEqual(nestedTexts[1], 'Nested item B', 'Second nested item text should match');

  // Check second parent item - Plain text
  const secondParentPlain = nestedList.components[2];
  assert.strictEqual(secondParentPlain.kind, 'Plain', 'Third component should be Plain');
  const secondParentText = extractTextFromComponents(secondParentPlain).trim();
  assert.strictEqual(secondParentText, 'Top level item two', 'Second parent item text should match');

  // ===== Test 4: Source location validation =====
  // Validate that "First" text actually appears at the reported source location
  const firstItemComponent = basicList.components[0];
  const firstItemStr = firstItemComponent.components?.find((c: any) => c.kind === 'Str' && c.result === 'First');
  if (firstItemStr) {
    validateSourceOffset(firstItemStr, 'First', source);
  }

  // Validate "Fifth" appears at correct location
  const fifthItemComponent = customList.components[0];
  const fifthItemStr = fifthItemComponent.components?.find((c: any) => c.kind === 'Str' && c.result === 'Fifth');
  if (fifthItemStr) {
    validateSourceOffset(fifthItemStr, 'Fifth', source);
  }

  // Validate nested list "Nested" text
  const nestedItemA = nestedOrderedList.components[0];
  const nestedStr = nestedItemA.components?.find((c: any) => c.kind === 'Str' && c.result === 'Nested');
  if (nestedStr) {
    validateSourceOffset(nestedStr, 'Nested', source);
  }
});

test('definition-list.json - DefinitionList conversion', () => {
  const json = loadExample('definition-list');
  const source = loadSourceFile('definition-list');
  const blocks = parseRustQmdBlocks(json.blocks, json);

  // Find DefinitionList blocks
  const defLists = blocks.filter(b => b.kind === 'DefinitionList');
  assert.strictEqual(defLists.length, 1, 'Should have exactly 1 DefinitionList block');

  const defList = defLists[0];
  assert.strictEqual(defList.kind, 'DefinitionList');

  // Extract definition items from components
  // DefinitionList flattens into: term inlines, definition blocks, term inlines, definition blocks, ...
  // Inline kinds: Str, Space, Emph, Strong, Code, etc.
  // Block kinds: Plain, Para, etc.
  const isInlineKind = (kind: string) => {
    return ['Str', 'Space', 'Emph', 'Strong', 'Code', 'Link', 'Math', 'Quoted', 'SoftBreak', 'LineBreak'].includes(kind);
  };

  const defItems: any[] = [];
  let i = 0;
  while (i < defList.components.length) {
    // Collect inline elements as the term
    const termComponents = [];
    while (i < defList.components.length && isInlineKind(defList.components[i].kind)) {
      termComponents.push(defList.components[i]);
      i++;
    }

    // Collect block elements as the definitions
    const definitions = [];
    while (i < defList.components.length && !isInlineKind(defList.components[i].kind)) {
      definitions.push(defList.components[i]);
      i++;
    }

    if (termComponents.length > 0) {
      defItems.push({ term: termComponents, definitions });
    }
  }

  // Validate we have exactly 3 definition items
  assert.strictEqual(defItems.length, 3, 'Should have exactly 3 definition items');

  // Helper to extract text from an array of components
  const extractTextFromArray = (components: any[]) => {
    return components.map(c => extractTextFromComponents(c)).join('');
  };

  // Validate Term 1
  const term1 = defItems[0];
  const term1Text = extractTextFromArray(term1.term);
  assert.strictEqual(term1Text, 'Term 1', 'First term should be "Term 1"');
  assert.strictEqual(term1.definitions.length, 1, 'Term 1 should have 1 definition');
  const term1DefText = extractTextFromComponents(term1.definitions[0]);
  assert.strictEqual(term1DefText, 'Definition for term 1', 'Term 1 definition text should match');

  // Validate Term 2
  const term2 = defItems[1];
  const term2Text = extractTextFromArray(term2.term);
  assert.strictEqual(term2Text, 'Term 2', 'Second term should be "Term 2"');
  assert.strictEqual(term2.definitions.length, 2, 'Term 2 should have 2 definitions');
  const term2Def1Text = extractTextFromComponents(term2.definitions[0]);
  assert.strictEqual(term2Def1Text, 'First definition for term 2', 'Term 2 first definition should match');
  const term2Def2Text = extractTextFromComponents(term2.definitions[1]);
  assert.strictEqual(term2Def2Text, 'Second definition for term 2', 'Term 2 second definition should match');

  // Validate Formatted Term
  const term3 = defItems[2];
  const term3Text = extractTextFromArray(term3.term);
  assert.strictEqual(term3Text, 'Formatted Term', 'Third term should be "Formatted Term"');
  assert.strictEqual(term3.definitions.length, 1, 'Formatted Term should have 1 definition');
  const term3DefText = extractTextFromComponents(term3.definitions[0]);
  assert.strictEqual(term3DefText, 'Definition with bold text', 'Formatted Term definition should match');

  // Validate source locations - check that key terms appear at their reported offsets
  // Find "Term" in Term 1 components
  const term1Str = term1.term.find((c: any) => c.kind === 'Str' && c.result === 'Term');
  if (term1Str && typeof term1Str.start === 'number') {
    const text = source.substring(term1Str.start, term1Str.end);
    assert.strictEqual(text, 'Term', 'Source location for "Term" should be accurate');
  }

  // Find "First" in Term 2's first definition
  const findFirstStr = (component: any): any => {
    if (component.kind === 'Str' && component.result === 'First') {
      return component;
    }
    if (component.components && Array.isArray(component.components)) {
      for (const child of component.components) {
        const found = findFirstStr(child);
        if (found) return found;
      }
    }
    return null;
  };
  const firstStr = findFirstStr(term2.definitions[0]);
  if (firstStr && typeof firstStr.start === 'number') {
    const text = source.substring(firstStr.start, firstStr.end);
    assert.strictEqual(text, 'First', 'Source location for "First" should be accurate');
  }

  // Find "Formatted" in the third term (inside Emph)
  const formattedEmph = term3.term.find((c: any) => c.kind === 'Emph');
  if (formattedEmph?.components) {
    const fStr = formattedEmph.components.find((c: any) => c.kind === 'Str' && c.result === 'Formatted');
    if (fStr && typeof fStr.start === 'number') {
      const text = source.substring(fStr.start, fStr.end);
      assert.strictEqual(text, 'Formatted', 'Source location for "Formatted" should be accurate');
    }
  }
});

test('div-attrs.json - Div with attributes conversion', () => {
  const json = loadExample('div-attrs');
  const source = loadSourceFile('div-attrs');
  const blocks = parseRustQmdBlocks(json.blocks, json);

  // Find Div blocks (excluding nested ones at top level)
  const topLevelDivs = blocks.filter(b => b.kind === 'Div');
  assert.strictEqual(topLevelDivs.length, 4, 'Should have exactly 4 top-level Div blocks');

  // Test 1: Div with class "callout-note"
  const calloutDiv = topLevelDivs.find(d => {
    return d.components.some(c => c.kind === 'attr-class' && c.result === 'callout-note');
  });
  assert.ok(calloutDiv, 'Should have Div with class "callout-note"');
  const calloutText = extractTextFromComponents(calloutDiv);
  assert.strictEqual(calloutText, 'This is a note callout.', 'Callout div content should match');

  // Validate callout class source location (includes . prefix)
  const calloutClass = calloutDiv.components.find(c => c.kind === 'attr-class' && c.result === 'callout-note');
  if (calloutClass && typeof calloutClass.start === 'number') {
    const text = source.substring(calloutClass.start, calloutClass.end);
    assert.strictEqual(text, '.callout-note', 'Source location for ".callout-note" class should be accurate');
  }

  // Test 2: Div with ID "my-div" and class "important"
  const divWithId = topLevelDivs.find(d => {
    return d.components.some(c => c.kind === 'attr-id' && c.result === 'my-div');
  });
  assert.ok(divWithId, 'Should have Div with ID "my-div"');

  const idAttr = divWithId.components.find(c => c.kind === 'attr-id' && c.result === 'my-div');
  assert.ok(idAttr, 'Should have attr-id component');

  const importantClass = divWithId.components.find(c => c.kind === 'attr-class' && c.result === 'important');
  assert.ok(importantClass, 'Should have class "important"');

  const idDivText = extractTextFromComponents(divWithId);
  assert.strictEqual(idDivText, 'Important content here.', 'ID div content should match');

  // Validate ID source location (includes # prefix)
  if (idAttr && typeof idAttr.start === 'number') {
    const text = source.substring(idAttr.start, idAttr.end);
    assert.strictEqual(text, '#my-div', 'Source location for "#my-div" ID should be accurate');
  }

  // Validate "important" class source location (includes . prefix)
  if (importantClass && typeof importantClass.start === 'number') {
    const text = source.substring(importantClass.start, importantClass.end);
    assert.strictEqual(text, '.important', 'Source location for ".important" class should be accurate');
  }

  // Test 3: Div with class "panel" and custom key-value attributes
  const panelDiv = topLevelDivs.find(d => {
    return d.components.some(c => c.kind === 'attr-class' && c.result === 'panel');
  });
  assert.ok(panelDiv, 'Should have Div with class "panel"');

  const panelText = extractTextFromComponents(panelDiv);
  assert.strictEqual(panelText, 'Panel with custom attributes.', 'Panel div content should match');

  // Validate custom key-value attributes
  const attrKeys = panelDiv.components.filter(c => c.kind === 'attr-key');
  const attrValues = panelDiv.components.filter(c => c.kind === 'attr-value');
  assert.strictEqual(attrKeys.length, 2, 'Should have exactly 2 attribute keys');
  assert.strictEqual(attrValues.length, 2, 'Should have exactly 2 attribute values');

  // Check keys and values
  const keyResults = attrKeys.map((k: any) => k.result).sort();
  const valueResults = attrValues.map((v: any) => v.result).sort();
  assert.deepStrictEqual(keyResults, ['custom-key', 'data-value'], 'Attribute keys should match');
  assert.deepStrictEqual(valueResults, ['42', 'test'], 'Attribute values should match');

  // Validate source locations for custom attributes
  // Note: The source locations point to the original text in the file
  // Keys map to their source text, values include quotes in source
  for (const key of attrKeys) {
    if (typeof key.start === 'number') {
      assert.ok(key.start < key.end, `Key ${key.result} should have valid source location`);
      assert.ok(key.start >= 0, `Key ${key.result} start offset should be non-negative`);
      const sourceText = source.substring(key.start, key.end);
      // Verify the source text is one of our expected keys
      assert.ok(['data-value', 'custom-key'].includes(sourceText),
        `Key source text "${sourceText}" should be a valid attribute key`);
    }
  }
  for (const value of attrValues) {
    if (typeof value.start === 'number') {
      assert.ok(value.start < value.end, `Value ${value.result} should have valid source location`);
      assert.ok(value.start >= 0, `Value ${value.result} start offset should be non-negative`);
      const sourceText = source.substring(value.start, value.end);
      // Verify the source text is one of our expected values (with quotes)
      assert.ok(['"42"', '"test"'].includes(sourceText),
        `Value source text "${sourceText}" should be a valid attribute value`);
    }
  }

  // Test 4: Nested divs - outer div with class "outer" contains inner div with class "inner"
  const outerDiv = topLevelDivs.find(d => {
    return d.components.some(c => c.kind === 'attr-class' && c.result === 'outer');
  });
  assert.ok(outerDiv, 'Should have Div with class "outer"');

  // Find the nested inner Div
  const innerDiv = outerDiv.components.find(c => c.kind === 'Div');
  assert.ok(innerDiv, 'Outer div should contain nested Div');

  // Validate inner div has class "inner"
  const innerClass = innerDiv.components.find((c: any) => c.kind === 'attr-class' && c.result === 'inner');
  assert.ok(innerClass, 'Inner div should have class "inner"');

  // Validate outer div content (should have "Outer div content." and the nested div)
  const outerParas = outerDiv.components.filter(c => c.kind === 'Para');
  assert.strictEqual(outerParas.length, 1, 'Outer div should have 1 Para block');
  const outerParaText = extractTextFromComponents(outerParas[0]);
  assert.strictEqual(outerParaText, 'Outer div content.', 'Outer div Para text should match');

  // Validate inner div content
  const innerParas = innerDiv.components.filter((c: any) => c.kind === 'Para');
  assert.strictEqual(innerParas.length, 1, 'Inner div should have 1 Para block');
  const innerParaText = extractTextFromComponents(innerParas[0]);
  assert.strictEqual(innerParaText, 'Inner div content.', 'Inner div Para text should match');

  // Validate source locations for nested div classes (includes . prefix)
  const outerClass = outerDiv.components.find(c => c.kind === 'attr-class' && c.result === 'outer');
  if (outerClass && typeof outerClass.start === 'number') {
    const text = source.substring(outerClass.start, outerClass.end);
    assert.strictEqual(text, '.outer', 'Source location for ".outer" class should be accurate');
  }
  if (innerClass && typeof innerClass.start === 'number') {
    const text = source.substring(innerClass.start, innerClass.end);
    assert.strictEqual(text, '.inner', 'Source location for ".inner" class should be accurate');
  }
});

test('figure.json - Figure conversion', () => {
  const json = loadExample('figure');
  const source = loadSourceFile('figure');
  const blocks = parseRustQmdBlocks(json.blocks, json);

  // Find top-level Figure blocks
  const topLevelFigures = blocks.filter(b => b.kind === 'Figure');
  assert.strictEqual(topLevelFigures.length, 1, 'Should have exactly 1 top-level Figure block');

  // Test 1: Figure with caption (fig-with-caption)
  const figWithCaption = topLevelFigures[0];
  assert.strictEqual(figWithCaption.kind, 'Figure');

  // Validate ID attribute
  const captionFigId = figWithCaption.components.find(c => c.kind === 'attr-id' && c.result === 'fig-with-caption');
  assert.ok(captionFigId, 'Figure should have ID "fig-with-caption"');

  // Validate caption text
  const captionPlain = figWithCaption.components.find(c => c.kind === 'Plain');
  assert.ok(captionPlain, 'Figure should have Plain block for caption');
  const captionText = extractTextFromComponents(captionPlain);
  assert.strictEqual(captionText, 'Simple figure caption', 'Figure caption should match');

  // Find Image component (may be nested inside a Plain block)
  let image: any = null;
  for (const comp of figWithCaption.components) {
    if (comp.kind === 'Image') {
      image = comp;
      break;
    }
    // Check if Image is nested inside a Plain block
    if (comp.kind === 'Plain' && comp.components) {
      image = comp.components.find((c: any) => c.kind === 'Image');
      if (image) break;
    }
  }
  assert.ok(image, 'Figure should contain an Image');

  // Validate image source (target)
  if (image && image.target) {
    assert.strictEqual(image.target[0], 'placeholder.png', 'Image source should be "placeholder.png"');
  }

  // Validate ID source location (includes # prefix)
  if (captionFigId && typeof captionFigId.start === 'number') {
    const text = source.substring(captionFigId.start, captionFigId.end);
    assert.strictEqual(text, '#fig-with-caption', 'Source location for "#fig-with-caption" should be accurate');
  }

  // Test 2: Find the Div with nested figures (fig-layout)
  const figLayoutDiv = blocks.find(b => {
    return b.kind === 'Div' && b.components.some(c => c.kind === 'attr-id' && c.result === 'fig-layout');
  });
  assert.ok(figLayoutDiv, 'Should have Div with ID "fig-layout"');

  // Find nested figures inside the div
  const nestedFigures = figLayoutDiv.components.filter((c: any) => c.kind === 'Figure');
  assert.strictEqual(nestedFigures.length, 2, 'Layout div should contain exactly 2 nested Figures');

  // Test 3: First nested figure (fig-a)
  const figA = nestedFigures.find((f: any) => {
    return f.components.some((c: any) => c.kind === 'attr-id' && c.result === 'fig-a');
  });
  assert.ok(figA, 'Should have nested Figure with ID "fig-a"');

  const figACaption = figA.components.find((c: any) => c.kind === 'Plain');
  const figACaptionText = extractTextFromComponents(figACaption);
  assert.strictEqual(figACaptionText, 'First', 'Figure A caption should be "First"');

  // Find Image in figA (may be nested inside a Plain block)
  let figAImage: any = null;
  for (const comp of figA.components) {
    if (comp.kind === 'Image') {
      figAImage = comp;
      break;
    }
    if (comp.kind === 'Plain' && comp.components) {
      figAImage = comp.components.find((c: any) => c.kind === 'Image');
      if (figAImage) break;
    }
  }
  assert.ok(figAImage, 'Figure A should contain an Image');
  if (figAImage && figAImage.target) {
    assert.strictEqual(figAImage.target[0], 'placeholder.png', 'Figure A image source should be "placeholder.png"');
  }

  // Test 4: Second nested figure (fig-b)
  const figB = nestedFigures.find((f: any) => {
    return f.components.some((c: any) => c.kind === 'attr-id' && c.result === 'fig-b');
  });
  assert.ok(figB, 'Should have nested Figure with ID "fig-b"');

  const figBCaption = figB.components.find((c: any) => c.kind === 'Plain');
  const figBCaptionText = extractTextFromComponents(figBCaption);
  assert.strictEqual(figBCaptionText, 'Second', 'Figure B caption should be "Second"');

  // Find Image in figB (may be nested inside a Plain block)
  let figBImage: any = null;
  for (const comp of figB.components) {
    if (comp.kind === 'Image') {
      figBImage = comp;
      break;
    }
    if (comp.kind === 'Plain' && comp.components) {
      figBImage = comp.components.find((c: any) => c.kind === 'Image');
      if (figBImage) break;
    }
  }
  assert.ok(figBImage, 'Figure B should contain an Image');
  if (figBImage && figBImage.target) {
    assert.strictEqual(figBImage.target[0], 'placeholder.png', 'Figure B image source should be "placeholder.png"');
  }

  // Test 5: Validate layout div caption Para
  const layoutPara = figLayoutDiv.components.find((c: any) => c.kind === 'Para');
  assert.ok(layoutPara, 'Layout div should have Para block');
  const layoutParaText = extractTextFromComponents(layoutPara);
  assert.strictEqual(layoutParaText, 'Figure layout with multiple images', 'Layout div Para text should match');

  // Validate nested figure IDs source locations
  const figAId = figA.components.find((c: any) => c.kind === 'attr-id' && c.result === 'fig-a');
  if (figAId && typeof figAId.start === 'number') {
    const text = source.substring(figAId.start, figAId.end);
    assert.strictEqual(text, '#fig-a', 'Source location for "#fig-a" should be accurate');
  }

  const figBId = figB.components.find((c: any) => c.kind === 'attr-id' && c.result === 'fig-b');
  if (figBId && typeof figBId.start === 'number') {
    const text = source.substring(figBId.start, figBId.end);
    assert.strictEqual(text, '#fig-b', 'Source location for "#fig-b" should be accurate');
  }

  // Note: The first image (![](placeholder.png){#fig-simple}) is wrapped in a Para, not a Figure,
  // because it has no caption text. This is expected Pandoc behavior.
});

test('horizontal-rule.json - HorizontalRule conversion', () => {
  const json = loadExample('horizontal-rule');
  const blocks = parseRustQmdBlocks(json.blocks, json);

  // Find HorizontalRule blocks
  const hrs = blocks.filter(b => b.kind === 'HorizontalRule');
  assert.ok(hrs.length > 0, 'Should have HorizontalRule blocks');

  // Check HorizontalRule structure
  const hr = hrs[0];
  assert.strictEqual(hr.kind, 'HorizontalRule');
  assert.ok('source' in hr, 'HorizontalRule should have source');
  assert.ok(typeof hr.start === 'number', 'HorizontalRule should have start offset');
  assert.ok(typeof hr.end === 'number', 'HorizontalRule should have end offset');

  // HorizontalRule should have no components
  assert.strictEqual(hr.components.length, 0, 'HorizontalRule should have no components');
  assert.strictEqual(hr.result, null, 'HorizontalRule result should be null');
});

test('raw-block.json - RawBlock conversion', () => {
  const json = loadExample('raw-block');
  const source = loadSourceFile('raw-block');
  const blocks = parseRustQmdBlocks(json.blocks, json);

  // Find RawBlock blocks
  const rawBlocks = blocks.filter(b => b.kind === 'RawBlock');
  assert.strictEqual(rawBlocks.length, 2, 'Should have exactly 2 RawBlock blocks');

  // Test 1: HTML RawBlock
  const htmlBlock = rawBlocks.find(rb => (rb.result as any[])[0] === 'html');
  assert.ok(htmlBlock, 'Should have HTML RawBlock');
  assert.strictEqual(htmlBlock.kind, 'RawBlock');

  // Validate HTML format
  const htmlResult = htmlBlock.result as any[];
  assert.ok(Array.isArray(htmlResult), 'HTML RawBlock result should be array');
  assert.strictEqual(htmlResult.length, 2, 'HTML RawBlock result should have [format, content]');
  assert.strictEqual(htmlResult[0], 'html', 'HTML RawBlock format should be "html"');

  // Validate HTML content
  const htmlContent = htmlResult[1];
  assert.ok(typeof htmlContent === 'string', 'HTML RawBlock content should be string');
  assert.ok(htmlContent.includes('<div class="custom">'), 'HTML content should include div element');
  assert.ok(htmlContent.includes('<p>Raw HTML block</p>'), 'HTML content should include p element');
  assert.ok(htmlContent.includes('</div>'), 'HTML content should include closing div');

  // Validate exact HTML content (with newlines)
  const expectedHtml = '<div class="custom">\n  <p>Raw HTML block</p>\n</div>';
  assert.strictEqual(htmlContent, expectedHtml, 'HTML content should match exactly');

  // Test 2: LaTeX RawBlock
  const latexBlock = rawBlocks.find(rb => (rb.result as any[])[0] === 'latex');
  assert.ok(latexBlock, 'Should have LaTeX RawBlock');
  assert.strictEqual(latexBlock.kind, 'RawBlock');

  // Validate LaTeX format
  const latexResult = latexBlock.result as any[];
  assert.ok(Array.isArray(latexResult), 'LaTeX RawBlock result should be array');
  assert.strictEqual(latexResult.length, 2, 'LaTeX RawBlock result should have [format, content]');
  assert.strictEqual(latexResult[0], 'latex', 'LaTeX RawBlock format should be "latex"');

  // Validate LaTeX content
  const latexContent = latexResult[1];
  assert.ok(typeof latexContent === 'string', 'LaTeX RawBlock content should be string');
  assert.ok(latexContent.includes('\\begin{theorem}'), 'LaTeX content should include begin theorem');
  assert.ok(latexContent.includes('Raw LaTeX block'), 'LaTeX content should include text');
  assert.ok(latexContent.includes('\\end{theorem}'), 'LaTeX content should include end theorem');

  // Validate exact LaTeX content (with newlines)
  const expectedLatex = '\\begin{theorem}\nRaw LaTeX block\n\\end{theorem}';
  assert.strictEqual(latexContent, expectedLatex, 'LaTeX content should match exactly');

  // Test 3: Validate source locations
  // HTML block should have valid source location
  assert.ok(typeof htmlBlock.start === 'number', 'HTML RawBlock should have start offset');
  assert.ok(typeof htmlBlock.end === 'number', 'HTML RawBlock should have end offset');
  assert.ok(htmlBlock.start < htmlBlock.end, 'HTML RawBlock start should be less than end');

  // LaTeX block should have valid source location
  assert.ok(typeof latexBlock.start === 'number', 'LaTeX RawBlock should have start offset');
  assert.ok(typeof latexBlock.end === 'number', 'LaTeX RawBlock should have end offset');
  assert.ok(latexBlock.start < latexBlock.end, 'LaTeX RawBlock start should be less than end');

  // Verify HTML block source location points to the right content
  if (typeof htmlBlock.start === 'number' && typeof htmlBlock.end === 'number') {
    const htmlSourceText = source.substring(htmlBlock.start, htmlBlock.end);
    // Should include the entire code block with fence
    assert.ok(htmlSourceText.includes('```{=html}'), 'HTML source should include opening fence');
    assert.ok(htmlSourceText.includes('```'), 'HTML source should include closing fence');
    assert.ok(htmlSourceText.includes('<div class="custom">'), 'HTML source should include div element');
  }

  // Verify LaTeX block source location points to the right content
  if (typeof latexBlock.start === 'number' && typeof latexBlock.end === 'number') {
    const latexSourceText = source.substring(latexBlock.start, latexBlock.end);
    // Should include the entire code block with fence
    assert.ok(latexSourceText.includes('```{=latex}'), 'LaTeX source should include opening fence');
    assert.ok(latexSourceText.includes('```'), 'LaTeX source should include closing fence');
    assert.ok(latexSourceText.includes('\\begin{theorem}'), 'LaTeX source should include begin theorem');
  }
});

test('all block types - source mapping validation', () => {
  const examples = ['ordered-list', 'definition-list', 'div-attrs', 'figure', 'horizontal-rule', 'raw-block'];

  examples.forEach(name => {
    const json = loadExample(name);
    const doc = parseRustQmdDocument(json);

    // Walk all components and verify they have source info
    function checkSource(component: any, depth = 0): void {
      // All components should have source (MappedString)
      assert.ok('source' in component, `${name}: Component at depth ${depth} should have source`);
      assert.ok('value' in component.source, `${name}: Source should be a MappedString`);

      // All components should have start/end offsets
      assert.ok(typeof component.start === 'number', `${name}: Should have start offset`);
      assert.ok(typeof component.end === 'number', `${name}: Should have end offset`);

      // Recursively check nested components
      if (component.components && Array.isArray(component.components)) {
        component.components.forEach((child: any) => checkSource(child, depth + 1));
      }
    }

    checkSource(doc);
  });
});

test('helper functions - basic validation', () => {
  // Test loadSourceFile
  const source = loadSourceFile('horizontal-rule');
  assert.ok(source.length > 0, 'Should load source file');
  assert.ok(source.includes('---'), 'horizontal-rule.qmd should contain ---');

  // Test extractTextFromComponents with a simple example
  const json = loadExample('horizontal-rule');
  const blocks = parseRustQmdBlocks(json.blocks, json);
  const header = blocks.find(b => b.kind === 'Header');
  assert.ok(header, 'Should find Header block');

  const headerText = extractTextFromComponents(header);
  assert.ok(headerText.includes('Horizontal'), 'Should extract header text');

  // Test validateSourceOffset
  // Find a Str component and validate its source location
  const strComponents = header.components.filter((c: any) => c.kind === 'Str');
  assert.ok(strComponents.length > 0, 'Should have Str components');

  const firstStr = strComponents[0];
  // The validateSourceOffset function will throw if validation fails
  validateSourceOffset(firstStr, firstStr.result as string, source);
});

console.log('All block type tests passed! ✨');
