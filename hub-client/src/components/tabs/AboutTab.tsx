/**
 * About Tab Component
 *
 * Displays information about Quarto Hub:
 * - Commit indicator
 * - Links to documentation and resources
 * - Button to view changelog in modal
 */

import { useState, useEffect } from 'react';
import { renderToHtml, isWasmReady } from '../../services/wasmRenderer';
import changelogMd from '../../../changelog.md?raw';
import './AboutTab.css';

type WasmStatus = 'loading' | 'ready' | 'error';

interface AboutTabProps {
  wasmStatus: WasmStatus;
}

// Minimal CSS for changelog rendering
const changelogStyles = `
  body {
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Oxygen, Ubuntu, sans-serif;
    font-size: 14px;
    line-height: 1.6;
    color: #333;
    padding: 24px;
    margin: 0;
    max-width: 800px;
  }
  h2 {
    font-size: 20px;
    font-weight: 600;
    margin: 0 0 16px 0;
    color: #111;
  }
  ul {
    margin: 0;
    padding: 0 0 0 20px;
  }
  li {
    margin: 8px 0;
  }
  a {
    color: #646cff;
    text-decoration: none;
  }
  a:hover {
    text-decoration: underline;
  }
  code {
    font-family: 'SF Mono', Monaco, 'Cascadia Code', monospace;
    font-size: 13px;
    background: #f4f4f4;
    padding: 2px 6px;
    border-radius: 3px;
  }
`;

export default function AboutTab({ wasmStatus }: AboutTabProps) {
  const [changelogHtml, setChangelogHtml] = useState<string>('');
  const [changelogError, setChangelogError] = useState<string | null>(null);
  const [showModal, setShowModal] = useState(false);

  // Render changelog when WASM becomes ready
  useEffect(() => {
    if (wasmStatus !== 'ready' || !isWasmReady()) {
      return;
    }

    async function renderChangelog() {
      try {
        const result = await renderToHtml(changelogMd);
        if (result.success) {
          // Inject minimal styles into the rendered HTML
          const styledHtml = result.html.replace(
            '</head>',
            `<style>${changelogStyles}</style></head>`
          );
          setChangelogHtml(styledHtml);
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

  const handleOpenChangelog = () => {
    setShowModal(true);
  };

  const handleCloseModal = () => {
    setShowModal(false);
  };

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
          <li>
            <button
              className="changelog-link-btn"
              onClick={handleOpenChangelog}
              disabled={wasmStatus !== 'ready' || !!changelogError}
            >
              {wasmStatus === 'loading' ? 'Loading...' : 'View Changelog'}
            </button>
            {changelogError && (
              <span className="changelog-error-hint"> (unavailable)</span>
            )}
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

      {/* Changelog Modal */}
      {showModal && (
        <div className="changelog-modal-overlay" onClick={handleCloseModal}>
          <div className="changelog-modal" onClick={(e) => e.stopPropagation()}>
            <div className="changelog-modal-header">
              <h3>Changelog</h3>
              <button className="changelog-modal-close" onClick={handleCloseModal}>
                ×
              </button>
            </div>
            <div className="changelog-modal-content">
              {changelogHtml ? (
                <iframe
                  srcDoc={changelogHtml}
                  title="Changelog"
                  sandbox="allow-same-origin"
                  className="changelog-iframe"
                />
              ) : (
                <div className="changelog-loading">Loading changelog...</div>
              )}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
