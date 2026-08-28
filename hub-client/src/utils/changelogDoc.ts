/**
 * Theming for the About tab's markdown documents (changelog, more-info).
 *
 * The documents are rendered to a full HTML page by the WASM pipeline and
 * shown in a sandboxed iframe. That iframe document is a separate browsing
 * context: it sees none of the app's theme classes or CSS variables, and
 * its canvas lets the modal's --bg-modal show through. Styles must
 * therefore be injected into the rendered HTML, and they must match the
 * app's effective theme — hardcoded light colors rendered the changelog
 * near-invisible (1.3:1 contrast) on the dark modal (GH #624).
 */

export type ChangelogTheme = 'light' | 'dark';

/**
 * Palette values mirror theme.css tokens, duplicated here because the
 * iframe document cannot read the app's CSS variables. Keep in sync:
 *
 * | token (theme.css)                    | light     | dark      |
 * | ------------------------------------ | --------- | --------- |
 * | --bg-modal                           | #ffffff   | #213D4F   |
 * | text (~ --text-primary / page text)  | #333333   | #ffffff   |
 * | links (~ --accent-secondary)         | #447099   | #A2B8CB   |
 * | link hover (--link-hover)            | #305775   | #D1DBE5   |
 * | code background (~ --bg-input)       | #f4f4f4   | #17212B   |
 * | code border (--border-color)         | —         | #305775   |
 *
 * Every text/background pair above meets WCAG AA (4.5:1);
 * changelogDoc.test.ts pins the contrasts.
 */
const CHANGELOG_COLORS: Record<
  ChangelogTheme,
  {
    background: string;
    text: string;
    heading: string;
    link: string;
    linkHover: string;
    codeBackground: string;
    codeBorder: string;
  }
> = {
  light: {
    background: '#ffffff',
    text: '#333333',
    heading: '#111111',
    link: '#447099',
    linkHover: '#305775',
    codeBackground: '#f4f4f4',
    codeBorder: 'transparent',
  },
  dark: {
    background: '#213D4F',
    text: '#ffffff',
    heading: '#ffffff',
    link: '#A2B8CB',
    linkHover: '#D1DBE5',
    codeBackground: '#17212B',
    codeBorder: '#305775',
  },
};

/**
 * Minimal stylesheet for the rendered markdown document, themed to match
 * the app chrome around the modal. `color-scheme` is declared so UA
 * painting (scrollbars, form controls) follows the same theme.
 */
export function changelogStylesForTheme(theme: ChangelogTheme): string {
  const c = CHANGELOG_COLORS[theme];
  return `
  :root {
    color-scheme: ${theme};
  }
  body {
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Oxygen, Ubuntu, sans-serif;
    font-size: 14px;
    line-height: 1.6;
    color: ${c.text};
    background: ${c.background};
    padding: 24px;
    margin: 0;
    max-width: 800px;
  }
  h2 {
    font-size: 20px;
    font-weight: 600;
    margin: 0 0 16px 0;
    color: ${c.heading};
  }
  ul {
    margin: 0;
    padding: 0 0 0 20px;
  }
  li {
    margin: 8px 0;
  }
  a {
    color: ${c.link};
    text-decoration: none;
  }
  a:hover {
    color: ${c.linkHover};
    text-decoration: underline;
  }
  code {
    font-family: 'SF Mono', Monaco, 'Cascadia Code', monospace;
    font-size: 13px;
    background: ${c.codeBackground};
    border: 1px solid ${c.codeBorder};
    padding: 2px 6px;
    border-radius: 3px;
  }
`;
}

/**
 * Inject the themed stylesheet into a rendered HTML document (before
 * `</head>`, so it wins over the UA defaults and needs no network).
 */
export function injectChangelogStyles(html: string, theme: ChangelogTheme): string {
  return html.replace('</head>', `<style>${changelogStylesForTheme(theme)}</style></head>`);
}
