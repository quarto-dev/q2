/**
 * "Import document" control (document import P5): an icon button in the Files sidebar header,
 * next to Add asset, that opens the file picker, filtered to the formats the Rust format table lists.
 *
 * The `accept` filter is advisory (users can override it in the picker), so the dialog
 * re-validates every file. Presentational: choosing files hands them to `onPick`, which
 * queues an import dialog for each.
 */
import { useRef } from 'react';
import { FileImportIcon } from './icons';
import Tooltip from './Tooltip';
import { importDoc } from '../strings';
import type { ImportFormats } from '../pandoc/importService';
import { importAccept } from '../utils/importFormats';

export interface ImportControlProps {
  /** The format table; null until it has loaded, which leaves the button disabled. */
  formats: ImportFormats | null;
  /** Disabled for reasons other than loading (replay mode). */
  disabled?: boolean;
  onPick: (file: File) => void;
}

export default function ImportControl({ formats, disabled = false, onPick }: ImportControlProps) {
  const inputRef = useRef<HTMLInputElement>(null);
  const inert = disabled || formats === null;

  return (
    <>
      <Tooltip content={importDoc.buttonTooltip}>
        <button
          type="button"
          className={`qh-btn small outline import-doc-btn${inert ? ' is-disabled' : ''}`}
          aria-label={importDoc.buttonLabel}
          aria-disabled={inert || undefined}
          onClick={() => {
            if (!inert) inputRef.current?.click();
          }}
        >
          <FileImportIcon />
        </button>
      </Tooltip>
      <input
        ref={inputRef}
        type="file"
        className="import-file-input"
        data-testid="import-file-input"
        accept={formats ? importAccept(formats) : undefined}
        multiple
        hidden
        tabIndex={-1}
        aria-hidden="true"
        onChange={(e) => {
          const files = Array.from(e.target.files ?? []);
          // Clear it so choosing the same file again still fires a change.
          e.target.value = '';
          for (const file of files) onPick(file);
        }}
      />
    </>
  );
}
