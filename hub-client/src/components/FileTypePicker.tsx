/**
 * FileTypePicker — "File type:" radio group of icon buttons (.qmd, .md,
 * .yml, other). With "other" the filename is used as typed.
 */

import type { ReactNode } from 'react';
import { QmdFileIcon, FileTextIcon, GearIcon, FilePlusIcon } from './icons';
import { dialogs } from '../strings';

export type FileTypeChoice = 'qmd' | 'md' | 'yml' | 'other';

const CHOICES: Array<{ id: FileTypeChoice; label: string; icon: ReactNode }> = [
  { id: 'qmd', label: '.qmd', icon: <QmdFileIcon size={18} /> },
  { id: 'md', label: '.md', icon: <FileTextIcon size={18} /> },
  { id: 'yml', label: '.yml', icon: <GearIcon size={18} /> },
  { id: 'other', label: dialogs.newFile.typeOther, icon: <FilePlusIcon size={18} /> },
];

export interface FileTypePickerProps {
  value: FileTypeChoice;
  onChange: (choice: FileTypeChoice) => void;
}

export default function FileTypePicker({ value, onChange }: FileTypePickerProps) {
  return (
    <div className="file-type-picker">
      <div className="file-type-choices" role="radiogroup" aria-label={dialogs.newFile.typeLabel}>
        {CHOICES.map((c) => (
          <button
            key={c.id}
            type="button"
            role="radio"
            aria-checked={value === c.id}
            className={`qh-btn outline small file-type-choice ${value === c.id ? 'checked' : ''}`}
            onClick={() => onChange(c.id)}
          >
            <span className="file-type-choice-icon">{c.icon}</span>
            <span
              className={`file-type-choice-label ${c.id === 'other' ? 'file-type-choice-label--other' : ''}`}
            >
              {c.label}
            </span>
          </button>
        ))}
      </div>
    </div>
  );
}
