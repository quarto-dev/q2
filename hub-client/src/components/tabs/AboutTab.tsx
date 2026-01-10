/**
 * About Tab Component
 *
 * Displays information about Quarto Hub:
 * - Commit indicator
 * - Links to documentation and resources
 * - Rendered changelog
 */

import { useState, useEffect } from 'react';
import { renderToHtml, isWasmReady } from '../../services/wasmRenderer';
import changelogMd from '../../../changelog.md?raw';
import './AboutTab.css';

type WasmStatus = 'loading' | 'ready' | 'error';

interface AboutTabProps {
  wasmStatus: WasmStatus;
}

export default function AboutTab({ wasmStatus }: AboutTabProps) {
  const [changelogHtml, setChangelogHtml] = useState<string>('');
  const [changelogError, setChangelogError] = useState<string | null>(null);

  // Render changelog when WASM becomes ready
  useEffect(() => {
    if (wasmStatus !== 'ready' || !isWasmReady()) {
      return;
    }

    async function renderChangelog() {
      try {
        const result = await renderToHtml(changelogMd);
        if (result.success) {
          setChangelogHtml(result.html);
          setChangelogError(null);
        } else {
          setChangelogError(result.error || 'Failed to render changelog');
        }
      } catch (err) {
        setChangelogError(err instanceof Error ? err.message : 'Unknown error');
      }
    }

    renderChangelog();
  }, [wasmStatus]);

  return (
    <div className="about-tab">
      <div className="about-tab-section">
        <label className="section-label">Quarto Hub</label>
        <p className="about-description">
          A collaborative editor for Quarto projects.
        </p>
      </div>

      <div className="about-tab-section">
        <label className="section-label">Links</label>
        <ul className="about-links">
          <li>
            <a
              href="https://github.com/quarto-dev/kyoto"
              target="_blank"
              rel="noopener noreferrer"
            >
              GitHub Repository
            </a>
          </li>
        </ul>
      </div>

      <div className="about-tab-section">
        <label className="section-label">Build Info</label>
        <div className="version-info">
          <span className="commit-label">commit</span>
          <span
            className="commit-hash"
            title={`Built: ${__BUILD_TIME__}\nCommit date: ${__GIT_COMMIT_DATE__}`}
          >
            {__GIT_COMMIT_HASH__}
          </span>
        </div>
      </div>

      <div className="about-tab-section changelog-section">
        <label className="section-label">Changelog</label>
        {wasmStatus === 'loading' && (
          <div className="changelog-loading">Loading renderer...</div>
        )}
        {wasmStatus === 'error' && (
          <div className="changelog-error">Renderer unavailable</div>
        )}
        {changelogError && (
          <div className="changelog-error">{changelogError}</div>
        )}
        {changelogHtml && (
          <div className="changelog-container">
            <iframe
              srcDoc={changelogHtml}
              title="Changelog"
              sandbox="allow-same-origin"
              className="changelog-iframe"
            />
          </div>
        )}
      </div>
    </div>
  );
}
