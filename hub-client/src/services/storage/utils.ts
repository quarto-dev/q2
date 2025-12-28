/**
 * Utility functions for storage and user identity.
 */

/**
 * Generate a consistent color from a user ID using a hash function.
 * The color is deterministic - the same userId always produces the same color.
 *
 * Uses a curated palette of distinct, visually pleasing colors that work
 * well for cursor/selection highlighting on both light and dark backgrounds.
 */
export function generateColorFromId(userId: string): string {
  // Curated palette of distinct colors for collaborative cursors
  // These are chosen to be:
  // - Visually distinct from each other
  // - Readable on both light and dark editor backgrounds
  // - Not too similar to selection highlight or syntax highlighting colors
  const palette = [
    '#E91E63', // Pink
    '#9C27B0', // Purple
    '#673AB7', // Deep Purple
    '#3F51B5', // Indigo
    '#2196F3', // Blue
    '#00BCD4', // Cyan
    '#009688', // Teal
    '#4CAF50', // Green
    '#8BC34A', // Light Green
    '#FF9800', // Orange
    '#FF5722', // Deep Orange
    '#795548', // Brown
    '#607D8B', // Blue Grey
    '#F44336', // Red
    '#00ACC1', // Cyan 600
    '#7B1FA2', // Purple 700
  ];

  // Simple hash function to convert userId to a number
  let hash = 0;
  for (let i = 0; i < userId.length; i++) {
    const char = userId.charCodeAt(i);
    hash = ((hash << 5) - hash) + char;
    hash = hash & hash; // Convert to 32-bit integer
  }

  // Use absolute value and modulo to pick a color from the palette
  const index = Math.abs(hash) % palette.length;
  return palette[index];
}

/**
 * Generate a random anonymous display name.
 * Format: "Anonymous [Adjective] [Animal]"
 *
 * This provides friendly, memorable names for users who haven't set a custom name.
 */
export function generateAnonymousName(): string {
  const adjectives = [
    'Swift',
    'Clever',
    'Bright',
    'Calm',
    'Bold',
    'Keen',
    'Quick',
    'Wise',
    'Kind',
    'Brave',
    'Gentle',
    'Noble',
    'Witty',
    'Merry',
    'Jolly',
    'Eager',
  ];

  const animals = [
    'Penguin',
    'Otter',
    'Fox',
    'Owl',
    'Dolphin',
    'Rabbit',
    'Koala',
    'Panda',
    'Falcon',
    'Lynx',
    'Wolf',
    'Bear',
    'Hawk',
    'Seal',
    'Deer',
    'Crane',
  ];

  const adjective = adjectives[Math.floor(Math.random() * adjectives.length)];
  const animal = animals[Math.floor(Math.random() * animals.length)];

  return `${adjective} ${animal}`;
}

/**
 * Validate that a color string is a valid hex color.
 */
export function isValidHexColor(color: string): boolean {
  return /^#[0-9A-Fa-f]{6}$/.test(color);
}

/**
 * Validate that a user name is acceptable.
 * - Not empty after trimming
 * - Not too long (max 50 characters)
 * - Contains at least one non-whitespace character
 */
export function isValidUserName(name: string): boolean {
  const trimmed = name.trim();
  return trimmed.length > 0 && trimmed.length <= 50;
}
