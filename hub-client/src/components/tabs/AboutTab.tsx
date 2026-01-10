/**
 * About Tab Component
 *
 * Displays information about Quarto Hub:
 * - Commit indicator
 * - Links to documentation and resources
 */

import './AboutTab.css';

export default function AboutTab() {
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
            <span className="coming-soon">Changelog (coming soon)</span>
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
    </div>
  );
}
