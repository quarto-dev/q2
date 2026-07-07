import { useCallback, useEffect, useRef, useState } from 'react';
import { getIndexHandle } from '@quarto/preview-runtime';
import {
  createExecutionChannel,
  type ExecutionChannel,
  type LiveExecutor,
} from '../services/executionChannel';

/** What {@link useExecutionChannel} exposes to the editor. */
export interface UseExecutionChannel {
  /** Executors currently believed online (refreshed by beacons, pruned stale). */
  executors: LiveExecutor[];
  /**
   * Broadcast an "execute this document now" request on the index channel.
   * Returns the request id, or `null` if not connected. Stable across renders.
   */
  requestExecution: (path: string) => string | null;
}

/**
 * Track which `q2` executors are online for the connected project and expose a
 * way to ask one to run a document (bd-sfet3264, Phase 2D + Phase 4b).
 *
 * Starts an execution channel on the index DocHandle while connected: it
 * surfaces the live-executor set (Phase 2) and, for Phase 4b, hands back a
 * stable `requestExecution` the Run affordance calls. The channel is torn down
 * and rebuilt when the connection drops/restores or the active project changes,
 * so it always listens/broadcasts on the current project's index handle.
 */
export function useExecutionChannel(
  isOnline: boolean,
  indexDocId: string | null,
): UseExecutionChannel {
  const [executors, setExecutors] = useState<LiveExecutor[]>([]);
  const channelRef = useRef<ExecutionChannel | null>(null);

  useEffect(() => {
    if (!isOnline || !indexDocId) {
      setExecutors([]);
      return;
    }
    const channel = createExecutionChannel({
      getIndexHandle: () => getIndexHandle(),
      onExecutorsChange: setExecutors,
    });
    channelRef.current = channel;
    channel.start();
    return () => {
      channel.stop();
      channelRef.current = null;
      setExecutors([]);
    };
  }, [isOnline, indexDocId]);

  const requestExecution = useCallback(
    (path: string) => channelRef.current?.requestExecution(path) ?? null,
    [],
  );

  return { executors, requestExecution };
}
