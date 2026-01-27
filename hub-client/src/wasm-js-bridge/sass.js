/**
 * WASM-JS Bridge for SASS Compilation
 *
 * This module provides SASS compilation functions that are called from the Rust
 * WASM code via wasm-bindgen. Uses dart-sass (the reference implementation)
 * for exact parity with TS Quarto.
 *
 * The functions are imported by quarto-system-runtime/src/wasm.rs using:
 *
 *   #[wasm_bindgen(raw_module = "/src/wasm-js-bridge/sass.js")]
 *
 * Key design decisions:
 * - Lazy loading: dart-sass (~5MB) is only loaded when first needed
 * - VFS integration: Custom importer reads from the virtual filesystem
 * - Deprecation warnings suppressed: Same settings as TS Quarto
 */

/** @type {import('sass') | null} */
let sassModule = null;

/** @type {Promise<import('sass')> | null} */
let sassLoadPromise = null;

/**
 * Lazy-load the sass module.
 *
 * The sass package is ~5MB, so we load it lazily to avoid blocking
 * hub-client startup. The first compilation will be slower, but
 * subsequent compilations will be fast.
 *
 * @returns {Promise<import('sass')>}
 */
async function loadSass() {
  if (sassModule) return sassModule;
  if (sassLoadPromise) return sassLoadPromise;

  sassLoadPromise = import("sass").then((module) => {
    sassModule = module.default || module;
    return sassModule;
  });

  return sassLoadPromise;
}

/**
 * Check if SASS compilation is available.
 *
 * This allows the Rust code to check capability before attempting compilation.
 * Always returns true since dart-sass can always be loaded (it's a JS package).
 *
 * @returns {boolean} Always returns true when this module is loaded
 */
export function jsSassAvailable() {
  return true;
}

/**
 * Get the SASS compiler name for diagnostics.
 *
 * @returns {string} The compiler name
 */
export function jsSassCompilerName() {
  return "dart-sass";
}

/**
 * Callback function to read files from the VFS.
 * This is set by the WASM module initialization.
 *
 * @type {((path: string) => string | null) | null}
 */
let vfsReadFile = null;

/**
 * Callback function to check if a path is a file in the VFS.
 *
 * @type {((path: string) => boolean) | null}
 */
let vfsIsFile = null;

/**
 * Callback to list all VFS files.
 * @type {(() => string[]) | null}
 */
let vfsListFiles = null;

/**
 * Set the VFS file reading callback.
 *
 * This must be called by the WASM runtime initialization before
 * any compilation that uses @import or @use.
 *
 * @param {(path: string) => string | null} readFn - Function to read file contents
 * @param {(path: string) => boolean} isFileFn - Function to check if path is a file
 * @param {() => string[]} [listFn] - Optional function to list all files
 */
export function setVfsCallbacks(readFn, isFileFn, listFn) {
  vfsReadFile = readFn;
  vfsIsFile = isFileFn;
  vfsListFiles = listFn || null;
}

/**
 * Create a custom importer for dart-sass that reads from the VFS.
 *
 * This importer handles:
 * - Relative imports from the containing file
 * - Load path resolution
 * - SCSS partial resolution (adding _ prefix and .scss extension)
 *
 * @param {string[]} loadPaths - Directories to search for imports
 * @returns {import('sass').Importer}
 */
function createVfsImporter(loadPaths) {
  return {
    canonicalize(url, context) {
      // Skip non-VFS URLs (http, https, etc.)
      if (url.startsWith("http:") || url.startsWith("https:")) {
        return null;
      }

      // Handle relative imports
      if (context.containingUrl && !url.startsWith("/")) {
        const containingPath = context.containingUrl.pathname;
        const dir = containingPath.substring(0, containingPath.lastIndexOf("/"));
        const resolved = tryResolve(dir + "/" + url);
        if (resolved) {
          return new URL("vfs:" + resolved);
        }
      }

      // Handle absolute VFS paths
      if (url.startsWith("/")) {
        const resolved = tryResolve(url);
        if (resolved) {
          return new URL("vfs:" + resolved);
        }
      }

      // Try each load path
      for (const loadPath of loadPaths) {
        const fullPath = loadPath + "/" + url;
        const resolved = tryResolve(fullPath);
        if (resolved) {
          return new URL("vfs:" + resolved);
        }
      }

      // Not found
      return null;
    },

    load(canonicalUrl) {
      const path = canonicalUrl.pathname;
      const content = vfsReadFile ? vfsReadFile(path) : null;

      if (content === null) {
        return null;
      }

      return {
        contents: content,
        syntax: path.endsWith(".sass") ? "indented" : "scss",
      };
    },
  };
}

