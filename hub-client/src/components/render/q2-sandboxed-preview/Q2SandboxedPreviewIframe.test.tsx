/**
 * @vitest-environment jsdom
 */
import { render, screen } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { Q2SandboxedPreviewIframe } from './Q2SandboxedPreviewIframe';

describe('Q2SandboxedPreviewIframe', () => {
  it('pins a light canvas behind the transparent sandboxed document', () => {
    // The sandboxed document paints no background; the editor's preview pane
    // follows the chrome theme (dark in dark mode). The iframe's own
    // background must stay light or the document's default dark text becomes
    // unreadable in dark mode.
    render(<Q2SandboxedPreviewIframe astJson="{}" />);
    const iframe = screen.getByTitle('q2-sandboxed-preview Renderer');
    expect(iframe.style.background).toBe('rgb(255, 255, 255)');
  });
});
