/** A one-line value as a text box; Enter or blur commits, Escape reverts. */

import { useState, type KeyboardEvent } from 'react';

interface Props {
  value: string;
  placeholder?: string;
  className?: string;
  onCommit: (value: string) => void;
}

export default function TextBox({ value, placeholder, className = '', onCommit }: Props) {
  const [draft, setDraft] = useState<string | null>(null);
  const commit = () => {
    if (draft !== null && draft.trim() !== '' && draft.trim() !== value) onCommit(draft.trim());
    setDraft(null);
  };
  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') e.currentTarget.blur();
    if (e.key === 'Escape') { setDraft(null); e.currentTarget.blur(); }
  };
  return (
    <input
      className={`cd-id cd-id-box nodrag ${className}`}
      spellCheck={false}
      placeholder={placeholder}
      value={draft ?? value}
      onChange={(e) => setDraft(e.target.value)}
      onFocus={() => setDraft(value)}
      onBlur={commit}
      onKeyDown={onKeyDown}
    />
  );
}
