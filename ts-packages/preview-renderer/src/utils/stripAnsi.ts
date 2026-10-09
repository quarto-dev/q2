/**
 * Strip ANSI escape codes from text: SGR color sequences and OSC 8
 * hyperlinks (`ESC ] 8 ; params ; uri ST`, terminated by ST = `ESC \` or BEL).
 * Used to clean up error messages that may contain terminal escapes.
 */
export function stripAnsi(text: string): string {
  // Match ANSI escape sequences: ESC [ ... m (SGR sequences)
  // This covers color codes like \x1b[31m, \x1b[38;5;246m, \x1b[0m, etc.
  // eslint-disable-next-line no-control-regex
  // OSC 8 hyperlinks: ariadne wraps `file:line:col` in them; drop the
  // markers and keep the visible link text.
  return text
    .replace(/\x1b\[[0-9;]*m/g, '')
    .replace(/\x1b\]8;[^\x07\x1b]*;[^\x07\x1b]*(?:\x07|\x1b\\)/g, '');
}
