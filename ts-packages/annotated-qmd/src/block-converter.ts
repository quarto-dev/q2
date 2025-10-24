/**
 * Block Conversion
 *
 * Converts Block AST nodes from quarto-markdown-pandoc JSON
 * into AnnotatedParse structures compatible with quarto-cli.
 */

import type { AnnotatedParse } from './types.js';
import type { SourceInfoReconstructor } from './source-map.js';
import type { Annotated_Block, Annotated_Caption } from './pandoc-types.js';
import { InlineConverter } from './inline-converter.js';

/**
 * Converts Block AST nodes from quarto-markdown-pandoc to AnnotatedParse
 */
export class BlockConverter {
  private inlineConverter: InlineConverter;

  constructor(
    private sourceReconstructor: SourceInfoReconstructor
  ) {
    this.inlineConverter = new InlineConverter(sourceReconstructor);
  }

  /**
   * Convert a Block node to AnnotatedParse
   */
  convertBlock(block: Annotated_Block): AnnotatedParse {
    const source = this.sourceReconstructor.toMappedString(block.s);
    const [start, end] = this.sourceReconstructor.getOffsets(block.s);

    switch (block.t) {
      // Simple blocks with inline content
      case 'Plain':
        return {
          result: block.c as unknown as import('./types.js').JSONValue,
          kind: 'Plain',
          source,
          components: block.c.map(inline => this.inlineConverter.convertInline(inline)),
          start,
          end
        };

      case 'Para':
        return {
          result: block.c as unknown as import('./types.js').JSONValue,
          kind: 'Para',
          source,
          components: block.c.map(inline => this.inlineConverter.convertInline(inline)),
          start,
          end
        };

      // Empty blocks
      case 'HorizontalRule':
        return {
          result: null,
          kind: 'HorizontalRule',
          source,
          components: [],
          start,
          end
        };

      case 'Null':
        return {
          result: null,
          kind: 'Null',
          source,
          components: [],
          start,
          end
        };

      // Header: [level, attr, inlines]
      case 'Header':
        return {
          result: block.c as unknown as import('./types.js').JSONValue,
          kind: 'Header',
          source,
          components: [
            ...this.convertAttr(block.c[1], block.attrS),
            ...block.c[2].map(inline => this.inlineConverter.convertInline(inline))
          ],
          start,
          end
        };

      // CodeBlock: [attr, string]
      case 'CodeBlock':
        return {
          result: block.c as unknown as import('./types.js').JSONValue,
          kind: 'CodeBlock',
          source,
          components: this.convertAttr(block.c[0], block.attrS),
          start,
          end
        };

      // RawBlock: [format, content]
      case 'RawBlock':
        return {
          result: block.c as unknown as import('./types.js').JSONValue,
          kind: 'RawBlock',
          source,
          components: [],
          start,
          end
        };

      // BlockQuote: contains blocks
      case 'BlockQuote':
        return {
          result: block.c as unknown as import('./types.js').JSONValue,
          kind: 'BlockQuote',
          source,
          components: block.c.map(b => this.convertBlock(b)),
          start,
          end
        };

      // BulletList: [[blocks]]
      // NOTE: components are flattened - all blocks from all items in document order.
      // Item boundaries are lost. Reconstruct from result field or use helper API.
      // TODO: Create helper API to navigate list items (tracked in beads)
      case 'BulletList':
        return {
          result: block.c as unknown as import('./types.js').JSONValue,
          kind: 'BulletList',
          source,
          components: block.c.flatMap(item => item.map(b => this.convertBlock(b))),
          start,
          end
        };

      // OrderedList: [listAttrs, [[blocks]]]
      // NOTE: components are flattened - all blocks from all items in document order.
      // Item boundaries are lost. Reconstruct from result field or use helper API.
      // TODO: Create helper API to navigate list items (tracked in beads)
      case 'OrderedList':
        return {
          result: block.c as unknown as import('./types.js').JSONValue,
          kind: 'OrderedList',
          source,
          components: block.c[1].flatMap(item => item.map(b => this.convertBlock(b))),
          start,
          end
        };

      // Div: [attr, blocks]
      case 'Div':
        return {
          result: block.c as unknown as import('./types.js').JSONValue,
          kind: 'Div',
          source,
          components: [
            ...this.convertAttr(block.c[0], block.attrS),
            ...block.c[1].map(b => this.convertBlock(b))
          ],
          start,
          end
        };

      // Figure: [attr, caption, blocks]
      case 'Figure':
        return {
          result: block.c as unknown as import('./types.js').JSONValue,
          kind: 'Figure',
          source,
          components: [
            ...this.convertAttr(block.c[0], block.attrS),
            ...this.convertCaption({
              shortCaption: block.c[1][0],
              longCaption: block.c[1][1]
            }),
            ...block.c[2].map(b => this.convertBlock(b))
          ],
          start,
          end
        };

      // DefinitionList: [(term, [definitions])]
      // NOTE: components are flattened - terms and definitions in document order.
      // Structure lost: can't distinguish term boundaries, definition boundaries,
      // or which blocks belong to which definition. Reconstruct from result field
      // or use helper API.
      // TODO: Create helper API to navigate definition list structure (tracked in beads)
      case 'DefinitionList':
        return {
          result: block.c as unknown as import('./types.js').JSONValue,
          kind: 'DefinitionList',
          source,
          components: block.c.flatMap(([term, definitions]) => [
            // Convert term inlines
            ...term.map(inline => this.inlineConverter.convertInline(inline)),
            // Convert all definition blocks (flatten the nested arrays)
            ...definitions.flatMap(defBlocks => defBlocks.map(b => this.convertBlock(b)))
          ]),
          start,
          end
        };

      // Table excluded per k-188 specification
      case 'Table':
        throw new Error('Table conversion not implemented in Phase 3 (see k-190)');

      default:
        // Exhaustiveness check
        const _exhaustive: never = block;
        throw new Error(`Unknown block type: ${(_exhaustive as Annotated_Block).t}`);
    }
  }

