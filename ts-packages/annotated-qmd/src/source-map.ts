/**
 * SourceInfo Reconstruction
 *
 * Converts pooled SourceInfo from quarto-markdown-pandoc JSON output
 * into MappedString objects from @quarto/mapped-string.
 */

import { MappedString, asMappedString, mappedConcat, mappedSubstring } from '@quarto/mapped-string';

/**
 * Serialized SourceInfo from the JSON pool
 */
export interface SerializableSourceInfo {
  r: [number, number]; // [start_offset, end_offset]
  t: number;           // type code (0=Original, 1=Substring, 2=Concat)
  d: unknown;          // type-specific data (varies by type)
}

/**
 * Type guard for Concat data structure
 * Rust serializes Concat data as a plain array: [[source_info_id, offset, length], ...]
 */
function isConcatData(data: unknown): data is [number, number, number][] {
  return Array.isArray(data) && data.every(
    item => Array.isArray(item) && item.length === 3
  );
}

/**
 * Source context containing file information
 */
export interface SourceContext {
  files: Array<{
    id: number;
    path: string;
    content: string;
  }>;
}

/**
 * Resolved SourceInfo pointing to a file location
 */
interface ResolvedSource {
  file_id: number;
  range: [number, number];
}

/**
 * Error handler callback for SourceInfo reconstruction errors
 */
export type SourceInfoErrorHandler = (msg: string, id?: number) => void;

/**
 * Default error handler that throws on errors
 */
const defaultErrorHandler: SourceInfoErrorHandler = (msg: string, id?: number) => {
  const idStr = id !== undefined ? ` (SourceInfo ID: ${id})` : '';
  throw new Error(`SourceInfo reconstruction error: ${msg}${idStr}`);
};

/**
 * Reconstructs SourceInfo from pooled format to MappedString objects
 */
export class SourceInfoReconstructor {
  private pool: SerializableSourceInfo[];
  private sourceContext: SourceContext;
  private errorHandler: SourceInfoErrorHandler;
  private resolvedCache = new Map<number, ResolvedSource>();
  private mappedStringCache = new Map<number, MappedString>();
  private topLevelMappedStrings = new Map<number, MappedString>();

  constructor(
    pool: SerializableSourceInfo[],
    sourceContext: SourceContext,
    errorHandler?: SourceInfoErrorHandler
  ) {
    this.pool = pool;
    this.sourceContext = sourceContext;
    this.errorHandler = errorHandler || defaultErrorHandler;

    // Create top-level MappedStrings for all files
    // Validate that content is populated - this is required for proper source mapping
    for (const file of sourceContext.files) {
      if (file.content === null || file.content === undefined) {
        throw new Error(
          `File ${file.id} (${file.path}) missing content. ` +
          `astContext.files[].content must be populated for source mapping to work.`
        );
      }
      this.topLevelMappedStrings.set(
        file.id,
        asMappedString(file.content, file.path)
      );
    }
  }

  /**
   * Convert SourceInfo ID to MappedString
   */
  toMappedString(id: number): MappedString {
    // Check cache first
    const cached = this.mappedStringCache.get(id);
    if (cached) {
      return cached;
    }

    // Validate ID
    if (id < 0 || id >= this.pool.length) {
      this.errorHandler(`Invalid SourceInfo ID ${id} (pool size: ${this.pool.length})`, id);
      // Return empty MappedString as fallback
      return asMappedString('');
    }

    const info = this.pool[id];
    let result: MappedString;

    switch (info.t) {
      case 0: // Original
        result = this.handleOriginal(id, info);
        break;
      case 1: // Substring
        result = this.handleSubstring(id, info);
        break;
      case 2: // Concat
        result = this.handleConcat(id, info);
        break;
      default:
        this.errorHandler(`Unknown SourceInfo type ${info.t}`, id);
        result = asMappedString('');
    }

    // Cache and return
    this.mappedStringCache.set(id, result);
    return result;
  }

  /**
   * Get offsets from SourceInfo (without creating full MappedString)
   */
  getOffsets(id: number): [number, number] {
    if (id < 0 || id >= this.pool.length) {
      this.errorHandler(`Invalid SourceInfo ID ${id}`, id);
      return [0, 0];
    }
    return this.pool[id].r;
  }

