/**
 * Settings Tab Component
 *
 * Displays user settings:
 * - Scroll sync toggle
 * - Error overlay collapsed toggle
 * - Nesting cursor toggle
 * - Rich-text editor toggle
 *
 * Preferences only — actions that produce artifacts (Export ZIP, Screenshot
 * Preview) live in the Project tab.
 */

import './SettingsTab.css';
import { usePreference } from '../../hooks/usePreference';

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

  return (
    <div className="settings-tab">
      <div className="settings-tab-section">
        <label className="setting-toggle">
          <input
            type="checkbox"
            checked={scrollSyncEnabled}
            onChange={(e) => onScrollSyncChange(e.target.checked)}
          />
          <span className="setting-name">Scroll sync</span>
          <span className="setting-description">
            Sync scroll position between editor and preview
          </span>
        </label>
        <label className="setting-toggle">
          <input
            type="checkbox"
            checked={errorOverlayCollapsed}
            onChange={(e) => setErrorOverlayCollapsed(e.target.checked)}
          />
          <span className="setting-name">Collapse error overlay</span>
          <span className="setting-description">
            Show errors as a small indicator instead of expanded panel
          </span>
        </label>
        <label className="setting-toggle">
          <input
            type="checkbox"
            checked={unlockNestingCursor}
            onChange={(e) => setUnlockNestingCursor(e.target.checked)}
          />
          <span className="setting-name">Nesting cursor</span>
          <span className="setting-description">
            Descend into nested list/quote blocks; edit each level cleanly.
          </span>
        </label>
        <label className="setting-toggle">
          <input
            type="checkbox"
            checked={richText}
            onChange={(e) => setRichText(e.target.checked)}
          />
          <span className="setting-name">Rich-text editor</span>
          <span className="setting-description">
            Edit paragraphs and headings as formatted text (WYSIWYG) instead of
            raw markdown. Other blocks still use the plain text editor.
          </span>
        </label>
      </div>
    </div>
  );
}
