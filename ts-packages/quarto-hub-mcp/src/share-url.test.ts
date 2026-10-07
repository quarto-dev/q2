import { describe, it, expect } from 'vitest';
import { buildShareUrl, DEFAULT_SERVER_URL, parseProjectRef, serversMatch } from './share-url.js';

// The canonical share link quarto-hub.com hands users. Note that file/server/
// name live inside the URL *fragment* (after `#`), which `new URL().searchParams`
// does not see — the whole point of parsing it by hand.
const SHARE_URL =
  'https://quarto-hub.com/#/share/3fA4nXRpYK1JPkeyef3KXFMEs4aN' +
  '?server=wss%3A%2F%2Fquarto-hub.com%2Fws&file=_brand.yml&name=A+%60quarto-hub%60+update';

describe('parseProjectRef', () => {
  it('returns a bare index doc id unchanged', () => {
    expect(parseProjectRef('3fA4nXRpYK1JPkeyef3KXFMEs4aN')).toEqual({
      project: '3fA4nXRpYK1JPkeyef3KXFMEs4aN',
    });
  });

  it('trims surrounding whitespace from a bare id', () => {
    expect(parseProjectRef('  3fA4nXRpYK1JPkeyef3KXFMEs4aN \n')).toEqual({
      project: '3fA4nXRpYK1JPkeyef3KXFMEs4aN',
    });
  });

  it('extracts project, file, server, and name from a full share URL', () => {
    expect(parseProjectRef(SHARE_URL)).toEqual({
      project: '3fA4nXRpYK1JPkeyef3KXFMEs4aN',
      file: '_brand.yml',
      server: 'wss://quarto-hub.com/ws',
      name: 'A `quarto-hub` update',
    });
  });

  it('decodes percent-encoding and + in the name', () => {
    // %60 -> backtick, + -> space
    expect(parseProjectRef(SHARE_URL).name).toBe('A `quarto-hub` update');
  });

  it('handles a share URL with no query params', () => {
    expect(
      parseProjectRef('https://quarto-hub.com/#/share/3fA4nXRpYK1JPkeyef3KXFMEs4aN'),
    ).toEqual({ project: '3fA4nXRpYK1JPkeyef3KXFMEs4aN' });
  });

  it('handles a bare fragment without the scheme/host', () => {
    expect(
      parseProjectRef('/share/3fA4nXRpYK1JPkeyef3KXFMEs4aN?file=slides.qmd'),
    ).toEqual({ project: '3fA4nXRpYK1JPkeyef3KXFMEs4aN', file: 'slides.qmd' });
  });

  it('omits file/server/name keys when those params are absent', () => {
    const ref = parseProjectRef(
      'https://quarto-hub.com/#/share/3fA4nXRpYK1JPkeyef3KXFMEs4aN?name=Untitled',
    );
    expect(ref).toEqual({ project: '3fA4nXRpYK1JPkeyef3KXFMEs4aN', name: 'Untitled' });
    expect('file' in ref).toBe(false);
    expect('server' in ref).toBe(false);
  });
});

describe('buildShareUrl', () => {
  // hub-client's share route treats server, file, and name as all required
  // and rejects the link as "incomplete" when any is missing (bd-jtl4o0pt)
  // — so buildShareUrl must emit server= even for the production hub.
  it('includes server=, file=, and name= even for the production hub', () => {
    const url = buildShareUrl({
      server: DEFAULT_SERVER_URL,
      indexDocId: '3fA4nXRpYK1JPkeyef3KXFMEs4aN',
      file: 'index.qmd',
      name: 'My book',
    });
    expect(url).toBe(
      'https://quarto-hub.com/#/share/3fA4nXRpYK1JPkeyef3KXFMEs4aN' +
        '?server=wss%3A%2F%2Fquarto-hub.com%2Fws&file=index.qmd&name=My+book',
    );
    expect(parseProjectRef(url)).toEqual({
      project: '3fA4nXRpYK1JPkeyef3KXFMEs4aN',
      server: DEFAULT_SERVER_URL,
      file: 'index.qmd',
      name: 'My book',
    });
  });

  it('encodes a slashy file path and a non-default hub, and round-trips', () => {
    const url = buildShareUrl({
      server: 'ws://127.0.0.1:4321/ws',
      indexDocId: 'abc123',
      file: 'docs/intro.qmd',
      name: 'A `quarto-hub` update',
    });
    expect(parseProjectRef(url)).toEqual({
      project: 'abc123',
      server: 'ws://127.0.0.1:4321/ws',
      file: 'docs/intro.qmd',
      name: 'A `quarto-hub` update',
    });
  });
});

describe('serversMatch', () => {
  it('matches identical URLs', () => {
    expect(serversMatch('wss://quarto-hub.com/ws', 'wss://quarto-hub.com/ws')).toBe(true);
  });

  it('ignores a trailing slash difference', () => {
    expect(serversMatch('wss://quarto-hub.com/ws', 'wss://quarto-hub.com/ws/')).toBe(true);
  });

  it('ignores host case', () => {
    expect(serversMatch('wss://Quarto-Hub.com/ws', 'wss://quarto-hub.com/ws')).toBe(true);
  });

  it('ignores surrounding whitespace', () => {
    expect(serversMatch('  wss://quarto-hub.com/ws ', 'wss://quarto-hub.com/ws')).toBe(true);
  });

  it('treats ws and wss as different (scheme matters)', () => {
    expect(serversMatch('ws://quarto-hub.com/ws', 'wss://quarto-hub.com/ws')).toBe(false);
  });

  it('treats different hosts as different', () => {
    expect(serversMatch('wss://sync.automerge.org', 'wss://quarto-hub.com/ws')).toBe(false);
  });

  it('treats different paths as different', () => {
    expect(serversMatch('wss://quarto-hub.com/ws', 'wss://quarto-hub.com/other')).toBe(false);
  });

  it('falls back to a trimmed string compare for unparseable input', () => {
    expect(serversMatch('not a url', 'not a url')).toBe(true);
    expect(serversMatch('not a url', 'other')).toBe(false);
  });
});
