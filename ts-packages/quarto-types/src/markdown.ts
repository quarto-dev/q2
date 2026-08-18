// parity: vendored from external-sources/quarto-cli/packages/quarto-types
/**
 * Markdown structure types for Quarto
 */

import type { MappedString } from "./text.js";
import type { Metadata } from "./metadata.js";

/**
 * Cell type from breaking Quarto markdown
 */
export interface QuartoMdCell {
  id?: string;
  cell_type: { language: string } | "markdown" | "raw";
  options?: Record<string, unknown>;
  source: MappedString;
  sourceVerbatim: MappedString;
  sourceWithYaml?: MappedString;
  sourceOffset: number;
  sourceStartLine: number;
  cellStartLine: number;
}

/**
 * Result from breaking Quarto markdown
 */
export interface QuartoMdChunks {
  cells: QuartoMdCell[];
}

/**
 * A partitioned markdown document
 */
export interface PartitionedMarkdown {
  /** YAML frontmatter as parsed metadata */
  yaml?: Metadata;

  /** Text of the first heading */
  headingText?: string;

  /** Attributes of the first heading */
  headingAttr?: {
    id: string;
    classes: string[];
    keyvalue: Array<[string, string]>;
  };

  /** Whether the document contains references */
  containsRefs: boolean;

  /** Complete markdown content */
  markdown: string;

  /** Markdown without YAML frontmatter */
  srcMarkdownNoYaml: string;
}
