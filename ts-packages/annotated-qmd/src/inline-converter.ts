/**
 * Inline Conversion
 *
 * Converts Inline AST nodes from quarto-markdown-pandoc JSON
 * into AnnotatedParse structures compatible with quarto-cli.
 */

import type { AnnotatedParse } from './types.js';
import type { SourceInfoReconstructor } from './source-map.js';
import type { Annotated_Inline } from './pandoc-types.js';

/**
 * Converts Inline AST nodes from quarto-markdown-pandoc to AnnotatedParse
 */
export class InlineConverter {
  constructor(
    private sourceReconstructor: SourceInfoReconstructor
  ) {}

  /**
   * Convert an Inline node to AnnotatedParse
   */
  convertInline(inline: Annotated_Inline): AnnotatedParse {
    const source = this.sourceReconstructor.toMappedString(inline.s);
    const [start, end] = this.sourceReconstructor.getOffsets(inline.s);

    switch (inline.t) {
      // Simple text nodes
      case 'Str':
        return {
          result: inline.c,
          kind: 'Str',
          source,
          components: [],
          start,
          end
        };

      case 'Space':
        return {
          result: null,  // Space has no content
          kind: 'Space',
          source,
          components: [],
          start,
          end
        };

      case 'SoftBreak':
        return {
          result: null,
          kind: 'SoftBreak',
          source,
          components: [],
          start,
          end
        };

      case 'LineBreak':
        return {
          result: null,
          kind: 'LineBreak',
          source,
          components: [],
          start,
          end
        };

      // Formatting (recursive - contain child inlines)
      case 'Emph':
        return {
          result: inline.c as unknown as import('./types.js').JSONValue,  // Keep Pandoc JSON AS-IS
          kind: 'Emph',
          source,
          components: inline.c.map(child => this.convertInline(child)),
          start,
          end
        };

      case 'Strong':
        return {
          result: inline.c as unknown as import('./types.js').JSONValue,
          kind: 'Strong',
          source,
          components: inline.c.map(child => this.convertInline(child)),
          start,
          end
        };

      case 'Strikeout':
        return {
          result: inline.c as unknown as import('./types.js').JSONValue,
          kind: 'Strikeout',
          source,
          components: inline.c.map(child => this.convertInline(child)),
          start,
          end
        };

      case 'Superscript':
        return {
          result: inline.c as unknown as import('./types.js').JSONValue,
          kind: 'Superscript',
          source,
          components: inline.c.map(child => this.convertInline(child)),
          start,
          end
        };

      case 'Subscript':
        return {
          result: inline.c as unknown as import('./types.js').JSONValue,
          kind: 'Subscript',
          source,
          components: inline.c.map(child => this.convertInline(child)),
          start,
          end
        };

      case 'SmallCaps':
        return {
          result: inline.c as unknown as import('./types.js').JSONValue,
          kind: 'SmallCaps',
          source,
          components: inline.c.map(child => this.convertInline(child)),
          start,
          end
        };

      case 'Underline':
        return {
          result: inline.c as unknown as import('./types.js').JSONValue,
          kind: 'Underline',
          source,
          components: inline.c.map(child => this.convertInline(child)),
          start,
          end
        };

      // Quoted (has QuoteType and children)
      case 'Quoted':
        return {
          result: inline.c as unknown as import('./types.js').JSONValue,
          kind: 'Quoted',
          source,
          components: inline.c[1].map(child => this.convertInline(child)),
          start,
          end
        };

      // Code (has Attr and string content + attrS)
      case 'Code':
        return {
          result: inline.c as unknown as import('./types.js').JSONValue,
          kind: 'Code',
          source,
          components: this.convertAttr(inline.c[0], inline.attrS),
          start,
          end
        };

      // Math (has MathType and string)
      case 'Math':
        return {
          result: inline.c as unknown as import('./types.js').JSONValue,
          kind: 'Math',
          source,
          components: [],
          start,
          end
        };

      // RawInline (has format and content)
      case 'RawInline':
        return {
          result: inline.c as unknown as import('./types.js').JSONValue,
          kind: 'RawInline',
          source,
          components: [],
          start,
          end
        };

      // Link (has Attr, Inlines, Target + attrS + targetS)
      case 'Link':
        return {
          result: inline.c as unknown as import('./types.js').JSONValue,
          kind: 'Link',
          source,
          components: [
            ...this.convertAttr(inline.c[0], inline.attrS),
            ...inline.c[1].map(child => this.convertInline(child)),
            ...this.convertTarget(inline.c[2], inline.targetS)
          ],
          start,
          end
        };

      // Image (has Attr, Inlines, Target + attrS + targetS)
      case 'Image':
        return {
          result: inline.c as unknown as import('./types.js').JSONValue,
          kind: 'Image',
          source,
          components: [
            ...this.convertAttr(inline.c[0], inline.attrS),
            ...inline.c[1].map(child => this.convertInline(child)),
            ...this.convertTarget(inline.c[2], inline.targetS)
          ],
          start,
          end
        };

      // Span (has Attr and Inlines + attrS)
      case 'Span':
        return {
          result: inline.c as unknown as import('./types.js').JSONValue,
          kind: 'Span',
          source,
          components: [
            ...this.convertAttr(inline.c[0], inline.attrS),
            ...inline.c[1].map(child => this.convertInline(child))
          ],
          start,
          end
        };

      // Cite (has Citations and Inlines)
      case 'Cite':
        return {
          result: inline.c as unknown as import('./types.js').JSONValue,
          kind: 'Cite',
          source,
          components: [
            ...inline.c[0].flatMap(citation => this.convertCitation(citation)),
            ...inline.c[1].map(child => this.convertInline(child))
          ],
          start,
          end
        };

      // Note (has Blocks - cross-reference, will need BlockConverter)
      case 'Note':
        return {
          result: inline.c as unknown as import('./types.js').JSONValue,
          kind: 'Note',
          source,
          components: [],  // Will be filled in when BlockConverter is available
          start,
          end
        };

      default:
        // Exhaustiveness check
        const _exhaustive: never = inline;
        throw new Error(`Unknown inline type: ${(_exhaustive as Annotated_Inline).t}`);
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
   * Convert Target tuple to AnnotatedParse components
   * Target = [url, title]
   * TargetSourceInfo = [urlSourceId, titleSourceId]
   */
  private convertTarget(
    target: [string, string],
    targetS: [number | null, number | null]
  ): AnnotatedParse[] {
    const components: AnnotatedParse[] = [];

    // URL
    if (target[0] && targetS[0] !== null) {
      const source = this.sourceReconstructor.toMappedString(targetS[0]);
      const [start, end] = this.sourceReconstructor.getOffsets(targetS[0]);
      components.push({
        result: target[0],
        kind: 'target-url',
        source,
        components: [],
        start,
        end
      });
    }

    // Title
    if (target[1] && targetS[1] !== null) {
      const source = this.sourceReconstructor.toMappedString(targetS[1]);
      const [start, end] = this.sourceReconstructor.getOffsets(targetS[1]);
      components.push({
        result: target[1],
        kind: 'target-title',
        source,
        components: [],
        start,
        end
      });
    }

    return components;
  }

  /**
   * Convert Citation to AnnotatedParse components
   */
  private convertCitation(
    citation: {
      citationId: string;
      citationPrefix: Annotated_Inline[];
      citationSuffix: Annotated_Inline[];
      citationMode: unknown;
      citationNoteNum: number;
      citationHash: number;
      citationIdS: number | null;
    }
  ): AnnotatedParse[] {
    const components: AnnotatedParse[] = [];

    // Citation ID
    if (citation.citationIdS !== null) {
      const source = this.sourceReconstructor.toMappedString(citation.citationIdS);
      const [start, end] = this.sourceReconstructor.getOffsets(citation.citationIdS);
      components.push({
        result: citation.citationId,
        kind: 'citation-id',
        source,
        components: [],
        start,
        end
      });
    }

    // Prefix inlines
    components.push(
      ...citation.citationPrefix.map(inline => this.convertInline(inline))
    );

    // Suffix inlines
    components.push(
      ...citation.citationSuffix.map(inline => this.convertInline(inline))
    );

    return components;
  }
}
