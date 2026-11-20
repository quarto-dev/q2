/**
 * Shows the document wired into its "doc" port as a devtools-style tree.
 * Objects and arrays fold; every leaf value is a text box, and each
 * keystroke that reads as a value of the same kind goes straight to the doc.
 */

import { useState, type KeyboardEvent } from 'react';
import { Handle, Position } from '@xyflow/react';
import { next as A } from '@automerge/automerge';
import { useInput, type DocPort } from './ports';

type Path = (string | number)[];
type Json = Record<string, unknown>;

function isContainer(value: unknown): value is object {
  return typeof value === 'object' && value !== null && !(value instanceof Uint8Array);
}

/** Read what the user typed back as a value of the same kind as before. */
function parseEntry(text: string, previous: unknown): unknown {
  if (typeof previous === 'string') {
    if (text.startsWith('"')) {
      try {
        const parsed: unknown = JSON.parse(text);
        if (typeof parsed === 'string') return parsed;
      } catch { /* taken literally below */ }
    }
    return text;
  }
  try {
    return JSON.parse(text);
  } catch {
    return text;
  }
}

/** Write `value` at `path` inside the doc being changed. */
function setAt(d: Json, path: Path, value: unknown): void {
  let target: unknown = d;
  for (const step of path.slice(0, -1)) target = (target as Record<string | number, unknown>)[step];
  const key = path[path.length - 1];
  const parent = target as Record<string | number, unknown>;
  if (typeof value === 'string' && typeof parent[key] === 'string') {
    A.updateText(d, path, value);
  } else {
    parent[key] = value;
  }
}

function Leaf({ value, onCommit }: { value: unknown; onCommit: (next: unknown) => void }) {
  // While focused the box shows what was typed; otherwise it shows the doc.
  const [draft, setDraft] = useState<string | null>(null);
  const text = draft ?? (typeof value === 'string' ? value : JSON.stringify(value) ?? String(value));
  const kind = value === null ? 'null' : typeof value;

  const onChange = (typed: string) => {
    setDraft(typed);
    const next = parseEntry(typed, value);
    if (next !== value) onCommit(next);
  };
  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Escape') e.currentTarget.blur();
  };
  const rows = Math.min(10, text.split('\n').length);
  return (
    <textarea
      className={`cd-json-edit cd-json-${kind} nodrag nowheel`}
      rows={rows}
      spellCheck={false}
      value={text}
      onChange={(e) => onChange(e.target.value)}
      onFocus={() => setDraft(text)}
      onBlur={() => setDraft(null)}
      onKeyDown={onKeyDown}
    />
  );
}

function Entry({ name, value, path, edit }: { name: string; value: unknown; path: Path; edit: (path: Path, v: unknown) => void }) {
  const [open, setOpen] = useState(path.length < 2);
  if (!isContainer(value)) {
    return (
      <div className="cd-json-row cd-json-leaf">
        <span className="cd-json-key">{name}</span>: <Leaf value={value} onCommit={(v) => edit(path, v)} />
      </div>
    );
  }
  const entries = Array.isArray(value)
    ? value.map((v, i) => [String(i), v, i] as const)
    : Object.entries(value).map(([k, v]) => [k, v, k] as const);
  const summary = Array.isArray(value) ? `Array(${value.length})` : `{…}`;
  return (
    <div className="cd-json-row">
      <span className="cd-json-toggle" onClick={() => setOpen(!open)}>{open ? '▾' : '▸'}</span>
      <span className="cd-json-key">{name}</span>: {open ? '' : <span className="cd-json-summary">{summary}</span>}
      {open && (
        <div className="cd-json-children">
          {entries.map(([k, v, step]) => <Entry key={k} name={k} value={v} path={[...path, step]} edit={edit} />)}
        </div>
      )}
    </div>
  );
}

export default function RawViewNode() {
  const port = useInput<DocPort<Json>>('doc');
  const edit = (path: Path, value: unknown) => port?.change((d) => setAt(d, path, value));
  return (
    <div className="cd-node cd-rawview">
      <div className="cd-title">Raw view</div>
      {port === null
        ? <div className="cd-empty">No doc connected to view</div>
        : (
          <div className="cd-json nowheel">
            {Object.entries(port.doc).map(([k, v]) => <Entry key={k} name={k} value={v} path={[k]} edit={edit} />)}
          </div>
        )}
      <Handle type="target" position={Position.Left} id="doc" title="doc" />
    </div>
  );
}
