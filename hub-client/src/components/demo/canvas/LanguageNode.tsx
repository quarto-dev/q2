/**
 * How qmd is shown in the editor: the Monarch grammar that colours the
 * document the instant it opens (before the LSP's semantic tokens
 * arrive), the quarto-light / quarto-dark themes that map token names to
 * colours, and the semantic-token legend those themes target. The theme
 * choice is offered on the "language" port.
 */

import { useState } from 'react';
import { Handle, Position } from '@xyflow/react';
import { QMD_TOKEN_LEGEND } from '../../../services/intelligenceService';
import { useOutput, type LanguagePort } from './ports';

const PORTS: Record<LanguagePort['theme'], LanguagePort> = { light: { theme: 'light' }, dark: { theme: 'dark' } };

export default function LanguageNode() {
  const [theme, setTheme] = useState<LanguagePort['theme']>('light');
  useOutput('language', PORTS[theme]);
  return (
    <div className="cd-node cd-language">
      <div className="cd-title">Language</div>
      <div className="cd-row">
        <span>theme</span>
        <span className="cd-choice">
          <button disabled={theme === 'light'} onClick={() => setTheme('light')}>quarto-light</button>
          <button disabled={theme === 'dark'} onClick={() => setTheme('dark')}>quarto-dark</button>
        </span>
      </div>
      <div className="cd-meta">qmd Monarch grammar · {QMD_TOKEN_LEGEND.length} semantic token types</div>
      <Handle type="source" position={Position.Left} id="language" title="language" />
    </div>
  );
}
