// parity: vendored from external-sources/quarto-cli/packages/quarto-types
/**
 * External engine interfaces for Quarto
 */

/**
 * Represents an external engine specified in a project
 */
export interface ExternalEngine {
  /**
   * Path to the engine implementation
   */
  path: string;
}
