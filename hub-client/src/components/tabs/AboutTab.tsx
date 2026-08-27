/**
 * About Tab Component
 *
 * Displays information about Quarto Hub:
 * - Commit indicator
 * - Links to documentation and resources
 * - Buttons to view markdown documents (changelog, more info) in modal
 */

import { useState, useEffect, useMemo } from 'react';
import Tooltip from '../Tooltip';
import { SHORTCUT_GROUPS } from '../../utils/keyboardShortcuts';
import { common, tabs } from '../../strings';
import { renderContentToHtml, isWasmReady } from '@quarto/preview-runtime';
import { useTheme } from '../ThemeContext';
import { injectChangelogStyles } from '../../utils/changelogDoc';
import changelogMd from '../../../changelog.md?raw';
import moreInfoMd from '../../../resources/more-info.md?raw';
import './AboutTab.css';

type WasmStatus = 'loading' | 'ready' | 'error';

interface AboutTabProps {
  wasmStatus: WasmStatus;
}

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
  const [rawDocs, setRawDocs] = useState<Record<string, string>>({});
  const [renderError, setRenderError] = useState<string | null>(null);
  const [activeModal, setActiveModal] = useState<string | null>(null);
  const { effectiveTheme } = useTheme();

  // Render all markdown documents when WASM becomes ready
  useEffect(() => {
    if (wasmStatus !== 'ready' || !isWasmReady()) {
      return;
    }

    async function renderDocuments() {
      try {
        const rendered: Record<string, string> = {};
        for (const [key, doc] of Object.entries(documents)) {
          const result = await renderContentToHtml(doc.markdown);
          if (result.success) {
            rendered[key] = result.html;
          } else {
            setRenderError(result.error || `Failed to render ${doc.title}`);
            return;
          }
        }
        setRawDocs(rendered);
        setRenderError(null);
      } catch (err) {
        setRenderError(err instanceof Error ? err.message : 'Unknown error');
      }
    }

    renderDocuments();
  }, [wasmStatus]);

  // Inject theme-matched styles into the iframe documents. Kept as a pure
  // re-injection over the raw renders so a theme flip restyles an already
  // rendered document without re-running the WASM pipeline (GH #624).
  const renderedDocs = useMemo(() => {
    const themed: Record<string, string> = {};
    for (const [key, html] of Object.entries(rawDocs)) {
      themed[key] = injectChangelogStyles(html, effectiveTheme);
    }
    return themed;
  }, [rawDocs, effectiveTheme]);

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
          {tabs.about.tagline}
        </p>
      </div>

      <div className="about-tab-section">
        <label className="section-label">{tabs.about.linksLabel}</label>
        <ul className="about-links">
          <li>
            <a
              href="https://github.com/quarto-dev/kyoto"
              target="_blank"
              rel="noopener noreferrer"
            >
              {tabs.about.github}
            </a>
          </li>
          <li>
            <button
              className="changelog-link-btn"
              onClick={() => handleOpenModal('moreInfo')}
              disabled={!isReady}
            >
              {wasmStatus === 'loading' ? common.loading : tabs.about.moreInfo}
            </button>
          </li>
          <li>
            <button
              className="changelog-link-btn"
              onClick={() => handleOpenModal('changelog')}
              disabled={!isReady}
            >
              {wasmStatus === 'loading' ? common.loading : tabs.about.viewChangelog}
            </button>
            {renderError && (
              <span className="changelog-error-hint"> {tabs.about.unavailable}</span>
            )}
          </li>
        </ul>
      </div>

      <div className="about-tab-section">
        <label className="section-label">{tabs.about.shortcutsLabel}</label>
        {SHORTCUT_GROUPS.map((group) => (
          <div key={group.title} className="shortcuts-group">
            <span className="shortcuts-group-title">{group.title}</span>
            <dl className="shortcuts-list">
              {group.entries.map((entry) => (
                <div key={`${entry.keys}-${entry.action}`} className="shortcuts-row">
                  <dt>
                    <kbd>{entry.keys}</kbd>
                  </dt>
                  <dd>{entry.action}</dd>
                </div>
              ))}
            </dl>
          </div>
        ))}
      </div>

      <div className="about-tab-section">
        <label className="section-label">{tabs.about.buildInfoLabel}</label>
        <div className="version-info">
          <span className="commit-label">{tabs.about.commitLabel}</span>
          <Tooltip content={tabs.about.builtTooltip(__BUILD_TIME__, __GIT_COMMIT_DATE__)}>
            <span className="commit-hash" tabIndex={0}>
              {__GIT_COMMIT_HASH__}
            </span>
          </Tooltip>
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
                <div className="changelog-loading">{common.loading}</div>
              )}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
