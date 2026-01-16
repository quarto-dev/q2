/**
 * Settings Tab Component
 *
 * Displays user settings:
 * - Scroll sync toggle
 * - Error overlay collapsed toggle
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

  return (
    <div className="settings-tab">
      <div className="settings-tab-section">
        <label className="section-label">Editor</label>
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
      </div>
      <div className="settings-tab-section">
        <label className="section-label">Preview</label>
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
      </div>
    </div>
  );
}