/**
 * Try to resolve a path to an actual file in the VFS.
 *
 * Handles SCSS partial resolution:
 * - foo -> _foo.scss, foo.scss, foo/_index.scss, foo/index.scss
 *
 * @param {string} basePath - The path to resolve
 * @returns {string | null} The resolved path or null
 */
function tryResolve(basePath) {
  if (!vfsIsFile) return null;

  // Normalize path (remove double slashes)
  const path = basePath.replace(/\/+/g, "/");

  // If path has extension, try it directly
  if (path.endsWith(".scss") || path.endsWith(".sass") || path.endsWith(".css")) {
    if (vfsIsFile(path)) return path;

    // Try partial (_file.scss)
    const dir = path.substring(0, path.lastIndexOf("/") + 1);
    const file = path.substring(path.lastIndexOf("/") + 1);
    if (!file.startsWith("_")) {
      const partial = dir + "_" + file;
      if (vfsIsFile(partial)) return partial;
    }

    return null;
  }

  // No extension - try common patterns
  const candidates = [
    path + ".scss",
    path + ".sass",
    path + ".css",
    // Partials
    path.replace(/\/([^/]+)$/, "/_$1") + ".scss",
    path.replace(/\/([^/]+)$/, "/_$1") + ".sass",
    // Index files
    path + "/_index.scss",
    path + "/index.scss",
    path + "/_index.sass",
    path + "/index.sass",
  ];

  // Also handle case where path doesn't start with /
  const pathWithSlash = path.startsWith("/") ? path : "/" + path;
  if (pathWithSlash !== path) {
    candidates.push(
      pathWithSlash + ".scss",
      pathWithSlash + ".sass",
      pathWithSlash + ".css",
      pathWithSlash.replace(/\/([^/]+)$/, "/_$1") + ".scss",
      pathWithSlash.replace(/\/([^/]+)$/, "/_$1") + ".sass"
    );
  }

  for (const candidate of candidates) {
    if (vfsIsFile(candidate)) return candidate;
  }

  return null;
}

/**
 * Compile SCSS to CSS.
 *
 * @param {string} scss - The SCSS source code
 * @param {string} style - Output style: "expanded" or "compressed"
 * @param {string} loadPathsJson - JSON-encoded array of load paths
 * @returns {Promise<string>} Compiled CSS
 *
 * @example
 * const css = await jsCompileSass(
 *   "$primary: blue; .btn { color: $primary; }",
 *   "expanded",
 *   "[]"
 * );
 */
export async function jsCompileSass(scss, style, loadPathsJson) {
  const sass = await loadSass();
  const loadPaths = JSON.parse(loadPathsJson);

  // Options to match TS Quarto's dart-sass invocation
  const options = {
    style: style === "compressed" ? "compressed" : "expanded",
    // Suppress deprecation warnings (same as TS Quarto)
    // These warnings are for features used in Bootstrap 5.3.1
    silenceDeprecations: ["global-builtin", "color-functions", "import"],
    // Suppress regular warnings too
    logger: {
      warn: () => {},
      debug: () => {},
    },
  };

  // Add custom importer if VFS callbacks are set
  if (vfsReadFile && vfsIsFile && loadPaths.length > 0) {
    options.importers = [createVfsImporter(loadPaths)];
  }

  try {
    const result = sass.compileString(scss, options);
    return result.css;
  } catch (error) {
    // Re-throw with a cleaner error message
    const message = error instanceof Error ? error.message : String(error);
    throw new Error(`SASS compilation failed: ${message}`);
  }
}
