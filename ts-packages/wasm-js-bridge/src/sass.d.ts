/**
 * Type declarations for the SASS bridge module.
 */

/**
 * Check if SASS compilation is available.
 */
export function jsSassAvailable(): boolean;

/**
 * Get the SASS compiler name.
 */
export function jsSassCompilerName(): string;

/**
 * Set VFS callbacks for the SASS importer.
 *
 * @param readFn - Function to read file contents from VFS
 * @param isFileFn - Function to check if path is a file in VFS
 * @param listFn - Optional function to list all files in VFS
 */
export function setVfsCallbacks(
  readFn: (path: string) => string | null,
  isFileFn: (path: string) => boolean,
  listFn?: () => string[]
): void;

/**
 * What a compile produced: the CSS and the VFS paths (`/project/…`)
 * dart-sass loaded for it through the VFS importer, each once, in load
 * order. Consumed by the Rust sass cache to validate imported partials.
 */
export interface SassCompileOutput {
  css: string;
  loadedUrls: string[];
}

/**
 * Compile SCSS to CSS.
 *
 * @param scss - The SCSS source code
 * @param style - Output style: "expanded" or "compressed"
 * @param loadPathsJson - JSON-encoded array of load paths
 * @returns The compiled CSS and the VFS files loaded for it
 */
export function jsCompileSass(
  scss: string,
  style: string,
  loadPathsJson: string
): Promise<SassCompileOutput>;
