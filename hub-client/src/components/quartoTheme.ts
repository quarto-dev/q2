/**
 * Quarto editor language + theme for Monaco.
 *
 * Registers the dedicated `'qmd'` language (a markdown-derived Monarch base for
 * instant painting + `nextEmbedded` routing of code cells / frontmatter to
 * Monaco's stock tokenizers — Hybrid-A, tier 1) and the `quarto-light` /
 * `quarto-dark` themes whose rules colour the semantic-token legend.
 *
 * The semantic-tokens provider (monacoProviders.ts) is the authoritative colour
 * source; the Monarch base paints instantly while the async round-trip is in
 * flight and fills any byte semantic leaves uncaptured.
 *
 * Theme-scoping safety: **every** rule token carries the `qmd.` sentinel
 * super-prefix, so no rule can prefix-match a scope another language emits (a
 * global Monaco theme applies to every model). The `quartoTheme.test.ts`
 * namespace-invariant test enforces this.
 */

import type * as Monaco from 'monaco-editor';

export const QMD_LANGUAGE_ID = 'qmd';

// Solarized palette mirrored from resources/scss/html/templates/highlight.scss
// so editor code-cell colours match the rendered HTML (code-cell parity).
const KEYWORD = '859900'; // green
const STRING = '2aa198'; // cyan
const ESCAPE = 'b58900'; // yellow
const NUMBER = 'd33682'; // magenta
const COMMENT = '93a1a1'; // base1
const FUNCTION = '268bd2'; // blue
const TYPE = 'b58900'; // yellow
const VARIABLE = '657b83'; // base00
const PROPERTY = '6c71c4'; // violet
const PUNCT = '657b83'; // base00
const TAG = 'dc322f'; // red
const SPECIAL = 'cb4b16'; // orange
const ERROR = 'dc322f';

// Structural accents (editor-only; no rendered counterpart).
const LINK_LABEL = '56b6c2';
const LINK_URL = '4a90e2';
const IMAGE_ACCENT = 'a0522d'; // brown — distinct from the cyan/blue link palette
const BRACKET = '5c6370'; // unobtrusive grey

/**
 * The theme rules for `quarto-light` / `quarto-dark`, exported so the
 * namespace-invariant test can assert every token starts with `qmd.`. Both
 * themes share these foregrounds (the solarized palette reads on light and
 * dark); tier-2 per-theme tuning is deferred (Phase 6).
 */
export const quartoThemeRules: Monaco.editor.ITokenThemeRule[] = [
  // --- structural ---
  { token: 'qmd.markup.heading', foreground: FUNCTION, fontStyle: 'bold' },
  { token: 'qmd.markup.emphasis', fontStyle: 'italic' },
  { token: 'qmd.markup.strong', fontStyle: 'bold' },
  { token: 'qmd.markup.strikethrough', foreground: COMMENT },
  { token: 'qmd.markup.link.label', foreground: LINK_LABEL },
  { token: 'qmd.markup.link.url', foreground: LINK_URL },
  { token: 'qmd.markup.link.title', foreground: STRING },
  { token: 'qmd.markup.image.label', foreground: LINK_LABEL },
  { token: 'qmd.markup.image.url', foreground: LINK_URL },
  { token: 'qmd.markup.raw.inline', foreground: STRING },
  { token: 'qmd.markup.raw', foreground: STRING },
  { token: 'qmd.markup.raw.info', foreground: KEYWORD },
  { token: 'qmd.markup.math', foreground: NUMBER },
  { token: 'qmd.markup.shortcode', foreground: FUNCTION },
  { token: 'qmd.markup.list', foreground: PUNCT },
  { token: 'qmd.markup.quote', foreground: COMMENT, fontStyle: 'italic' },
  { token: 'qmd.markup.comment', foreground: COMMENT, fontStyle: 'italic' },
  { token: 'qmd.punctuation.special', foreground: PUNCT },
  { token: 'qmd.punctuation.special.image', foreground: IMAGE_ACCENT },
  { token: 'qmd.punctuation.bracket', foreground: BRACKET },
  { token: 'qmd.punctuation.delimiter.fence', foreground: KEYWORD },
  { token: 'qmd.punctuation.delimiter.frontmatter', foreground: KEYWORD },
  { token: 'qmd.attribute.specifier', foreground: FUNCTION, fontStyle: 'italic' },
  // --- embedded code (mirror the hl-<root> colours for render parity) ---
  { token: 'qmd.code.attribute', foreground: PROPERTY },
  { token: 'qmd.code.boolean', foreground: NUMBER },
  { token: 'qmd.code.character', foreground: STRING },
  { token: 'qmd.code.comment', foreground: COMMENT, fontStyle: 'italic' },
  { token: 'qmd.code.constant', foreground: NUMBER },
  { token: 'qmd.code.constructor', foreground: FUNCTION },
  { token: 'qmd.code.embedded', foreground: SPECIAL },
  { token: 'qmd.code.error', foreground: ERROR },
  { token: 'qmd.code.escape', foreground: ESCAPE },
  { token: 'qmd.code.function', foreground: FUNCTION },
  { token: 'qmd.code.keyword', foreground: KEYWORD, fontStyle: 'bold' },
  { token: 'qmd.code.label', foreground: PROPERTY },
  { token: 'qmd.code.markup', foreground: FUNCTION },
  { token: 'qmd.code.module', foreground: TYPE },
  { token: 'qmd.code.namespace', foreground: TYPE },
  { token: 'qmd.code.number', foreground: NUMBER },
  { token: 'qmd.code.operator', foreground: PUNCT },
  { token: 'qmd.code.property', foreground: PROPERTY },
  { token: 'qmd.code.punctuation', foreground: PUNCT },
  { token: 'qmd.code.special', foreground: SPECIAL },
  { token: 'qmd.code.string', foreground: STRING },
  { token: 'qmd.code.tag', foreground: TAG },
  { token: 'qmd.code.type', foreground: TYPE },
  { token: 'qmd.code.variable', foreground: VARIABLE },
];