  /**
   * Get top-level MappedString for a file
   *
   * This returns the full file content as a MappedString.
   * Use this for the AnnotatedParse.source field at the document level.
   */
  getTopLevelMappedString(fileId: number): MappedString {
    const result = this.topLevelMappedStrings.get(fileId);
    if (!result) {
      throw new Error(
        `No top-level MappedString for file ${fileId}. ` +
        `Available file IDs: ${Array.from(this.topLevelMappedStrings.keys()).join(', ')}`
      );
    }
    return result;
  }

  /**
   * Get file ID and offsets in top-level coordinates
   *
   * Resolves the SourceInfo chain to find which file this SourceInfo
   * ultimately refers to, and what offsets in that file's content.
   */
  getSourceLocation(id: number): { fileId: number; start: number; end: number } {
    const resolved = this.resolveChain(id);
    return {
      fileId: resolved.file_id,
      start: resolved.range[0],
      end: resolved.range[1]
    };
  }

  /**
   * Get all three AnnotatedParse source fields (source, start, end)
   *
   * This is the primary API for converters to use. It returns:
   * - source: The top-level MappedString for the file (full file content)
   * - start: Offset in top-level coordinates
   * - end: Offset in top-level coordinates
   *
   * Invariant: source.value.substring(start, end) extracts the correct text
   */
  getAnnotatedParseSourceFields(id: number): {
    source: MappedString;
    start: number;
    end: number;
  } {
    const { fileId, start, end } = this.getSourceLocation(id);
    return {
      source: this.getTopLevelMappedString(fileId),
      start,
      end
    };
  }

  /**
   * Handle Original SourceInfo type (t=0)
   * Data format: file_id (number)
   */
  private handleOriginal(id: number, info: SerializableSourceInfo): MappedString {
    // Runtime type check
    if (typeof info.d !== 'number') {
      this.errorHandler(`Original SourceInfo data must be a number (file_id), got ${typeof info.d}`, id);
      return asMappedString('');
    }

    const fileId = info.d;
    const [start, end] = info.r;

    // Get top-level MappedString for this file
    const topLevel = this.topLevelMappedStrings.get(fileId);
    if (!topLevel) {
      this.errorHandler(`File ID ${fileId} not found in source context`, id);
      return asMappedString('');
    }

    // Use mappedSubstring to maintain connection to top-level file
    // This preserves the mapping chain so that AnnotatedParse.source can reference top-level
    return mappedSubstring(topLevel, start, end);
  }

  /**
   * Handle Substring SourceInfo type (t=1)
   * Data format: parent_id (number)
   * The range in info.r is relative to the parent's content
   */
  private handleSubstring(id: number, info: SerializableSourceInfo): MappedString {
    // Runtime type check
    if (typeof info.d !== 'number') {
      this.errorHandler(`Substring SourceInfo data must be a number (parent_id), got ${typeof info.d}`, id);
      return asMappedString('');
    }

    const parentId = info.d;
    const [localStart, localEnd] = info.r;

    // Get parent MappedString (recursive, with caching)
    const parent = this.toMappedString(parentId);

    // Create substring with offset mapping
    return mappedSubstring(parent, localStart, localEnd);
  }

  /**
   * Handle Concat SourceInfo type (t=2)
   * Data format: [[source_info_id, offset, length], ...]
   * (Rust serializes as plain array, not object with pieces field)
   */
  private handleConcat(id: number, info: SerializableSourceInfo): MappedString {
    // Runtime type check
    if (!isConcatData(info.d)) {
      this.errorHandler(`Invalid Concat data format (expected array of [id, offset, length]), got ${typeof info.d}`, id);
      return asMappedString('');
    }

    const pieces = info.d;  // Direct array access

    // Build MappedString array from pieces
    const mappedPieces: MappedString[] = [];
    for (const [pieceId, offset, length] of pieces) {
      const pieceMapped = this.toMappedString(pieceId);
      // Extract first 'length' characters from this piece
      // Note: 'offset' is offset_in_concat (where piece goes in final string),
      // NOT an offset into the piece itself
      const substring = mappedSubstring(pieceMapped, 0, length);
      mappedPieces.push(substring);
    }

    // Concatenate all pieces
    if (mappedPieces.length === 0) {
      return asMappedString('');
    }
    if (mappedPieces.length === 1) {
      return mappedPieces[0];
    }

    return mappedConcat(mappedPieces);
  }

