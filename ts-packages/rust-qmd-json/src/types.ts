/**
 * Type definitions for quarto-markdown-pandoc JSON format
 * and AnnotatedParse structures
 */

import type { MappedString } from '@quarto/mapped-string';
import type { SerializableSourceInfo } from './source-map.js';

/**
 * JSON value type (matching quarto-cli's JSONValue)
 */
export type JSONValue =
  | string
  | number
  | boolean
  | null
  | JSONValue[]
  | { [key: string]: JSONValue };

/**
 * AnnotatedParse structure (matching quarto-cli's interface)
 */
export interface AnnotatedParse {
  start: number;
  end: number;
  result: JSONValue;
  kind: string;
  source: MappedString;
  components: AnnotatedParse[];
}

/**
 * MetaValue from quarto-markdown-pandoc JSON
 */
export interface JsonMetaValue {
  t: string;      // Type: "MetaString", "MetaBool", "MetaInlines", "MetaBlocks", "MetaList", "MetaMap"
  c?: unknown;    // Content (varies by type)
  s: number;      // SourceInfo ID
}

/**
 * MetaMap entry structure
 */
export interface MetaMapEntry {
  key: string;
  key_source: number;  // SourceInfo ID for key
  value: JsonMetaValue;
}

/**
 * Complete JSON output from quarto-markdown-pandoc
 */
export interface RustQmdJson {
  meta: Record<string, JsonMetaValue>;
  blocks: unknown[];  // Not used in metadata conversion
  source_pool: SerializableSourceInfo[];
  source_context: {
    files: Array<{
      id: number;
      path: string;
      content: string;
    }>;
  };
}