  /**
   * Convert Attr tuple to AnnotatedParse components
   * Attr = [id, classes, kvPairs]
   * AttrSourceInfo = {id, classes, kvs}
   */
  private convertAttr(
    attr: [string, string[], [string, string][]],
    attrS: { id: number | null; classes: (number | null)[]; kvs: [number | null, number | null][] }
  ): AnnotatedParse[] {
    const components: AnnotatedParse[] = [];

    // ID
    if (attr[0] && attrS.id !== null) {
      const source = this.sourceReconstructor.toMappedString(attrS.id);
      const [start, end] = this.sourceReconstructor.getOffsets(attrS.id);
      components.push({
        result: attr[0],
        kind: 'attr-id',
        source,
        components: [],
        start,
        end
      });
    }

    // Classes
    for (let i = 0; i < attr[1].length; i++) {
      const className = attr[1][i];
      const classSourceId = attrS.classes[i];
      if (classSourceId !== null) {
        const source = this.sourceReconstructor.toMappedString(classSourceId);
        const [start, end] = this.sourceReconstructor.getOffsets(classSourceId);
        components.push({
          result: className,
          kind: 'attr-class',
          source,
          components: [],
          start,
          end
        });
      }
    }

    // Key-value pairs
    for (let i = 0; i < attr[2].length; i++) {
      const [key, value] = attr[2][i];
      const [keySourceId, valueSourceId] = attrS.kvs[i];

      if (keySourceId !== null) {
        const source = this.sourceReconstructor.toMappedString(keySourceId);
        const [start, end] = this.sourceReconstructor.getOffsets(keySourceId);
        components.push({
          result: key,
          kind: 'attr-key',
          source,
          components: [],
          start,
          end
        });
      }

      if (valueSourceId !== null) {
        const source = this.sourceReconstructor.toMappedString(valueSourceId);
        const [start, end] = this.sourceReconstructor.getOffsets(valueSourceId);
        components.push({
          result: value,
          kind: 'attr-value',
          source,
          components: [],
          start,
          end
        });
      }
    }

    return components;
  }

  /**
   * Convert Caption to AnnotatedParse components
   * Caption = { shortCaption: Inline[] | null, longCaption: Block[] }
   */
  private convertCaption(caption: Annotated_Caption): AnnotatedParse[] {
    const components: AnnotatedParse[] = [];

    // Short caption (if present)
    if (caption.shortCaption) {
      components.push(
        ...caption.shortCaption.map(inline => this.inlineConverter.convertInline(inline))
      );
    }

    // Long caption (always present)
    components.push(
      ...caption.longCaption.map(block => this.convertBlock(block))
    );

    return components;
  }
}
