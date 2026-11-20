/**
 * A string. Typed here, or wired in on "string" (then shown, not
 * editable), and offered on "string" to anything that takes a plain
 * string: a server URL for the websocket adapters, a file name for the
 * document. `data.label` names what the string is; `data.value` seeds it.
 */

import { useState } from 'react';
import { Handle, Position, type NodeProps } from '@xyflow/react';
import { useInput, useOutput } from './ports';
import TextBox from './TextBox';

interface StringData {
  label?: string;
  value?: string;
}

export default function StringNode({ data }: NodeProps) {
  const { label, value: seed } = data as StringData;
  const wired = useInput<string>('string');
  const [own, setOwn] = useState(seed ?? '');
  const value = wired ?? own;
  useOutput('string', value === '' ? undefined : value);

  return (
    <div className="cd-node cd-string">
      <div className="cd-title">{label ?? 'String'}</div>
      {wired !== null
        ? <div className="cd-id cd-string-wired" title="wired in">{wired}</div>
        : <TextBox value={own} placeholder="…" className="cd-string-box" onCommit={setOwn} />}
      <Handle type="target" position={Position.Top} id="string" title="string" />
      <Handle type="source" position={Position.Right} id="string" title="string" />
    </div>
  );
}
