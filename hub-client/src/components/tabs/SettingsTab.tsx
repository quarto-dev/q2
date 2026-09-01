/**
 * Settings Tab Component
 *
 * Displays user settings:
 * - Scroll sync toggle
 * - Error overlay collapsed toggle
 * - Nesting cursor toggle
 * - Rich-text editor toggle
 * - Document branches toggle (experimental)
 *
 * Preferences only — actions that produce artifacts (Export ZIP, Screenshot
 * Preview) live in the Project tab.
 */

import './SettingsTab.css';
import { usePreference } from '../../hooks/usePreference';
import { tabs } from '../../strings';

interface SettingsTabProps {
  scrollSyncEnabled: boolean;
  onScrollSyncChange: (enabled: boolean) => void;
}

export default function SettingsTab({
  scrollSyncEnabled,
  onScrollSyncChange,
}: SettingsTabProps) {
  const [errorOverlayCollapsed, setErrorOverlayCollapsed] = usePreference('errorOverlayCollapsed');
  const [unlockNestingCursor, setUnlockNestingCursor] = usePreference('unlockNestingCursor');
  const [richText, setRichText] = usePreference('richText');
  const [documentBranches, setDocumentBranches] = usePreference('documentBranches');

  return (
    <div className="settings-tab">
      <div className="settings-tab-section">
        <label className="setting-toggle">
          <input
            type="checkbox"
            checked={scrollSyncEnabled}
            onChange={(e) => onScrollSyncChange(e.target.checked)}
          />
          <span className="setting-name">{tabs.settings.scrollSync}</span>
          <span className="setting-description">
            {tabs.settings.scrollSyncDescription}
          </span>
        </label>
        <label className="setting-toggle">
          <input
            type="checkbox"
            checked={errorOverlayCollapsed}
            onChange={(e) => setErrorOverlayCollapsed(e.target.checked)}
          />
          <span className="setting-name">{tabs.settings.collapseErrorOverlay}</span>
          <span className="setting-description">
            {tabs.settings.collapseErrorOverlayDescription}
          </span>
        </label>
        <label className="setting-toggle">
          <input
            type="checkbox"
            checked={unlockNestingCursor}
            onChange={(e) => setUnlockNestingCursor(e.target.checked)}
          />
          <span className="setting-name">{tabs.settings.nestingCursor}</span>
          <span className="setting-description">
            {tabs.settings.nestingCursorDescription}
          </span>
        </label>
        <label className="setting-toggle">
          <input
            type="checkbox"
            checked={richText}
            onChange={(e) => setRichText(e.target.checked)}
          />
          <span className="setting-name">{tabs.settings.richText}</span>
          <span className="setting-description">
            {tabs.settings.richTextDescription}
          </span>
        </label>
        <label className="setting-toggle">
          <input
            type="checkbox"
            checked={documentBranches}
            onChange={(e) => setDocumentBranches(e.target.checked)}
          />
          <span className="setting-name">{tabs.settings.documentBranches}</span>
          <span className="setting-description">
            {tabs.settings.documentBranchesDescription}
          </span>
        </label>
      </div>
    </div>
  );
}
