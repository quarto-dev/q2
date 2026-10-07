export interface Limits {
  /** Total bytes of mapped files (the `.typ`, images, resource files). */
  total_bytes: number;
  /** Packages the retry loop may fetch beyond the ones named in the sources. */
  max_packages: number;
  /** Total tarball bytes fetched for one compile. */
  package_bytes: number;
  /** Wall time for the package prefetch and retry loop. */
  package_ms: number;
  /** Compile attempts (each retry follows one more package). */
  max_attempts: number;
}

export const DEFAULT_LIMITS: Limits = {
  total_bytes: 300 * 1024 * 1024,
  max_packages: 16,
  package_bytes: 64 * 1024 * 1024,
  package_ms: 30_000,
  max_attempts: 8,
};

/** Where `@preview` tarballs live (CORS verified 2026-10-01: `access-control-allow-origin: *`). */
export const PACKAGE_REGISTRY = 'https://packages.typst.org';
