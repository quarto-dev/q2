/**
 * Document Conversion
 *
 * Provides a DocumentConverter class that orchestrates InlineConverter,
 * BlockConverter, and MetadataConverter to convert complete Pandoc documents
 * from quarto-markdown-pandoc JSON into AnnotatedParse structures.
 */

import type {
  AnnotatedParse,
  JsonMetaValue,
  RustQmdJson,
  Annotated_Block,
  Annotated_Inline
} from './types.js';
import type { SourceInfoReconstructor } from './source-map.js';
import { asMappedString } from '@quarto/mapped-string';
import { InlineConverter } from './inline-converter.js';
import { BlockConverter } from './block-converter.js';
import { MetadataConverter } from './meta-converter.js';

/**
 * Converts complete Pandoc documents from quarto-markdown-pandoc
 */
export class DocumentConverter {
  private inlineConverter: InlineConverter;
  private blockConverter: BlockConverter;
  private metadataConverter: MetadataConverter;

  constructor(
    private sourceReconstructor: SourceInfoReconstructor,
    metaTopLevelKeySources?: Record<string, number>
  ) {
    this.inlineConverter = new InlineConverter(sourceReconstructor);
    this.blockConverter = new BlockConverter(sourceReconstructor);
    this.metadataConverter = new MetadataConverter(
      sourceReconstructor,
      metaTopLevelKeySources
    );
  }

  /**
   * Convert a complete Pandoc document to AnnotatedParse
   *
   * Returns an AnnotatedParse with:
   * - result: The original document JSON
   * - kind: 'Document'
   * - source: Full document source (if available)
   * - components: Array of metadata and block AnnotatedParse nodes
   */
  convertDocument(doc: RustQmdJson): AnnotatedParse {
    const components: AnnotatedParse[] = [];

    // Convert metadata (if present)
    if (doc.meta && Object.keys(doc.meta).length > 0) {
      components.push(this.metadataConverter.convertMeta(doc.meta));
    }

    // Convert all blocks
    if (doc.blocks && doc.blocks.length > 0) {
      components.push(...doc.blocks.map(block => this.blockConverter.convertBlock(block)));
    }

    // Try to get overall document source if we have file context
    // For now, use empty MappedString as we don't track document-level source
    const source = asMappedString('');
    const start = 0;
    const end = 0;

    return {
      result: doc as unknown as import('./types.js').JSONValue,
      kind: 'Document',
      source,
      components,
      start,
      end
    };
  }

  /**
   * Convert an array of blocks to an array of AnnotatedParse nodes
   */
  convertBlocks(blocks: Annotated_Block[]): AnnotatedParse[] {
    return blocks.map(block => this.blockConverter.convertBlock(block));
  }

  /**
   * Convert a single block to AnnotatedParse
   */
  convertBlock(block: Annotated_Block): AnnotatedParse {
    return this.blockConverter.convertBlock(block);
  }

  /**
   * Convert a single inline to AnnotatedParse
   */
  convertInline(inline: Annotated_Inline): AnnotatedParse {
    return this.inlineConverter.convertInline(inline);
  }
}
