/** Map a file path to the Monaco language id used to edit it. */
export function getLanguageForFile(filePath: string): string {
  const ext = filePath.split('.').pop()?.toLowerCase();
  switch (ext) {
    case 'tsx':
    case 'ts':
      return 'typescript';
    case 'jsx':
    case 'js':
      return 'javascript';
    case 'json':
      return 'json';
    case 'html':
      return 'html';
    case 'css':
      return 'css';
    case 'scss':
      return 'scss';
    case 'yaml':
    case 'yml':
      return 'yaml';
    case 'lmd':
      // An lmd module (Lua plus `{= markdown =}` fragments; bd-76ogq4ew)
      // highlights as Lua: Monaco has no lmd grammar, and Lua's gets the
      // code right while leaving the fragments plain.
      return 'lua';
    case 'qmd':
    case 'md':
      // .md is a renderable source file parsed with the qmd grammar
      // (bd-6d2wj4zp Phase 5, D11) — giving it the qmd language wires
      // up the same Monarch highlighting and the qmd-registered
      // symbol/folding/semantic-token providers as .qmd.
      return 'qmd';
    default:
      return 'markdown';
  }
}
