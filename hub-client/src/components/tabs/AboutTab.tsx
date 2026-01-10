/**
 * About Tab Component
 *
 * Displays information about Quarto Hub:
 * - Commit indicator
 * - Links to documentation and resources
 * - Buttons to view markdown documents (changelog, more info) in modal
 */

import { useState, useEffect } from 'react';
import { renderToHtml, isWasmReady } from '../../services/wasmRenderer';
import changelogMd from '../../../changelog.md?raw';
import moreInfoMd from '../../../resources/more-info.md?raw';
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

// Document configuration for the modal viewer
interface MarkdownDocument {
  title: string;
  markdown: string;
}

const documents: Record<string, MarkdownDocument> = {
  changelog: { title: 'Changelog', markdown: changelogMd },
  moreInfo: { title: 'More Information', markdown: moreInfoMd },
};

export default function AboutTab({ wasmStatus }: AboutTabProps) {
  const [renderedDocs, setRenderedDocs] = useState<Record<string, string>>({});
  const [renderError, setRenderError] = useState<string | null>(null);
  const [activeModal, setActiveModal] = useState<string | null>(null);

  // Render all markdown documents when WASM becomes ready
  useEffect(() => {
    if (wasmStatus !== 'ready' || !isWasmReady()) {
      return;
    }

    async function renderDocuments() {
      try {
        const rendered: Record<string, string> = {};
        for (const [key, doc] of Object.entries(documents)) {
          const result = await renderToHtml(doc.markdown);
          if (result.success) {
            // Inject minimal styles into the rendered HTML
            rendered[key] = result.html.replace(
              '</head>',
              `<style>${changelogStyles}</style></head>`
            );
          } else {
            setRenderError(result.error || `Failed to render ${doc.title}`);
            return;
          }
        }
        setRenderedDocs(rendered);
        setRenderError(null);
      } catch (err) {
        setRenderError(err instanceof Error ? err.message : 'Unknown error');
      }
    }

    renderDocuments();
  }, [wasmStatus]);

  const handleOpenModal = (docKey: string) => {
    setActiveModal(docKey);
  };

  const handleCloseModal = () => {
    setActiveModal(null);
  };

  const isReady = wasmStatus === 'ready' && !renderError;

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
              onClick={() => handleOpenModal('moreInfo')}
              disabled={!isReady}
            >
              {wasmStatus === 'loading' ? 'Loading...' : 'More Information'}
            </button>
          </li>
          <li>
            <button
              className="changelog-link-btn"
              onClick={() => handleOpenModal('changelog')}
              disabled={!isReady}
            >
              {wasmStatus === 'loading' ? 'Loading...' : 'View Changelog'}
            </button>
            {renderError && (
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

      {/* Markdown Document Modal */}
      {activeModal && (
        <div className="changelog-modal-overlay" onClick={handleCloseModal}>
          <div className="changelog-modal" onClick={(e) => e.stopPropagation()}>
            <div className="changelog-modal-header">
              <h3>{documents[activeModal]?.title}</h3>
              <button className="changelog-modal-close" onClick={handleCloseModal}>
                ×
              </button>
            </div>
            <div className="changelog-modal-content">
              {renderedDocs[activeModal] ? (
                <iframe
                  srcDoc={renderedDocs[activeModal]}
                  title={documents[activeModal]?.title}
                  sandbox="allow-same-origin"
                  className="changelog-iframe"
                />
              ) : (
                <div className="changelog-loading">Loading...</div>
              )}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
