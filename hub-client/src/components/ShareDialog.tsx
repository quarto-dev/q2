/**
 * Share Dialog Component
 *
 * Modal dialog for sharing a project with a shareable URL.
 * Displays a warning about permanent access and allows copying the link.
 */

import { useState, useCallback, useRef, useEffect } from 'react';
import './ShareDialog.css';

export interface ShareDialogProps {
  isOpen: boolean;
  shareableUrl: string;
  onClose: () => void;
  onCopied?: () => void;
}

export default function ShareDialog({
  isOpen,
  shareableUrl,
  onClose,
  onCopied,
}: ShareDialogProps) {
  const [copied, setCopied] = useState(false);
  const urlInputRef = useRef<HTMLInputElement>(null);

  // Reset copied state when dialog opens
  useEffect(() => {
    if (isOpen) {
      setCopied(false);
    }
  }, [isOpen]);

  // Select all text when dialog opens
  useEffect(() => {
    if (isOpen && urlInputRef.current) {
      setTimeout(() => {
        urlInputRef.current?.select();
      }, 100);
    }
  }, [isOpen]);

  const handleCopyLink = useCallback(async () => {
    try {
      // Try modern Clipboard API first
      if (navigator.clipboard && navigator.clipboard.writeText) {
        await navigator.clipboard.writeText(shareableUrl);
      } else {
        // Fallback for older browsers or HTTP contexts
        const textArea = document.createElement('textarea');
        textArea.value = shareableUrl;
        textArea.style.position = 'fixed';
        textArea.style.left = '-9999px';
        document.body.appendChild(textArea);
        textArea.select();
        document.execCommand('copy');
        document.body.removeChild(textArea);
      }

      setCopied(true);
      onCopied?.();

      // Close dialog after a brief delay to show the success state
      setTimeout(() => {
        onClose();
      }, 500);
    } catch (err) {
      console.error('Failed to copy to clipboard:', err);
    }
  }, [shareableUrl, onCopied, onClose]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === 'Escape') {
        onClose();
      } else if (e.key === 'Enter') {
        handleCopyLink();
      }
    },
    [onClose, handleCopyLink]
  );

  if (!isOpen) return null;

  return (
    <div className="dialog-overlay" onClick={onClose}>
      <div
        className="share-dialog"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={handleKeyDown}
      >
        <div className="dialog-header">
          <h2>Share Project</h2>
          <button className="close-btn" onClick={onClose}>
            &times;
          </button>
        </div>

        <div className="dialog-content">
          <div className="warning-box">
            <span className="warning-icon">&#9888;</span>
            <p>
              <strong>Anyone with this link can access and edit this project permanently.</strong>
            </p>
            <p className="warning-detail">
              Only share with people you trust. This link cannot be revoked.
            </p>
          </div>

          <div className="url-field">
            <label htmlFor="shareable-url">Shareable Link:</label>
            <input
              ref={urlInputRef}
              id="shareable-url"
              type="text"
              value={shareableUrl}
              readOnly
              onClick={(e) => (e.target as HTMLInputElement).select()}
            />
          </div>
        </div>

        <div className="dialog-actions">
          <button className="cancel-btn" onClick={onClose}>
            Cancel
          </button>
          <button
            className={`copy-btn ${copied ? 'copied' : ''}`}
            onClick={handleCopyLink}
          >
            {copied ? 'Copied!' : 'Copy Link'}
          </button>
        </div>
      </div>
    </div>
  );
}
