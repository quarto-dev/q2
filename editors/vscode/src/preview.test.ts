import { describe, expect, it, vi } from 'vitest';

vi.mock('vscode', () => ({}));

import { previewHtml, statusText } from './preview';

describe('statusText', () => {
  it('summarises each state', () => {
    expect(statusText({ previewId: 'p', state: 'rendering' }).text).toContain('Rendering');
    expect(statusText({ previewId: 'p', state: 'rendered', ok: true, errors: 0, warnings: 0 }).text).toContain(
      'check',
    );
    expect(statusText({ previewId: 'p', state: 'rendered', ok: true, errors: 0, warnings: 2 }).text).toBe(
      '$(warning) Preview: 2 warnings',
    );
    expect(statusText({ previewId: 'p', state: 'rendered', ok: false, errors: 1 }).text).toBe(
      '$(error) Preview: 1 error',
    );
    expect(statusText({ previewId: 'p', state: 'stopped', error: 'boom' }).tooltip).toBe('boom');
  });
});

describe('previewHtml', () => {
  it('frames only the preview origin and escapes the URL', () => {
    const html = previewHtml('http://127.0.0.1:4321/a.html?x="<y>"', 'N0NCE');
    expect(html).toContain("frame-src http://127.0.0.1:4321;");
    expect(html).toContain("script-src 'nonce-N0NCE'");
    expect(html).not.toContain('"<y>"');
    expect(html).toContain('src="http://127.0.0.1:4321/a.html?x=&quot;&lt;y>&quot;"');
  });
});
