/**
 * Strip ANSI escape codes from text.
 * Used to clean up error messages that may contain terminal color codes.
 */
export function stripAnsi(text: string): string {
  // Match ANSI escape sequences: ESC [ ... m (SGR sequences)
  // This covers color codes like \x1b[31m, \x1b[38;5;246m, \x1b[0m, etc.
  // eslint-disable-next-line no-control-regex
  return text.replace(/\x1b\[[0-9;]*m/g, '');
}
