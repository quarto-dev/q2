/**
 * Block Conversion
 *
 * Converts Block AST nodes from quarto-markdown-pandoc JSON
 * into AnnotatedParse structures compatible with quarto-cli.
 */

import type { AnnotatedParse, Annotated_Block, Annotated_Caption } from './types.js';
import type { SourceInfoReconstructor } from './source-map.js';
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
    // Wire the converters together to handle Note elements with block content
    this.inlineConverter.setBlockConverter(this);
  }

  /**
   * Convert a Block node to AnnotatedParse
   */
  convertBlock(block: Annotated_Block): AnnotatedParse {
    const { source, start, end } =
      this.sourceReconstructor.getAnnotatedParseSourceFields(block.s);

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
      // TODO (k-193): Create helper API to navigate list items
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
      // TODO (k-193): Create helper API to navigate list items
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
      // Components in source order: caption, blocks, then attr (attr comes after in source)
      case 'Figure':
        return {
          result: block.c as unknown as import('./types.js').JSONValue,
          kind: 'Figure',
          source,
          components: [
            ...this.convertCaption({
              shortCaption: block.c[1][0],
              longCaption: block.c[1][1]
            }),
            ...block.c[2].map(b => this.convertBlock(b)),
            ...this.convertAttr(block.c[0], block.attrS)
          ],
          start,
          end
        };

      // DefinitionList: [(term, [definitions])]
      // NOTE: components are flattened - terms and definitions in document order.
      // Structure lost: can't distinguish term boundaries, definition boundaries,
      // or which blocks belong to which definition. Reconstruct from result field
      // or use helper API.
      // TODO (k-193): Create helper API to navigate definition list structure
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

      // Table: [attr, caption, colspec, head, bodies, foot]
      // Components flattened: attr, caption content, all rows/cells in document order
      case 'Table':
        return {
          result: block.c as unknown as import('./types.js').JSONValue,
          kind: 'Table',
          source,
          components: [
            // Components in source order:
            // TableHead (structural node)
            this.convertTableHead(block.c[3], block.headS),
            // TableBody (structural nodes, multiple bodies)
            ...block.c[4].map((body, i) =>
              this.convertTableBody(body, block.bodiesS[i])
            ),
            // TableFoot (structural node)
            this.convertTableFoot(block.c[5], block.footS),
            // Caption (short and long structural nodes) - comes after table in source
            ...this.convertCaption({
              shortCaption: block.c[1][0],
              longCaption: block.c[1][1]
            }, block.captionS),
            // Table attr - extracted from caption, so comes last
            ...this.convertAttr(block.c[0], block.attrS)
          ],
          start,
          end
        };

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
      const { source, start, end } =
        this.sourceReconstructor.getAnnotatedParseSourceFields(attrS.id);
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
        const { source, start, end } =
          this.sourceReconstructor.getAnnotatedParseSourceFields(classSourceId);
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
        const { source, start, end } =
          this.sourceReconstructor.getAnnotatedParseSourceFields(keySourceId);
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
        const { source, start, end } =
          this.sourceReconstructor.getAnnotatedParseSourceFields(valueSourceId);
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
   *
   * If captionS is provided (Table case), creates structural nodes for short/long.
   * If captionS is not provided (Figure case), flattens components as before.
   */
  private convertCaption(caption: Annotated_Caption, captionS?: number): AnnotatedParse[] {
    const components: AnnotatedParse[] = [];

    if (captionS !== undefined) {
      // Create structural nodes using caption source info
      const source = this.sourceReconstructor.toMappedString(captionS);
      const [start, end] = this.sourceReconstructor.getOffsets(captionS);

      // Short caption structural node (if present)
      if (caption.shortCaption) {
        components.push({
          result: caption.shortCaption as unknown as import('./types.js').JSONValue,
          kind: 'caption-short',
          source,
          components: caption.shortCaption.map(inline => this.inlineConverter.convertInline(inline)),
          start,
          end
        });
      }

      // Long caption structural node (always present)
      components.push({
        result: caption.longCaption as unknown as import('./types.js').JSONValue,
        kind: 'caption-long',
        source,
        components: caption.longCaption.map(block => this.convertBlock(block)),
        start,
        end
      });
    } else {
      // Flatten components (Figure case, no caption source info)
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
    }

    return components;
  }

  /**
   * Convert TableHead to AnnotatedParse components
   * TableHead = [attr, rows]
   */
  private convertTableHead(
    head: import('./types.js').Annotated_TableHead_Array,
    headS: import('./types.js').TableHeadSourceInfo
  ): AnnotatedParse {
    const source = this.sourceReconstructor.toMappedString(headS.s);
    const [start, end] = this.sourceReconstructor.getOffsets(headS.s);

    const components: AnnotatedParse[] = [];

    // Head attr
    components.push(...this.convertAttr(head[0], headS.attrS));

    // Head rows (each row is now a structural node)
    head[1].forEach((row, i) => {
      components.push(this.convertRow(row, headS.rowsS[i]));
    });

    return {
      result: head as unknown as import('./types.js').JSONValue,
      kind: 'table-head',
      source,
      components,
      start,
      end
    };
  }

  /**
   * Convert TableBody to AnnotatedParse components
   * TableBody = [attr, rowHeadColumns, head, body]
   */
  private convertTableBody(
    body: import('./types.js').Annotated_TableBody_Array,
    bodyS: import('./types.js').TableBodySourceInfo
  ): AnnotatedParse {
    const source = this.sourceReconstructor.toMappedString(bodyS.s);
    const [start, end] = this.sourceReconstructor.getOffsets(bodyS.s);

    const components: AnnotatedParse[] = [];

    // Body attr
    components.push(...this.convertAttr(body[0], bodyS.attrS));

    // Body head rows (each row is now a structural node)
    body[2].forEach((row, i) => {
      components.push(this.convertRow(row, bodyS.headS[i]));
    });

    // Body body rows (each row is now a structural node)
    body[3].forEach((row, i) => {
      components.push(this.convertRow(row, bodyS.bodyS[i]));
    });

    return {
      result: body as unknown as import('./types.js').JSONValue,
      kind: 'table-body',
      source,
      components,
      start,
      end
    };
  }

  /**
   * Convert TableFoot to AnnotatedParse components
   * TableFoot = [attr, rows]
   */
  private convertTableFoot(
    foot: import('./types.js').Annotated_TableFoot_Array,
    footS: import('./types.js').TableFootSourceInfo
  ): AnnotatedParse {
    const source = this.sourceReconstructor.toMappedString(footS.s);
    const [start, end] = this.sourceReconstructor.getOffsets(footS.s);

    const components: AnnotatedParse[] = [];

    // Foot attr
    components.push(...this.convertAttr(foot[0], footS.attrS));

    // Foot rows (each row is now a structural node)
    foot[1].forEach((row, i) => {
      components.push(this.convertRow(row, footS.rowsS[i]));
    });

    return {
      result: foot as unknown as import('./types.js').JSONValue,
      kind: 'table-foot',
      source,
      components,
      start,
      end
    };
  }

  /**
   * Convert Row to AnnotatedParse components
   * Row = [attr, cells]
   */
  private convertRow(
    row: import('./types.js').Annotated_Row,
    rowS: import('./types.js').RowSourceInfo
  ): AnnotatedParse {
    const source = this.sourceReconstructor.toMappedString(rowS.s);
    const [start, end] = this.sourceReconstructor.getOffsets(rowS.s);

    const components: AnnotatedParse[] = [];

    // Row attr
    components.push(...this.convertAttr(row[0], rowS.attrS));

    // Row cells (each cell is now a structural node)
    row[1].forEach((cell, i) => {
      components.push(this.convertCell(cell, rowS.cellsS[i]));
    });

    return {
      result: row as unknown as import('./types.js').JSONValue,
      kind: 'table-row',
      source,
      components,
      start,
      end
    };
  }

  /**
   * Convert Cell to AnnotatedParse components
   * Cell = [attr, alignment, rowSpan, colSpan, content]
   */
  private convertCell(
    cell: import('./types.js').Annotated_Cell,
    cellS: import('./types.js').CellSourceInfo
  ): AnnotatedParse {
    const source = this.sourceReconstructor.toMappedString(cellS.s);
    const [start, end] = this.sourceReconstructor.getOffsets(cellS.s);

    const components: AnnotatedParse[] = [];

    // Cell attr
    components.push(...this.convertAttr(cell[0], cellS.attrS));

    // Cell content (blocks)
    components.push(...cell[4].map(block => this.convertBlock(block)));

    return {
      result: cell as unknown as import('./types.js').JSONValue,
      kind: 'table-cell',
      source,
      components,
      start,
      end
    };
  }
}
