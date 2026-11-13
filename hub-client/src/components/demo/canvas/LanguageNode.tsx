/**
 * How qmd is shown in the editor: the `qmd` Monaco language with its
 * Monarch grammar, which colours the document the instant it opens
 * (before the LSP's semantic tokens arrive), the quarto-light /
 * quarto-dark themes that map token names to colours, and the
 * semantic-token legend those themes target. Offered on the "language"
 * port; an editor without it shows plain text.
 */

import { useState } from 'react';
import { Handle, Position } from '@xyflow/react';
import { QMD_LANGUAGE_ID } from '../../quartoTheme';
import { QMD_TOKEN_LEGEND } from '../../../services/intelligenceService';
import { useOutput, type LanguagePort } from './ports';

const PORTS: Record<LanguagePort['theme'], LanguagePort> = {
  light: { languageId: QMD_LANGUAGE_ID, theme: 'light' },
  dark: { languageId: QMD_LANGUAGE_ID, theme: 'dark' },
};

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
      <div className="cd-meta">language {QMD_LANGUAGE_ID} · Monarch grammar · {QMD_TOKEN_LEGEND.length} semantic token types</div>
      <Handle type="source" position={Position.Left} id="language" title="language" />
    </div>
  );
}
