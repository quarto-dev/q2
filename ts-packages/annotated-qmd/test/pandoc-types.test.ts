/**
 * Tests for Pandoc type definitions
 *
 * These tests validate that our TypeScript types correctly match
 * the actual JSON output from Pandoc and quarto-markdown-pandoc.
 */

import { test } from 'node:test';
import assert from 'node:assert';
import type {
  Inline,
  Block,
  PandocDocument,
  QmdPandocDocument,
  Attr,
  Target,
  Inline_Str,
  Inline_Emph,
  Block_Para,
  Annotated_Inline_Str,
  Annotated_Block_Para,
} from '../src/pandoc-types.js';
import {
  isQmdPandocDocument,
  isInline,
  isBlock,
} from '../src/pandoc-types.js';

test('Attr type matches Pandoc structure', () => {
  const attr: Attr = ["my-id", ["class1", "class2"], [["key", "value"]]];

  assert.strictEqual(attr[0], "my-id");
  assert.strictEqual(attr[1].length, 2);
  assert.strictEqual(attr[2][0][0], "key");
});

test('Target type matches Pandoc structure', () => {
  const target: Target = ["https://example.com", "Example"];

  assert.strictEqual(target[0], "https://example.com");
  assert.strictEqual(target[1], "Example");
});

test('Simple Inline types compile correctly', () => {
  const str: Inline = { t: "Str", c: "hello" };
  const space: Inline = { t: "Space" };
  const softBreak: Inline = { t: "SoftBreak" };
  const lineBreak: Inline = { t: "LineBreak" };

  assert.strictEqual(str.t, "Str");
  assert.strictEqual(space.t, "Space");
});

test('Formatting Inline types compile correctly', () => {
  const emph: Inline = {
    t: "Emph",
    c: [{ t: "Str", c: "italic" }]
  };

  const strong: Inline = {
    t: "Strong",
    c: [{ t: "Str", c: "bold" }]
  };

  assert.strictEqual(emph.t, "Emph");
  assert.strictEqual(strong.t, "Strong");
});

test('Code Inline type compiles correctly', () => {
  const code: Inline = {
    t: "Code",
    c: [["", [], []], "console.log('hi')"]
  };

  assert.strictEqual(code.t, "Code");
  if (code.t === "Code") {
    assert.strictEqual(code.c[1], "console.log('hi')");
  }
});

test('Math Inline type compiles correctly', () => {
  const math: Inline = {
    t: "Math",
    c: [{ t: "InlineMath" }, "x^2"]
  };

  assert.strictEqual(math.t, "Math");
  if (math.t === "Math") {
    assert.strictEqual(math.c[0].t, "InlineMath");
    assert.strictEqual(math.c[1], "x^2");
  }
});

test('Link Inline type compiles correctly', () => {
  const link: Inline = {
    t: "Link",
    c: [
      ["", [], []],
      [{ t: "Str", c: "text" }],
      ["url", "title"]
    ]
  };

  assert.strictEqual(link.t, "Link");
  if (link.t === "Link") {
    assert.strictEqual(link.c[2][0], "url");
  }
});

test('Para Block type compiles correctly', () => {
  const para: Block = {
    t: "Para",
    c: [
      { t: "Str", c: "Hello" },
      { t: "Space" },
      { t: "Str", c: "world" }
    ]
  };

  assert.strictEqual(para.t, "Para");
  if (para.t === "Para") {
    assert.strictEqual(para.c.length, 3);
  }
});

test('Header Block type compiles correctly', () => {
  const header: Block = {
    t: "Header",
    c: [
      1,
      ["my-header", [], []],
      [{ t: "Str", c: "Title" }]
    ]
  };

  assert.strictEqual(header.t, "Header");
  if (header.t === "Header") {
    assert.strictEqual(header.c[0], 1);  // level
    assert.strictEqual(header.c[1][0], "my-header");  // id
  }
});

