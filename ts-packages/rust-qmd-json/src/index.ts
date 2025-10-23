/**
 * @quarto/rust-qmd-json
 *
 * Converts quarto-markdown-pandoc JSON output to AnnotatedParse structures
 * compatible with quarto-cli's YAML validation infrastructure.
 */

// Re-export types and functions from @quarto/mapped-string
export type { MappedString } from '@quarto/mapped-string';
export { asMappedString, mappedSubstring } from '@quarto/mapped-string';

// Re-export our types
export type {
  AnnotatedParse,
  JSONValue,
  JsonMetaValue,
  MetaMapEntry,
  RustQmdJson
} from './types.js';

export type {
  SerializableSourceInfo,
  SourceContext,
  SourceInfoErrorHandler
} from './source-map.js';

// Re-export classes
export { SourceInfoReconstructor } from './source-map.js';
export { MetadataConverter } from './meta-converter.js';

// Import for main function
import { SourceInfoReconstructor } from './source-map.js';
import { MetadataConverter } from './meta-converter.js';
import type { RustQmdJson, AnnotatedParse } from './types.js';
import type { SourceInfoErrorHandler } from './source-map.js';

/**
 * Convert quarto-markdown-pandoc JSON output to AnnotatedParse
 *
 * @param json - The JSON output from quarto-markdown-pandoc
 * @param errorHandler - Optional error handler for SourceInfo reconstruction errors
 * @returns AnnotatedParse structure compatible with quarto-cli
 *
 * @example
 * ```typescript
 * import { parseRustQmdMetadata } from '@quarto/rust-qmd-json';
 *
 * const json = {
 *   meta: {
 *     title: { t: 'MetaString', c: 'Hello', s: 0 }
 *   },
 *   blocks: [],
 *   source_pool: [
 *     { r: [11, 16], t: 0, d: 0 }
 *   ],
 *   source_context: {
 *     files: [
 *       { id: 0, path: 'test.qmd', content: '---\ntitle: Hello\n---' }
 *     ]
 *   }
 * };
 *
 * const metadata = parseRustQmdMetadata(json);
 * console.log(metadata.result); // { title: 'Hello' }
 * ```
 */
export function parseRustQmdMetadata(
  json: RustQmdJson,
  errorHandler?: SourceInfoErrorHandler
): AnnotatedParse {
  // 1. Create SourceInfoReconstructor with pool and context
  const sourceReconstructor = new SourceInfoReconstructor(
    json.source_pool,
    json.source_context,
    errorHandler
  );

  // 2. Create MetadataConverter
  const converter = new MetadataConverter(sourceReconstructor);

  // 3. Convert metadata to AnnotatedParse
  return converter.convertMeta(json.meta);
}
