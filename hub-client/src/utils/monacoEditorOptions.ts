/** Monaco options shared by every hub-client editor surface (Editor, demos). */

// Stable options object — defined at module level so @monaco-editor/react
// never sees a new reference and skips its internal updateOptions() effect.
export const QMD_EDITOR_OPTIONS = {
  minimap: { enabled: false },
  fontSize: 14,
  lineNumbers: 'on' as const,
  // Size the number column near the document's actual digit count
  // instead of Monaco's default 5-char reserve; the extra char over the
  // digits acts as left padding (numbers are right-aligned).
  lineNumbersMinChars: 4,
  wordWrap: 'on' as const,
  padding: { top: 16 },
  scrollBeyondLastLine: false,
  // Disable paste-as to prevent snippet expansion (e.g., URLs from browser
  // address bar being pasted with $0 appended). See quarto-dev/kyoto#3.
  pasteAs: { enabled: false },
  fixedOverflowWidgets: true,
  // Prefer showing hover below the line — prevents diagnostic popups near
  // the top of the editor from overlapping the navbar.
  hover: { above: false },
  quickSuggestions: false,
  suggestOnTriggerCharacters: false,
  wordBasedSuggestions: 'off' as const,
  acceptSuggestionOnEnter: 'off' as const,
  acceptSuggestionOnCommitCharacter: false,
  suggest: { showWords: false, showSnippets: false },
  inlineSuggest: { enabled: false },
  // Enable the semantic-tokens layer (the quarto-{light,dark} themes also set
  // `semanticHighlighting: true`; setting both is the reliable combination).
  'semanticHighlighting.enabled': true as const,
};