  /**
   * Recursively resolve SourceInfo chains to find original file location
   * This is cached to avoid re-resolving deep chains
   */
  private resolveChain(id: number): ResolvedSource {
    // Check cache first
    const cached = this.resolvedCache.get(id);
    if (cached) {
      return cached;
    }

    // Validate ID
    if (id < 0 || id >= this.pool.length) {
      this.errorHandler(`Invalid SourceInfo ID ${id}`, id);
      return { file_id: -1, range: [0, 0] };
    }

    const info = this.pool[id];
    let resolved: ResolvedSource;

    switch (info.t) {
      case 0: // Original - base case
        {
          if (typeof info.d !== 'number') {
            this.errorHandler(`Original SourceInfo data must be a number`, id);
            resolved = { file_id: -1, range: info.r };
          } else {
            resolved = {
              file_id: info.d,
              range: info.r
            };
          }
        }
        break;

      case 1: // Substring - chain through parent
        {
          if (typeof info.d !== 'number') {
            this.errorHandler(`Substring SourceInfo data must be a number`, id);
            resolved = { file_id: -1, range: info.r };
          } else {
            const parentResolved = this.resolveChain(info.d);
            const [localStart, localEnd] = info.r;
            const [parentStart, _] = parentResolved.range;
            resolved = {
              file_id: parentResolved.file_id,
              range: [parentStart + localStart, parentStart + localEnd]
            };
          }
        }
        break;

      case 2: // Concat - resolve using MappedString.map()
        {
          if (!isConcatData(info.d)) {
            this.errorHandler(`Invalid Concat data format`, id);
            resolved = { file_id: -1, range: info.r };
          } else {
            const pieces = info.d;  // Direct array access
            if (pieces.length === 0) {
              this.errorHandler(`Empty Concat pieces`, id);
              resolved = { file_id: -1, range: info.r };
            } else {
              // Get the concatenated MappedString (already built by handleConcat)
              const concatMapped = this.toMappedString(id);

              if (concatMapped.value.length === 0) {
                // Empty concat - use first piece's location
                const [firstPieceId] = pieces[0];
                const firstResolved = this.resolveChain(firstPieceId);
                resolved = {
                  file_id: firstResolved.file_id,
                  range: [firstResolved.range[0], firstResolved.range[0]]
                };
                break;
              }

              // Map the start position (offset 0 in concat)
              const startMap = concatMapped.map(0);
              if (!startMap) {
                this.errorHandler(`Failed to map start position for Concat`, id);
                resolved = { file_id: -1, range: info.r };
                break;
              }

              // Map the last character position (length - 1)
              const lastCharMap = concatMapped.map(concatMapped.value.length - 1);
              if (!lastCharMap) {
                this.errorHandler(`Failed to map last character position for Concat`, id);
                resolved = { file_id: -1, range: info.r };
                break;
              }

              // Find the file_id by checking which top-level MappedString this is
              let file_id = -1;
              for (const [fid, topLevel] of this.topLevelMappedStrings.entries()) {
                if (topLevel === startMap.originalString) {
                  file_id = fid;
                  break;
                }
              }

              if (file_id === -1) {
                this.errorHandler(`Could not find file_id for Concat originalString`, id);
                resolved = { file_id: -1, range: info.r };
              } else {
                // End position is one past the last character
                resolved = {
                  file_id,
                  range: [startMap.index, lastCharMap.index + 1]
                };
              }
            }
          }
        }
        break;

      default:
        this.errorHandler(`Unknown SourceInfo type ${info.t}`, id);
        resolved = { file_id: -1, range: [0, 0] };
    }

    // Cache and return
    this.resolvedCache.set(id, resolved);
    return resolved;
  }

  // TODO: Implement circular reference detection
  // This would require tracking visited IDs during resolveChain traversal
}
