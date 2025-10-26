/**
 * Test suite for inline type fixtures
 *
 * Tests conversion of all inline types added in k-198:
 * - Image
 * - Span with attributes
 * - Cite (citations)
 * - Note (footnotes)
 * - Quoted (single/double)
 * - Strikeout
 * - Superscript
 * - Subscript
 * - SmallCaps
 * - Underline
 * - RawInline
 * - SoftBreak
 * - LineBreak
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
 */
function extractTextFromComponents(component: any): string {
  let text = '';

  if (component.kind === 'Str' && typeof component.result === 'string') {
    return component.result;
  }

  if (component.kind === 'Space') {
    return ' ';
  }

  if (component.kind === 'SoftBreak' || component.kind === 'LineBreak') {
    return '\n';
  }

  if (component.components && Array.isArray(component.components)) {
    for (const child of component.components) {
      text += extractTextFromComponents(child);
    }
  }

  return text;
}

/**
 * Recursively find an inline element by kind in a component tree
 */
function findInlineByKind(component: any, kind: string): any {
  if (component.kind === kind) {
    return component;
  }

  if (component.components && Array.isArray(component.components)) {
    for (const child of component.components) {
      const found = findInlineByKind(child, kind);
      if (found) return found;
    }
  }

  return null;
}

/**
 * Recursively find all inline elements by kind
 */
function findAllInlinesByKind(component: any, kind: string): any[] {
  const results: any[] = [];

  if (component.kind === kind) {
    results.push(component);
  }

  if (component.components && Array.isArray(component.components)) {
    for (const child of component.components) {
      results.push(...findAllInlinesByKind(child, kind));
    }
  }

  return results;
}

test('inline-types.json - Image inline conversion', () => {
  const json = loadExample('inline-types');
  const source = loadSourceFile('inline-types');
  const blocks = parseRustQmdBlocks(json.blocks, json);

  // Find Para blocks containing Image
  const paraWithImage = blocks.find(b => {
    return b.kind === 'Para' && b.components &&
           b.components.some((c: any) => c.kind === 'Image');
  });
  assert.ok(paraWithImage, 'Should have Para with Image');

  // Find the Image element
  const image = findInlineByKind(paraWithImage, 'Image');
  assert.ok(image, 'Should find Image inline element');
  assert.strictEqual(image.kind, 'Image');

  // Validate Image has target (URL) in result
  // Image result is [Attr, Inlines, Target] where Target is [url, title]
  assert.ok(image.result, 'Image should have result');
  assert.ok(Array.isArray(image.result), 'Image result should be array');
  const target = (image.result as any[])[2];
  assert.ok(target, 'Image should have target in result[2]');
  assert.ok(Array.isArray(target), 'Image target should be array');
  assert.strictEqual(target[0], 'placeholder.png', 'Image target URL should be placeholder.png');

  // Validate Image alt text (in components)
  const altText = extractTextFromComponents(image);
  assert.strictEqual(altText, 'alt text', 'Image alt text should match');

  // Validate source location
  assert.ok(typeof image.start === 'number', 'Image should have start offset');
  assert.ok(typeof image.end === 'number', 'Image should have end offset');
});

test('inline-types.json - Span with attributes conversion', () => {
  const json = loadExample('inline-types');
  const source = loadSourceFile('inline-types');
  const blocks = parseRustQmdBlocks(json.blocks, json);

  // Find all Span elements
  const allSpans: any[] = [];
  for (const block of blocks) {
    allSpans.push(...findAllInlinesByKind(block, 'Span'));
  }
  assert.ok(allSpans.length >= 3, 'Should have at least 3 Span elements');

  // Test 1: Span with class
  const spanWithClass = allSpans.find(s => {
    return s.components && s.components.some((c: any) =>
      c.kind === 'attr-class' && c.result === 'highlight'
    );
  });
  assert.ok(spanWithClass, 'Should have Span with class "highlight"');
  const classSpanText = extractTextFromComponents(spanWithClass.components.find((c: any) =>
    c.kind === 'Str' || c.kind === 'Space'
  ));

  // Test 2: Span with ID
  const spanWithId = allSpans.find(s => {
    return s.components && s.components.some((c: any) =>
      c.kind === 'attr-id' && c.result === 'my-span'
    );
  });
  assert.ok(spanWithId, 'Should have Span with ID "my-span"');

  // Test 3: Span with custom attributes
  const spanWithCustom = allSpans.find(s => {
    return s.components && s.components.some((c: any) =>
      c.kind === 'attr-key' && c.result === 'data-value'
    );
  });
  assert.ok(spanWithCustom, 'Should have Span with custom attribute "data-value"');
});

