/**
 * Injectable diagnostic logger for the sync client.
 *
 * The sync client runs in two very different hosts:
 *
 *  - the browser (hub-client SPA), where `console.log` is the natural
 *    sink for connection-progress diagnostics; and
 *  - stdio MCP servers (quarto-hub-mcp), where stdout carries the
 *    JSON-RPC protocol stream and ANY non-protocol write corrupts it
 *    (bd-sl4o01y0) — diagnostics must go to stderr.
 *
 * Library code must therefore never call `console.log` directly; it
 * calls `syncLog`, and the host decides where that goes. The default
 * preserves the browser behavior.
 */

export type SyncLogger = (...args: unknown[]) => void;

let logger: SyncLogger = (...args) => console.log(...args);

/** Replace the diagnostic log sink (e.g. stderr in stdio servers). */
export function setSyncLogger(fn: SyncLogger): void {
  logger = fn;
}

/** Emit a connection-progress / diagnostic line via the current sink. */
export function syncLog(...args: unknown[]): void {
  logger(...args);
}