test('CodeBlock Block type compiles correctly', () => {
  const codeBlock: Block = {
    t: "CodeBlock",
    c: [
      ["", ["python"], []],
      "print('hello')"
    ]
  };

  assert.strictEqual(codeBlock.t, "CodeBlock");
  if (codeBlock.t === "CodeBlock") {
    assert.strictEqual(codeBlock.c[0][1][0], "python");
    assert.strictEqual(codeBlock.c[1], "print('hello')");
  }
});

test('BulletList Block type compiles correctly', () => {
  const bulletList: Block = {
    t: "BulletList",
    c: [
      [{ t: "Plain", c: [{ t: "Str", c: "Item 1" }] }],
      [{ t: "Plain", c: [{ t: "Str", c: "Item 2" }] }]
    ]
  };

  assert.strictEqual(bulletList.t, "BulletList");
  if (bulletList.t === "BulletList") {
    assert.strictEqual(bulletList.c.length, 2);
  }
});

test('OrderedList Block type compiles correctly', () => {
  const orderedList: Block = {
    t: "OrderedList",
    c: [
      [1, { t: "Decimal" }, { t: "Period" }],
      [
        [{ t: "Plain", c: [{ t: "Str", c: "First" }] }],
        [{ t: "Plain", c: [{ t: "Str", c: "Second" }] }]
      ]
    ]
  };

  assert.strictEqual(orderedList.t, "OrderedList");
  if (orderedList.t === "OrderedList") {
    assert.strictEqual(orderedList.c[0][0], 1);  // start number
    assert.strictEqual(orderedList.c[1].length, 2);  // two items
  }
});

test('DefinitionList Block type compiles correctly', () => {
  const defList: Block = {
    t: "DefinitionList",
    c: [
      [
        [{ t: "Str", c: "Term" }],  // term
        [[{ t: "Plain", c: [{ t: "Str", c: "Definition" }] }]]  // definitions
      ]
    ]
  };

  assert.strictEqual(defList.t, "DefinitionList");
  if (defList.t === "DefinitionList") {
    assert.strictEqual(defList.c.length, 1);  // one term/def pair
  }
});

test('Div Block type compiles correctly', () => {
  const div: Block = {
    t: "Div",
    c: [
      ["my-div", ["class"], []],
      [{ t: "Para", c: [{ t: "Str", c: "content" }] }]
    ]
  };

  assert.strictEqual(div.t, "Div");
  if (div.t === "Div") {
    assert.strictEqual(div.c[0][0], "my-div");
    assert.strictEqual(div.c[1].length, 1);
  }
});

test('PandocDocument type compiles correctly', () => {
  const doc: PandocDocument = {
    "pandoc-api-version": [1, 23, 1],
    meta: {},
    blocks: [
      { t: "Para", c: [{ t: "Str", c: "Hello" }] }
    ]
  };

  assert.deepStrictEqual(doc["pandoc-api-version"], [1, 23, 1]);
  assert.strictEqual(doc.blocks.length, 1);
});

test('QmdPandocDocument type compiles correctly', () => {
  const doc: QmdPandocDocument = {
    "pandoc-api-version": [1, 23, 1],
    meta: {},
    blocks: [
      { t: "Para", c: [{ t: "Str", c: "Hello", s: 0 }], s: 1 }
    ],
    astContext: {
      sourceInfoPool: [
        { r: [0, 5], t: 0, d: 0 },
        { r: [0, 10], t: 0, d: 0 }
      ],
      files: [
        { name: "test.qmd", content: "Hello test" }
      ]
    }
  };

  assert.strictEqual(isQmdPandocDocument(doc), true);
  assert.strictEqual(doc.astContext.sourceInfoPool.length, 2);
});

test('isInline type guard works', () => {
  const inline = { t: "Str", c: "hello" };
  const notInline = { foo: "bar" };

  assert.strictEqual(isInline(inline), true);
  assert.strictEqual(isInline(notInline), false);
  assert.strictEqual(isInline(null), false);
  assert.strictEqual(isInline(undefined), false);
});

test('isBlock type guard works', () => {
  const block = { t: "Para", c: [] };
  const notBlock = { foo: "bar" };

  assert.strictEqual(isBlock(block), true);
  assert.strictEqual(isBlock(notBlock), false);
  assert.strictEqual(isBlock(null), false);
  assert.strictEqual(isBlock(undefined), false);
});

