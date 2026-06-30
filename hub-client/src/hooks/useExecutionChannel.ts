import { useEffect, useState } from 'react';
import { getIndexHandle } from '@quarto/preview-runtime';
import {
  createExecutionChannel,
  type LiveExecutor,
} from '../services/executionChannel';

/**
 * Track which `q2` executors are currently online for the connected project
 * (bd-sfet3264, Phase 2D).
 *
 * Starts an execution channel on the index DocHandle while connected and
 * returns the live-executor set (refreshed by capability beacons, pruned when
 * they go stale). The channel is torn down and rebuilt when the connection
 * drops/restores or the active project changes, so it always listens on the
 * current project's index handle.
 *
 * Phase 2 only *consumes* beacons (capability detection). No executor exists
 * yet to produce them — that's Phase 4 — so in practice this returns `[]`
 * today; the wiring is in place for when the executor lands.
 */
export function useExecutionChannel(
  isOnline: boolean,
  indexDocId: string | null,
): LiveExecutor[] {
  const [executors, setExecutors] = useState<LiveExecutor[]>([]);

  useEffect(() => {
    if (!isOnline || !indexDocId) {
      setExecutors([]);
      return;
    }
    const channel = createExecutionChannel({
      getIndexHandle: () => getIndexHandle(),
      onExecutorsChange: setExecutors,
    });
    channel.start();
    return () => {
      channel.stop();
      setExecutors([]);
    };
  }, [isOnline, indexDocId]);

  return executors;
}