test('inline-types.json - Cite (citations) conversion', () => {
  const json = loadExample('inline-types');
  const blocks = parseRustQmdBlocks(json.blocks, json);

  // Find Para blocks containing Cite
  const paraWithCite = blocks.find(b => {
    return b.kind === 'Para' && b.components &&
           b.components.some((c: any) => c.kind === 'Cite');
  });
  assert.ok(paraWithCite, 'Should have Para with Cite');

  // Find all Cite elements
  const allCites = findAllInlinesByKind(paraWithCite, 'Cite');
  assert.ok(allCites.length >= 1, 'Should have at least 1 Cite element');

  // Validate Cite structure
  const cite = allCites[0];
  assert.strictEqual(cite.kind, 'Cite');
  assert.ok(cite.components, 'Cite should have components');

  // Cite should have citation data in result
  assert.ok(cite.result, 'Cite should have result');
});

test('inline-types.json - Note (footnote) conversion', () => {
  const json = loadExample('inline-types');
  const blocks = parseRustQmdBlocks(json.blocks, json);

  // Find Para blocks containing Note
  const allNotes: any[] = [];
  for (const block of blocks) {
    allNotes.push(...findAllInlinesByKind(block, 'Note'));
  }
  assert.ok(allNotes.length >= 1, 'Should have at least 1 Note element');

  // Validate Note structure
  const note = allNotes[0];
  assert.strictEqual(note.kind, 'Note');

  // Note result contains block-level content (Para, etc.)
  assert.ok(note.result, 'Note should have result');
  assert.ok(Array.isArray(note.result), 'Note result should be array of blocks');
  assert.ok((note.result as any[]).length > 0, 'Note should have block content');

  // The components array should contain converted blocks
  assert.ok(Array.isArray(note.components), 'Note should have components array');
  assert.ok(note.components.length > 0, 'Note components should contain converted blocks');

  // Verify the first component is a converted Para block
  const firstBlock = note.components[0];
  assert.strictEqual(firstBlock.kind, 'Para', 'First component should be Para block');
  assert.ok(firstBlock.components, 'Para should have inline components');

  // Extract text from the footnote content
  const footnoteText = extractTextFromComponents(firstBlock);
  assert.strictEqual(footnoteText, 'This is the footnote text.', 'Footnote text should match');
});

test('inline-types.json - Quoted (single/double) conversion', () => {
  const json = loadExample('inline-types');
  const blocks = parseRustQmdBlocks(json.blocks, json);

  // Find all Quoted elements
  const allQuoted: any[] = [];
  for (const block of blocks) {
    allQuoted.push(...findAllInlinesByKind(block, 'Quoted'));
  }
  assert.ok(allQuoted.length >= 2, 'Should have at least 2 Quoted elements');

  // Test SingleQuote
  // Quoted result is [QuoteType, Inlines] where QuoteType is {t: "SingleQuote" | "DoubleQuote"}
  const singleQuoted = allQuoted.find(q => {
    return q.result && Array.isArray(q.result) && q.result[0]?.t === 'SingleQuote';
  });
  assert.ok(singleQuoted, 'Should have Quoted with SingleQuote');
  const singleText = extractTextFromComponents(singleQuoted);
  assert.strictEqual(singleText, 'Hello', 'Single quoted text should be "Hello"');

  // Test DoubleQuote
  const doubleQuoted = allQuoted.find(q => {
    return q.result && Array.isArray(q.result) && q.result[0]?.t === 'DoubleQuote';
  });
  assert.ok(doubleQuoted, 'Should have Quoted with DoubleQuote');
  const doubleText = extractTextFromComponents(doubleQuoted);
  assert.strictEqual(doubleText, 'World', 'Double quoted text should be "World"');
});

test('inline-types.json - Strikeout conversion', () => {
  const json = loadExample('inline-types');
  const blocks = parseRustQmdBlocks(json.blocks, json);

  // Find Strikeout element
  const strikeout = blocks.reduce((found: any, block) => {
    return found || findInlineByKind(block, 'Strikeout');
  }, null);
  assert.ok(strikeout, 'Should have Strikeout element');
  assert.strictEqual(strikeout.kind, 'Strikeout');

  // Validate strikeout text
  const text = extractTextFromComponents(strikeout);
  assert.strictEqual(text, 'strikethrough', 'Strikeout text should be "strikethrough"');
});

test('inline-types.json - Superscript conversion', () => {
  const json = loadExample('inline-types');
  const blocks = parseRustQmdBlocks(json.blocks, json);

  // Find Superscript element
  const superscript = blocks.reduce((found: any, block) => {
    return found || findInlineByKind(block, 'Superscript');
  }, null);
  assert.ok(superscript, 'Should have Superscript element');
  assert.strictEqual(superscript.kind, 'Superscript');

  // Validate superscript text
  const text = extractTextFromComponents(superscript);
  assert.strictEqual(text, '2', 'Superscript text should be "2"');
});

