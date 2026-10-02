import { describe, expect, it } from 'vitest';
import { viewerUrl } from './pdfViewer';

describe('viewerUrl', () => {
  it('addresses the stock viewer under the base path', () => {
    expect(viewerUrl('/')).toBe('/pdfjs/web/viewer.html');
    expect(viewerUrl('/hub')).toBe('/hub/pdfjs/web/viewer.html');
  });
  it('names the file and hides the sidebar', () => {
    expect(viewerUrl('/', 'blob:http://x/1')).toBe('/pdfjs/web/viewer.html?file=blob%3Ahttp%3A%2F%2Fx%2F1#pagemode=none');
  });
});
