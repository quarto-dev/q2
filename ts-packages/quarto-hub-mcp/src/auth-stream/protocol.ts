/**
 * Token-stream protocol for `q2 provide-hub`'s auth bridge (bd-sfet3264,
 * Phase 3C).
 *
 * The Rust `q2 provide-hub` process spawns this helper as a child and reads
 * Bearer tokens from its **stdout** (newline-delimited JSON), writing
 * `{"type":"refresh"}` to its **stdin** to pull a fresh token before a
 * reconnect. Logs and the interactive auth URL go to **stderr**. stdio pipes
 * are identical on Windows/macOS/Linux, so this hand-off is cross-platform.
 *
 * This module is the transport-agnostic core: it is driven by abstract
 * `getToken`/`forceRefresh` callbacks and an async line input, so it is fully
 * unit-testable without real OAuth, a keyring, or process stdio.
 */

/** A token the parent can use as `Authorization: Bearer <bearer>`. */
export interface Token {
  bearer: string;
  /** ISO-8601 expiry, so the parent can refresh ahead of time. */
  expiresAt: string;
}

/** Outbound stdout frame. */
export type OutFrame =
  | { type: 'token'; bearer: string; expiresAt: string }
  | { type: 'error'; message: string };

/** Inbound stdin command. */
export type InCommand = { type: 'refresh' };

export interface TokenStreamDeps {
  /** Get the current valid token (used for the initial emit). */
  getToken: () => Promise<Token>;
  /** Force a fresh token (in response to a `refresh` command). */
  forceRefresh: () => Promise<Token>;
  /** Inbound stdin lines (newline-delimited). */
  input: AsyncIterable<string>;
  /** Emit one outbound frame (the caller serializes it to a stdout line). */
  emit: (frame: OutFrame) => void;
}

/**
 * Parse one stdin line into a recognized command, or `null` for blank lines,
 * non-JSON, or anything we don't understand (forward-compatible: unknown
 * commands are ignored rather than fatal).
 */
export function parseCommand(line: string): InCommand | null {
  const trimmed = line.trim();
  if (!trimmed) return null;
  try {
    const v: unknown = JSON.parse(trimmed);
    if (v && typeof v === 'object' && (v as { type?: unknown }).type === 'refresh') {
      return { type: 'refresh' };
    }
  } catch {
    // not JSON — ignore
  }
  return null;
}

function errorMessage(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/**
 * Emit the initial token, then service `refresh` commands until the input
 * ends. A failure to obtain the *initial* token is fatal (we emit an error
 * frame and return — the parent cannot authenticate). A failed *refresh* is
 * reported as an error frame but the stream keeps running (the previous token
 * may still be valid; the parent decides).
 */
export async function runTokenStream(deps: TokenStreamDeps): Promise<void> {
  try {
    const t = await deps.getToken();
    deps.emit({ type: 'token', bearer: t.bearer, expiresAt: t.expiresAt });
  } catch (e) {
    deps.emit({ type: 'error', message: errorMessage(e) });
    return;
  }

  for await (const line of deps.input) {
    const cmd = parseCommand(line);
    if (cmd?.type !== 'refresh') continue;
    try {
      const t = await deps.forceRefresh();
      deps.emit({ type: 'token', bearer: t.bearer, expiresAt: t.expiresAt });
    } catch (e) {
      deps.emit({ type: 'error', message: errorMessage(e) });
    }
  }
}
