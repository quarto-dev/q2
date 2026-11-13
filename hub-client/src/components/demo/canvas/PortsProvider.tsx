/** The shared registry behind canvas/ports.ts; wrap the ReactFlow in it. */

import { useCallback, useMemo, useState, type ReactNode } from 'react';
import { PortsContext } from './ports';

export default function PortsProvider({ children }: { children: ReactNode }) {
  const [values, setValues] = useState<Map<string, unknown>>(() => new Map());
  const publish = useCallback((key: string, value: unknown) => {
    setValues((prev) => {
      if (prev.get(key) === value) return prev;
      const next = new Map(prev);
      if (value === undefined) next.delete(key); else next.set(key, value);
      return next;
    });
  }, []);
  const ports = useMemo(() => ({ values, publish }), [values, publish]);
  return <PortsContext.Provider value={ports}>{children}</PortsContext.Provider>;
}
