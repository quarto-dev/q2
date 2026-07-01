/**
 * @quarto/api — console namespace (HOST-ONLY)
 *
 * Ported from Q1:
 *   - info, warning, error: deno_ral/log.ts (routed through PlatformHost.log)
 *   - withSpinner: core/console.ts:76 (NEUTRAL — no cliffy/ANSI animation)
 *   - completeMessage: core/console.ts:147
 *
 * All I/O goes through the injected PlatformHost.log — never Deno / node:*.
 *
 * withSpinner is neutral: it runs the provided async fn, emits the start
 * message and a completion message via host.log, and returns the fn's result.
 * No setInterval/ANSI animation — those are Deno/cliffy-specific. The
 * completion format mirrors Q1's completeMessage ("[✓] msg").
 *
 * Factory: makeConsole(host: Pick<PlatformHost, "log">): ConsoleNamespace
 */

import type { PlatformHost } from "../platform/index.js";
import type { QuartoAPI } from "@quarto/types";

// Q1 spinner completion marker chars (from core/console.ts)
const kSpinnerCompleteContainerOpen = "[";
const kSpinnerCompleteContainerClose = "]";
const kSpinnerCompleteChar = "✓";

/** Options for logging a message (mirrors Q1's LogMessageOptions). */
export interface LogMessageOptions {
  newline?: boolean;
  bold?: boolean;
  format?: (msg: string) => string;
}

/**
 * The console namespace returned by `makeConsole`.
 *
 * Fully-host namespace: derived from the vendored SDK contract
 * (`QuartoAPI["console"]`) rather than redefined (Plan 2 B2, Fix B). The impl
 * functions below keep the local `LogMessageOptions`/inline spinner-options
 * signatures; they conform to the derived shape (the SDK's option types are
 * structurally compatible), and a future SDK method addition becomes a compile
 * error in `makeConsole` until implemented.
 */
export type ConsoleNamespace = QuartoAPI["console"];

/**
 * Build the console namespace backed by the given host.
 *
 * @param host - A PlatformHost (or minimal fake) with a `log` sub-object.
 */
export function makeConsole(
  host: Pick<PlatformHost, "log">,
): ConsoleNamespace {
  function info(message: string, _options?: LogMessageOptions): void {
    host.log.info(message);
  }

  function warning(message: string, _options?: LogMessageOptions): void {
    host.log.warning(message);
  }

  function error(message: string, _options?: LogMessageOptions): void {
    host.log.error(message);
  }

  function completeMessage(message: string): void {
    // Q1 format: "\r[✓] <msg>" with newline=true
    // We emit without the carriage-return prefix since we have no spinner to
    // overwrite, but we preserve the bracket-check format for recognisability.
    host.log.info(
      `${kSpinnerCompleteContainerOpen}${kSpinnerCompleteChar}${kSpinnerCompleteContainerClose} ${message}`,
    );
  }

  async function withSpinner<T>(
    options: {
      message: string | (() => string);
      doneMessage?: string | boolean;
    },
    fn: () => Promise<T>,
  ): Promise<T> {
    // Resolve the start message (Q1 supports a thunk)
    const startMsg =
      typeof options.message === "function"
        ? options.message()
        : options.message;

    // Emit start (neutral — no ANSI animation)
    host.log.info(startMsg);

    // Clear terminal line if the host supports it (no-op in tests)
    host.log.clearLine?.();

    // Run the operation
    const result = await fn();

    // Emit completion message
    const doneMessage = options.doneMessage;
    if (typeof doneMessage === "string") {
      completeMessage(doneMessage);
    } else if (doneMessage !== false) {
      // Default: mirror Q1's "cancel(options.doneMessage)" where doneMessage
      // is undefined → show the status message as the completion text.
      completeMessage(startMsg);
    }
    // if doneMessage === false: suppress completion output (Q1 semantics)

    return result;
  }

  return { info, warning, error, withSpinner, completeMessage };
}
