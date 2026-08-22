/**
 * SourceInfo Reconstruction
 *
 * Converts pooled SourceInfo from quarto-markdown-pandoc JSON output
 * into MappedString objects from @quarto/mapped-string.
 */

import { MappedString, asMappedString, mappedConcat, mappedSubstring } from '@quarto/mapped-string';
import type { SerializableSourceInfo, SourceContext, SourceInfoErrorHandler } from './types.js';

export type { SerializableSourceInfo, SourceContext, SourceInfoErrorHandler };

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
 * Resolved SourceInfo pointing to a file location
 */
interface ResolvedSource {
  file_id: number;
  range: [number, number];
}

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
   * Invariant: source.value.substring(start, end) extracts the raw source
   * text at that range — not necessarily the decoded value. For content
   * that was unescaped or reparsed (e.g. `\*` -> `*`), the raw substring
   * differs from the node's own decoded `result`; use `result` when the
   * decoded text is what's needed.
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
   * Extent of a SourceInfo in its **own** offset space.
   *
   * `r` always spans the node, but the space it is expressed in differs by
   * type: for `Original` it is file coordinates, for `Substring` it is the
   * parent's content coordinates, and for `Concat` it is the node's own
   * content coordinates (`[0, contentLength]`). The *length* is the same
   * number in every case, which is all this needs to report.
   */
  private contentLength(info: SerializableSourceInfo): number {
    return info.r[1] - info.r[0];
  }

  /**
   * Map a half-open **content** range of `id` to the source range that
   * produced it.
   *
   * This composes through each link's *mapping*, never affinely over a
   * resolved hull. The distinction only shows up once a node's content
   * differs byte-for-byte from its source, which is exactly what content
   * provenance introduces:
   *
   * - a markdown attribute value collapses `\X` (2 source bytes) to `X`
   *   (1 content byte);
   * - a YAML block scalar collapses a newline plus its indentation to a
   *   single newline, and appends a trailing newline that no source byte
   *   produced at all.
   *
   * Adding a content offset to a parent's resolved start therefore drifts by
   * the bytes every earlier collapse removed, and deriving an exclusive end
   * by mapping the last content byte and adding one lands *inside* a
   * trailing multi-byte piece.
   *
   * **Requires a gap-free, single-file tiling** to report a tight hull, which
   * is what the Rust producer emits: `\X` -> `X` consumes exactly the two
   * source bytes it replaces, and quote stripping trims the content range's
   * ends rather than leaving an interior gap. A gappy tiling — a dropped
   * zero-content piece is the likely way to introduce one — still yields a
   * hull, but a loose one that spans bytes the content does not own. A tiling
   * whose pieces span more than one file has no single range at all and takes
   * the error path.
   */
  private mapContentRange(id: number, start: number, end: number): ResolvedSource {
    if (id < 0 || id >= this.pool.length) {
      this.errorHandler(`Invalid SourceInfo ID ${id}`, id);
      return { file_id: -1, range: [0, 0] };
    }

    const info = this.pool[id];

    switch (info.t) {
      case 0: // Original - base case: content space is file space
        if (typeof info.d !== 'number') {
          this.errorHandler(`Original SourceInfo data must be a number`, id);
          return { file_id: -1, range: info.r };
        }
        return { file_id: info.d, range: [info.r[0] + start, info.r[0] + end] };

      case 1: // Substring - shift into the parent's content space
        if (typeof info.d !== 'number') {
          this.errorHandler(`Substring SourceInfo data must be a number`, id);
          return { file_id: -1, range: info.r };
        }
        return this.mapContentRange(info.d, info.r[0] + start, info.r[0] + end);

      case 2: // Concat - union the contributions of the overlapping pieces
        {
          if (!isConcatData(info.d)) {
            this.errorHandler(`Invalid Concat data format`, id);
            return { file_id: -1, range: info.r };
          }
          const pieces = info.d;
          if (pieces.length === 0) {
            this.errorHandler(`Empty Concat pieces`, id);
            return { file_id: -1, range: info.r };
          }

          let file_id = -1;
          let lo = Number.POSITIVE_INFINITY;
          let hi = Number.NEGATIVE_INFINITY;
          // `lastPieceEnd`/`lastPieceFileId` are overwritten on every
          // iteration that reaches the `start >= pieceEnd` branch below, so
          // whichever such piece is visited *last* wins. That is correct
          // only because pieces arrive in ascending `offset_in_concat` order
          // (as `pampa` emits them — see
          // `crates/pampa/src/writers/json.rs`), making iteration order and
          // position order coincide. The hull computed from `lo`/`hi` above
          // does not depend on this — it is order-independent — but this
          // fallback path would silently pick the wrong piece for the
          // zero-width-at-the-end case if pieces ever arrived unsorted.
          let lastPieceEnd: number | undefined;
          let lastPieceFileId: number | undefined;

          // A piece is [source_info_id, offset_in_concat, content_length].
          for (const [pieceId, pieceOffset, pieceLength] of pieces) {
            if (pieceId < 0 || pieceId >= this.pool.length) {
              this.errorHandler(`Invalid Concat piece SourceInfo ID ${pieceId}`, id);
              return { file_id: -1, range: info.r };
            }
            const pieceEnd = pieceOffset + pieceLength;

            // A piece whose declared content length equals its own extent
            // has a *positional* correspondence between content offsets and
            // source offsets — content offset k came from source offset k —
            // so it can be indexed into. That is not the same as the bytes
            // being identical: `quarto-source-map`'s own frozen test for
            // `replacement(3..4, 1)` is length-preserving (1 source byte maps
            // to 1 content byte) while replacing `\n` with `' '`
            // (`quarto-source-map-*/src/provenance_builder.rs`). The
            // `verbatim` flag that would tell TS "bytes are identical" does
            // not survive onto the wire at all — lengths are all we have.
            //
            // Any piece that is NOT length-preserving is a replacement: its
            // content bytes have no per-byte correspondence with its source
            // bytes, so it is opaque and contributes its whole source span or
            // nothing.
            //
            // Do NOT reuse this predicate to reconstruct a *string* — only
            // ranges are safe here, because positional correspondence says
            // nothing about byte equality. `handleConcat` above does exactly
            // this misuse (see bd-g7qh1ltt): it slices a piece's *source*
            // text as if it were the piece's *content* text, which is wrong
            // whenever the piece is a replacement.
            const lengthPreserving = pieceLength === this.contentLength(this.pool[pieceId]);

            let contribution: ResolvedSource | undefined;
            if (start === end) {
              // Zero-width query: the piece that *contains* the point.
              if (start >= pieceOffset && start < pieceEnd) {
                const local = start - pieceOffset;
                contribution = lengthPreserving
                  ? this.mapContentRange(pieceId, local, local)
                  : this.mapContentRange(pieceId, 0, 0);
              }
            } else if (start < pieceEnd && end > pieceOffset) {
              contribution = lengthPreserving
                ? this.mapContentRange(
                    pieceId,
                    Math.max(start, pieceOffset) - pieceOffset,
                    Math.min(end, pieceEnd) - pieceOffset
                  )
                : this.mapContentRange(pieceId, 0, this.contentLength(this.pool[pieceId]));
            }

            if (contribution) {
              if (file_id === -1) {
                file_id = contribution.file_id;
              } else if (contribution.file_id !== file_id) {
                this.errorHandler(
                  `Concat pieces span more than one file; no single source range`,
                  id
                );
                return { file_id: -1, range: info.r };
              }
              lo = Math.min(lo, contribution.range[0]);
              hi = Math.max(hi, contribution.range[1]);
            }

            if (start >= pieceEnd) {
              // Remember where the content ran out, so a zero-width query at
              // the very end of the concat still resolves to a position.
              // This piece does not *contribute* to the query — it is
              // entirely before it — so it must not set `file_id`: doing so
              // let a piece with nothing to do with the query pre-empt the
              // file a genuinely contributing piece later resolves to,
              // turning a legitimate single-file query into a spurious
              // "spans more than one file" error. `lastPieceFileId` is kept
              // separate and used only as the last resort below, when no
              // piece ever contributed (the zero-width-at-the-very-end case).
              const tail = lengthPreserving
                ? this.mapContentRange(pieceId, pieceLength, pieceLength)
                : this.mapContentRange(pieceId, 0, this.contentLength(this.pool[pieceId]));
              lastPieceFileId = tail.file_id;
              lastPieceEnd = tail.range[1];
            }
          }

          if (lo === Number.POSITIVE_INFINITY) {
            // No piece overlapped — only reachable for a zero-width query at
            // the concat's end.
            //
            // A Concat whose own content length is 0 hits this every time it
            // is queried as a whole (`start === end === 0`): its one piece
            // never satisfies the zero-width "contains the point" check
            // above (that needs `start < pieceEnd`, and `pieceEnd` is 0 too),
            // so it falls through to the `start >= pieceEnd` branch instead
            // and this is the only contribution. Behavior change from the
            // pre-`mapContentRange` code, recorded rather than fixed: the old
            // code had an explicit `length === 0` branch returning the
            // *first* piece's start; this returns the *last* qualifying
            // piece's end (probed: `[{t:0,r:[0,2],d:0},{t:2,r:[0,0],
            // d:[[0,0,0]]}]` now resolves to `[2,2]`, not the old `[0,0]`).
            // Not reachable from `pampa` today — it checked; empty attribute
            // values arrive as `Original r=[8,8]`, not an empty Concat.
            if (lastPieceEnd !== undefined && lastPieceFileId !== undefined) {
              return { file_id: lastPieceFileId, range: [lastPieceEnd, lastPieceEnd] };
            }
            this.errorHandler(`Concat range [${start}, ${end}] matched no piece`, id);
            return { file_id: -1, range: info.r };
          }

          if (file_id === -1) {
            this.errorHandler(`Could not resolve file_id for Concat`, id);
            return { file_id: -1, range: info.r };
          }
          return { file_id, range: [lo, hi] };
        }

      default:
        this.errorHandler(`Unknown SourceInfo type ${info.t}`, id);
        return { file_id: -1, range: [0, 0] };
    }
  }

  /**
   * Resolve a SourceInfo to the source range that produced its whole content
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

    const resolved = this.mapContentRange(id, 0, this.contentLength(this.pool[id]));

    // Cache and return
    this.resolvedCache.set(id, resolved);
    return resolved;
  }

  // TODO (k-214): Implement circular reference detection
  // This would require tracking visited IDs during resolveChain traversal
}
