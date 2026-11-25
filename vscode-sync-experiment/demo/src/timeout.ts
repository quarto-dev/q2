/**
 * `--timeout <seconds>`: end the run this long after the first editor window is up, whatever the story is doing. Setup
 * (the hub, the WASM boot) is not counted, so a short timeout still gives the editor time to render. Pure, so the rule
 * can be tested; the launcher polls it.
 */
export function timeoutDue(editorUpAt: number | undefined, now: number, seconds: number | undefined): boolean {
  if (seconds === undefined || editorUpAt === undefined) return false;
  return now - editorUpAt >= seconds * 1000;
}
