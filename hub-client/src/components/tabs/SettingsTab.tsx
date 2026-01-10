/**
 * Settings Tab Component
 *
 * Displays user settings:
 * - Scroll sync toggle
 */

import './SettingsTab.css';

interface SettingsTabProps {
  scrollSyncEnabled: boolean;
  onScrollSyncChange: (enabled: boolean) => void;
}

export default function SettingsTab({
  scrollSyncEnabled,
  onScrollSyncChange,
}: SettingsTabProps) {
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
    </div>
  );
}
