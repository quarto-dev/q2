/**
 * Auth-loss teardown decision for the currently-open project.
 *
 * Extracted from App.tsx so the transition semantics are pinned by unit
 * tests: teardown requires a genuine auth LOSS (`hadAuth && !hasAuth`),
 * not merely the state "logged off with a hub project open" — a cached
 * hub project legitimately opens logged-off under the local actor
 * (B1, bd-qklxdkwh) and must stay open.
 */
export function shouldTeardownOnAuthChange(args: {
  authEnabled: boolean;
  hadAuth: boolean;
  hasAuth: boolean;
  authLoading: boolean;
  projectSyncServer: string | undefined | null;
}): boolean {
  return (
    args.authEnabled &&
    args.hadAuth &&
    !args.hasAuth &&
    !args.authLoading &&
    !!args.projectSyncServer
  );
}
