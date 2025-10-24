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

  constructor(
    pool: SerializableSourceInfo[],
    sourceContext: SourceContext,
    errorHandler?: SourceInfoErrorHandler
  ) {
    this.pool = pool;
    this.sourceContext = sourceContext;
    this.errorHandler = errorHandler || defaultErrorHandler;
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

    // Find file in context
    const file = this.sourceContext.files.find(f => f.id === fileId);
    if (!file) {
      this.errorHandler(`File ID ${fileId} not found in source context`, id);
      return asMappedString('');
    }

    // Extract substring from file content
    const content = file.content.substring(start, end);
    return asMappedString(content, file.path);
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
      // Extract substring at specified offset/length
      const substring = mappedSubstring(pieceMapped, offset, offset + length);
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

      case 2: // Concat - use first piece's resolution
        // TODO: Concat doesn't have a single file location, so we use the first piece
        // For error reporting, this may not be ideal
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
              const [firstPieceId, offset, length] = pieces[0];
              const firstResolved = this.resolveChain(firstPieceId);
              // Offset into the first piece
              const [pieceStart, _] = firstResolved.range;
              resolved = {
                file_id: firstResolved.file_id,
                range: [pieceStart + offset, pieceStart + offset + length]
              };
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