test('inline-types.json - Subscript conversion', () => {
  const json = loadExample('inline-types');
  const blocks = parseRustQmdBlocks(json.blocks, json);

  // Find Subscript element
  const subscript = blocks.reduce((found: any, block) => {
    return found || findInlineByKind(block, 'Subscript');
  }, null);
  assert.ok(subscript, 'Should have Subscript element');
  assert.strictEqual(subscript.kind, 'Subscript');

  // Validate subscript text
  const text = extractTextFromComponents(subscript);
  assert.strictEqual(text, '2', 'Subscript text should be "2"');
});

test('inline-types.json - SmallCaps conversion', () => {
  const json = loadExample('inline-types');
  const blocks = parseRustQmdBlocks(json.blocks, json);

  // Find SmallCaps element
  const smallcaps = blocks.reduce((found: any, block) => {
    return found || findInlineByKind(block, 'SmallCaps');
  }, null);
  assert.ok(smallcaps, 'Should have SmallCaps element');
  assert.strictEqual(smallcaps.kind, 'SmallCaps');

  // Validate smallcaps text
  const text = extractTextFromComponents(smallcaps);
  assert.strictEqual(text, 'small caps', 'SmallCaps text should be "small caps"');
});

test('inline-types.json - Underline conversion', () => {
  const json = loadExample('inline-types');
  const blocks = parseRustQmdBlocks(json.blocks, json);

  // Find Underline element
  const underline = blocks.reduce((found: any, block) => {
    return found || findInlineByKind(block, 'Underline');
  }, null);
  assert.ok(underline, 'Should have Underline element');
  assert.strictEqual(underline.kind, 'Underline');

  // Validate underline text
  const text = extractTextFromComponents(underline);
  assert.strictEqual(text, 'underlined', 'Underline text should be "underlined"');
});

test('inline-types.json - RawInline conversion', () => {
  const json = loadExample('inline-types');
  const blocks = parseRustQmdBlocks(json.blocks, json);

  // Find all RawInline elements
  const allRawInline: any[] = [];
  for (const block of blocks) {
    allRawInline.push(...findAllInlinesByKind(block, 'RawInline'));
  }
  assert.ok(allRawInline.length >= 2, 'Should have at least 2 RawInline elements');

  // Test HTML RawInline
  const htmlRaw = allRawInline.find(r => r.result && r.result[0] === 'html');
  assert.ok(htmlRaw, 'Should have HTML RawInline');
  assert.strictEqual(htmlRaw.result[0], 'html', 'RawInline format should be "html"');
  assert.ok(htmlRaw.result[1].includes('<span class="raw">'), 'HTML RawInline should contain span element');

  // Test LaTeX RawInline
  const latexRaw = allRawInline.find(r => r.result && r.result[0] === 'latex');
  assert.ok(latexRaw, 'Should have LaTeX RawInline');
  assert.strictEqual(latexRaw.result[0], 'latex', 'RawInline format should be "latex"');
  assert.ok(latexRaw.result[1].includes('\\textrm'), 'LaTeX RawInline should contain textrm command');
});

test('inline-types.json - SoftBreak conversion', () => {
  const json = loadExample('inline-types');
  const blocks = parseRustQmdBlocks(json.blocks, json);

  // Find SoftBreak element
  const softbreak = blocks.reduce((found: any, block) => {
    return found || findInlineByKind(block, 'SoftBreak');
  }, null);
  assert.ok(softbreak, 'Should have SoftBreak element');
  assert.strictEqual(softbreak.kind, 'SoftBreak');
  assert.strictEqual(softbreak.result, null, 'SoftBreak result should be null');
});

test('inline-types.json - LineBreak conversion', () => {
  const json = loadExample('inline-types');
  const blocks = parseRustQmdBlocks(json.blocks, json);

  // Find all LineBreak elements
  const allLineBreaks: any[] = [];
  for (const block of blocks) {
    allLineBreaks.push(...findAllInlinesByKind(block, 'LineBreak'));
  }
  assert.ok(allLineBreaks.length >= 1, 'Should have at least 1 LineBreak element');

  // Validate LineBreak structure
  const linebreak = allLineBreaks[0];
  assert.strictEqual(linebreak.kind, 'LineBreak');
  assert.strictEqual(linebreak.result, null, 'LineBreak result should be null');
});

console.log('\nAll inline type tests passed! ✨\n');
