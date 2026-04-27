/**
 * Validate a project-relative path for upload/asset creation.
 *
 * Returns an error message string if invalid, or `null` if valid.
 *
 * Rules:
 * - Empty string (`""`) is valid and means "project root".
 * - No leading `/` (project paths are always relative to project root).
 * - No `.` or `..` segments (prevent path traversal and normalize noise).
 * - No empty segments (`foo//bar`, trailing `/`).
 * - No forbidden characters in any segment: `<>:"|?*\`.
 */

const FORBIDDEN_CHARS = /[<>:"|?*\\]/;

export function validateProjectPath(path: string): string | null {
  if (path === '') {
    return null;
  }

  if (path.startsWith('/')) {
    return 'Path must not start with a leading slash';
  }

  const segments = path.split('/');

  for (const segment of segments) {
    if (segment === '') {
      return 'Path contains an empty segment (double slash or trailing slash)';
    }
    if (segment === '.' || segment === '..') {
      return 'Path must not contain "." or ".." segments';
    }
    if (FORBIDDEN_CHARS.test(segment)) {
      return 'Path contains invalid characters';
    }
  }

  return null;
}
