// parity: vendored from external-sources/quarto-cli/packages/quarto-types
/**
 * Core text manipulation types for Quarto
 */

/**
 * Represents a range within a string
 */
export interface Range {
  start: number;
  end: number;
}

/**
 * A string with source mapping information
 */
export interface MappedString {
  /**
   * The text content
   */
  readonly value: string;

  /**
   * Optional filename where the content originated
   */
  readonly fileName?: string;

  /**
   * Maps positions in this string back to positions in the original source
   * @param index Position in the current string
   * @param closest Whether to find the closest mapping if exact is not available
   */
  readonly map: (index: number, closest?: boolean) => StringMapResult;

  /**
   * Flattened provenance: one entry per leaf-backed segment, in output order,
   * covering [0, value.length). Optional — consumers that don't need provenance
   * (engines) ignore it; `undefined` ⇒ provenance NOT PROVIDED (opaque), which the
   * serializer encodes as an empty wire map. `source: null` marks a segment with no
   * original file (KNOWN-synthetic / inserted text) — distinct from `undefined`.
   */
  readonly segments?: () => ReadonlyArray<{
    start: number;            // offset of this segment in `value`
    length: number;
    source: { file: string; fileOffset: number } | null;
  }>;
}

/**
 * Result of mapping a position in a mapped string
 */
export type StringMapResult = {
  /**
   * Position in the original source
   */
  index: number;

  /**
   * Reference to the original mapped string
   */
  originalString: MappedString;
} | undefined;

/**
 * String that may be mapped or unmapped
 */
export type EitherString = string | MappedString;

/**
 * A chunk that can be a plain string, a MappedString, or a Range within a source string
 */
export type StringChunk = string | MappedString | Range;