/**
 * Markdown-derived Monarch base. Non-authoritative: the semantic provider
 * overrides it where present. Tier 1 routes embedded regions (Quarto `{r}` /
 * `{python}` brace cells *and* plain ` ```python ` cells, plus `---…---`
 * frontmatter) to Monaco's stock tokenizers via `nextEmbedded` — Monaco's
 * stock markdown regex does NOT match the brace syntax, which is GH#10 bug #3.
 */
export const qmdMonarch: Monaco.languages.IMonarchLanguage = {
  defaultToken: '',
  tokenPostfix: '.qmd',
  tokenizer: {
    root: [
      // YAML frontmatter only at the very top of the document.
      [/^---\s*$/, { token: 'keyword', next: '@frontmatter', nextEmbedded: 'yaml' }],
      { include: '@content' },
    ],
    frontmatter: [
      [/^(?:---|\.\.\.)\s*$/, { token: 'keyword', next: '@pop', nextEmbedded: '@pop' }],
    ],
    content: [
      // Quarto executable cell: ```{r}, ```{python}, ... — route by the inner
      // language name (capture group 2).
      [
        /^(\s*`{3,})(?:\{)([\w-]+)([^}]*\}.*)$/,
        ['string', { token: 'attribute.name', nextEmbedded: '$2', next: '@codeblock' }, 'attribute.value'],
      ],
      // Plain fenced cell: ```python, ```sql, ...
      [
        /^(\s*`{3,})([\w-]+)(.*)$/,
        ['string', { token: 'type', nextEmbedded: '$2', next: '@codeblock' }, 'string'],
      ],
      // Bare fence with no info string.
      [/^(\s*`{3,})\s*$/, { token: 'string', next: '@codeblockPlain' }],
      // Headings, emphasis, inline code — basic structure so the base is not
      // plain text before semantic settles. Link/image brackets are deliberately
      // NOT coloured here: the semantic layer leaves `[`/`]` at the default
      // foreground (only the image `![` opener is special), so a base rule that
      // coloured the opening `[` (and not the closing `]`) made the brackets
      // mismatch wherever the base shows through.
      [/^#{1,6}\s.*$/, 'keyword'],
      [/\*\*([^\\*]|\*(?!\*))+\*\*/, 'strong'],
      [/__([^\\_]|_(?!_))+__/, 'strong'],
      [/\*([^\\*]|\*\*)+\*/, 'emphasis'],
      [/~~[^~]+~~/, 'emphasis'],
      [/`[^`]+`/, 'string'],
    ],
    codeblock: [
      // Closing fence ends the embedded region.
      [/^\s*`{3,}\s*$/, { token: 'string', next: '@pop', nextEmbedded: '@pop' }],
    ],
    codeblockPlain: [
      [/^\s*`{3,}\s*$/, { token: 'string', next: '@pop' }],
      [/.*$/, ''],
    ],
  },
};

const qmdLanguageConfiguration: Monaco.languages.LanguageConfiguration = {
  comments: { blockComment: ['<!--', '-->'] },
  brackets: [
    ['[', ']'],
    ['(', ')'],
    ['{', '}'],
  ],
  autoClosingPairs: [
    { open: '[', close: ']' },
    { open: '(', close: ')' },
    { open: '{', close: '}' },
    { open: '`', close: '`' },
    { open: '"', close: '"' },
    { open: '$', close: '$' },
  ],
  surroundingPairs: [
    { open: '[', close: ']' },
    { open: '(', close: ')' },
    { open: '`', close: '`' },
    { open: '*', close: '*' },
    { open: '_', close: '_' },
  ],
};

/**
 * Register the `'qmd'` language, its Monarch base + configuration, and the
 * `quarto-light` / `quarto-dark` themes. Idempotent-safe to call once per
 * editor mount (Monaco dedupes language ids). Call from `beforeMount`.
 */
export function registerQmdLanguage(monaco: typeof Monaco): void {
  const alreadyRegistered = monaco.languages
    .getLanguages()
    .some((l) => l.id === QMD_LANGUAGE_ID);
  if (!alreadyRegistered) {
    monaco.languages.register({
      id: QMD_LANGUAGE_ID,
      extensions: ['.qmd'],
      aliases: ['Quarto', 'Quarto Markdown', 'qmd'],
    });
  }

  monaco.languages.setLanguageConfiguration(QMD_LANGUAGE_ID, qmdLanguageConfiguration);
  monaco.languages.setMonarchTokensProvider(QMD_LANGUAGE_ID, qmdMonarch);

  // `semanticHighlighting: true` is what makes Monaco override the Monarch base
  // with the semantic-tokens provider's tokens. The field is a real runtime
  // theme-data flag but is missing from the installed monaco typings, so widen
  // the type to set it without an error.
  const themeData = (
    base: Monaco.editor.BuiltinTheme,
  ): Monaco.editor.IStandaloneThemeData & { semanticHighlighting?: boolean } => ({
    base,
    inherit: true,
    semanticHighlighting: true,
    rules: quartoThemeRules,
    colors: {},
  });
  monaco.editor.defineTheme('quarto-light', themeData('vs'));
  monaco.editor.defineTheme('quarto-dark', themeData('vs-dark'));
}
