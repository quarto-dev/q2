/**
 * What the hub-client does when the project owner replaces a file (GE-7), wired once by `App`: each part of the app that
 * holds a doc handle rebinds or resets itself from the same `done` event.
 *
 * - presence moves to the new handle and tells peers on the old doc that we moved (G25);
 * - the branch service goes back to main for the path and reports how many branches stay with the old doc (G34), which
 *   becomes a line of the notice;
 * - the replay session closes itself (`useReplayMode` subscribes on its own, because the session lives in that hook);
 * - the attribution store notices a different doc id by itself on its next read.
 */
import { swapStatus } from '@quarto/preview-runtime';
import { BRANCHES_STAY, formatNotice } from '@quarto/quarto-git-epochs';
import { handleDocSwapped } from './branchService';
import { rebindCurrentFile } from './presenceService';
import { isCompletedSwap, subscribeFileReplaced } from './swapEvents';

/** Subscribe the consumers. Returns the function that removes them. */
export function installSwapConsumers(): () => void {
  return subscribeFileReplaced((event) => {
    if (!isCompletedSwap(event)) return;
    rebindCurrentFile();
    const stayed = handleDocSwapped(event.path, event.oldDocId);
    if (stayed > 0) swapStatus.appendToNextNotice(formatNotice(BRANCHES_STAY, { n: stayed }));
  });
}