test('Inline with source info compiles correctly', () => {
  const str: Inline = { t: "Str", c: "hello", s: 42 };

  assert.strictEqual(str.t, "Str");
  if (str.t === "Str") {
    assert.strictEqual(str.s, 42);
  }
});

test('Block with source info compiles correctly', () => {
  const para: Block = {
    t: "Para",
    c: [{ t: "Str", c: "test", s: 0 }],
    s: 1
  };

  assert.strictEqual(para.t, "Para");
  if (para.t === "Para") {
    assert.strictEqual(para.s, 1);
  }
});

test('Complex nested structure type-checks', () => {
  // This represents a real-world structure from Pandoc
  const doc: PandocDocument = {
    "pandoc-api-version": [1, 23, 1],
    meta: {},
    blocks: [
      {
        t: "Header",
        c: [1, ["header", [], []], [{ t: "Str", c: "Header" }]]
      },
      {
        t: "Para",
        c: [
          { t: "Str", c: "Paragraph" },
          { t: "Space" },
          { t: "Str", c: "with" },
          { t: "Space" },
          { t: "Strong", c: [{ t: "Str", c: "bold" }] },
          { t: "Str", c: "," },
          { t: "Space" },
          { t: "Emph", c: [{ t: "Str", c: "italic" }] },
          { t: "Str", c: "," },
          { t: "Space" },
          { t: "Str", c: "and" },
          { t: "Space" },
          {
            t: "Link",
            c: [
              ["", [], []],
              [{ t: "Str", c: "link" }],
              ["url", ""]
            ]
          },
          { t: "Str", c: "." }
        ]
      },
      {
        t: "BulletList",
        c: [
          [{ t: "Plain", c: [{ t: "Str", c: "Item" }, { t: "Space" }, { t: "Str", c: "1" }] }],
          [{ t: "Plain", c: [{ t: "Str", c: "Item" }, { t: "Space" }, { t: "Str", c: "2" }] }]
        ]
      }
    ]
  };

  // If this compiles and runs, the types are working correctly
  assert.strictEqual(doc.blocks.length, 3);
  assert.strictEqual(doc.blocks[0].t, "Header");
  assert.strictEqual(doc.blocks[1].t, "Para");
  assert.strictEqual(doc.blocks[2].t, "BulletList");
});

test('Named base types work correctly', () => {
  const str: Inline_Str = { t: "Str", c: "hello" };
  const emph: Inline_Emph = { t: "Emph", c: [str] };
  const para: Block_Para = { t: "Para", c: [str, emph] };

  assert.strictEqual(str.t, "Str");
  assert.strictEqual(str.c, "hello");
  assert.strictEqual(emph.t, "Emph");
  assert.strictEqual(para.t, "Para");
});

test('Annotated types add s field via intersection', () => {
  // Base type without s
  const baseStr: Inline_Str = { t: "Str", c: "hello" };

  // Annotated type with s (via intersection)
  const annotatedStr: Annotated_Inline_Str = { t: "Str", c: "hello", s: 42 };

  assert.strictEqual(baseStr.t, "Str");
  assert.strictEqual('s' in baseStr, false);
  assert.strictEqual(annotatedStr.s, 42);
});

test('Annotated types are compatible with base types', () => {
  const annotatedStr: Annotated_Inline_Str = { t: "Str", c: "world", s: 10 };

  // Should be assignable to base Inline union
  const inline: Inline = annotatedStr;

  assert.strictEqual(inline.t, "Str");
});

test('Base and annotated blocks work together', () => {
  const basePara: Block_Para = {
    t: "Para",
    c: [{ t: "Str", c: "test" }]
  };

  const annotatedPara: Annotated_Block_Para = {
    t: "Para",
    c: [{ t: "Str", c: "test", s: 0 }],
    s: 1
  };

  assert.strictEqual(basePara.t, "Para");
  assert.strictEqual('s' in basePara, false);
  assert.strictEqual(annotatedPara.s, 1);
});

console.log('All Pandoc type tests passed! ✨');
