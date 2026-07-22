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
