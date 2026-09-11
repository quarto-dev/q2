/**
 * A person shown in a facepile — a colored initials disk.
 *
 * Populated from real identities: the current user (from user settings) and
 * the contributors cached on a project's summary (which come from the index
 * document's `identities` map as people open/edit projects).
 */
export interface Face {
  name: string;
  initials: string;
  /** Hex color from the palette, e.g. "#E91E63". */
  color: string;
}

/**
 * Initials for a facepile disk: first two letters of a single-word name,
 * first + last initials otherwise.
 */
export function initialsFor(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  if (parts.length === 0) return '?';
  if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase();
  return (parts[0][0] + parts[parts.length - 1][0]).toUpperCase();
}
