/**
 * Settings Tab Component
 *
 * Displays user settings:
 * - Scroll sync toggle
 * - Error overlay collapsed toggle
 * - Preview screenshot button
 */

import { useState } from 'react';
import html2canvas from 'html2canvas';
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
  const [isCapturing, setIsCapturing] = useState(false);

  const handleScreenshot = async () => {
    try {
      setIsCapturing(true);

      // Find the preview pane element
      const previewPane = document.querySelector('.preview-pane') as HTMLElement;

      if (!previewPane) {
        alert('Preview pane not found');
        return;
      }

      // Capture the preview pane using html2canvas
      const canvas = await html2canvas(previewPane, {
        backgroundColor: '#ffffff',
        useCORS: true,
        logging: false,
      });

      // Convert canvas to blob and download
      canvas.toBlob((blob) => {
        if (blob) {
          const url = URL.createObjectURL(blob);
          const link = document.createElement('a');
          link.href = url;
          link.download = `preview-screenshot-${new Date().toISOString().slice(0, 19).replace(/:/g, '-')}.png`;
          document.body.appendChild(link);
          link.click();
          document.body.removeChild(link);
          URL.revokeObjectURL(url);
        }
      }, 'image/png');
    } catch (error) {
      console.error('Failed to capture screenshot:', error);
      alert('Failed to capture screenshot. Please try again.');
    } finally {
      setIsCapturing(false);
    }
  };

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
        <div style={{ marginTop: '16px' }}>
          <button
            className="screenshot-button"
            onClick={handleScreenshot}
            disabled={isCapturing}
            style={{
              width: '100%',
              padding: '8px 12px',
              backgroundColor: '#007acc',
              color: 'white',
              border: 'none',
              borderRadius: '4px',
              cursor: isCapturing ? 'wait' : 'pointer',
              fontSize: '13px',
              fontWeight: 500,
            }}
          >
            {isCapturing ? 'Capturing...' : '📸 Screenshot Preview'}
          </button>
          <span className="setting-description" style={{ marginTop: '8px', display: 'block' }}>
            Capture the current preview as a PNG image
          </span>
        </div>
      </div>
    </div>
  );
}
