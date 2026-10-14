/**
 * Actionable message for engine extensions that still use the `Deno`
 * namespace. q2 runs engines under Node, where `Deno` is not defined; the raw
 * `ReferenceError: Deno is not defined` names neither the extension nor the
 * fix.
 */

/** Where the migration note for third-party engines lives. */
export const MIGRATION_DOC = "docs/guides/extensions/ts-engines-node.qmd";

const DENO_NOT_DEFINED = /\bDeno is not defined\b/;

/** First file path mentioned in a stack trace (`file://…` URL or absolute path). */
function extensionPathFromStack(stack: string | null | undefined): string | undefined {
  if (!stack) return undefined;
  const url = /file:\/\/\/[^\s)]+?(?=:\d+:\d+\)?|\s|$)/.exec(stack);
  if (url) {
    try {
      return decodeURIComponent(new URL(url[0]).pathname);
    } catch {
      return url[0];
    }
  }
  return /\(?((?:\/|[A-Za-z]:\\)[^\s():]+):\d+:\d+\)?/.exec(stack)?.[1];
}

/**
 * Rewrite a `Deno is not defined` failure into an actionable message; return
 * `undefined` for any other error. `knownPath` is the engine path when the
 * failure happened while loading it; otherwise the path is recovered from the
 * stack.
 */
export function rewriteDenoError(
  err: unknown,
  knownPath?: string,
): { message: string; stack: string | null } | undefined {
  const e = err as { message?: string; stack?: string } | null | undefined;
  const text = String(e?.message ?? err);
  if (!DENO_NOT_DEFINED.test(text)) return undefined;
  const path = knownPath ?? extensionPathFromStack(e?.stack);
  const where = path ? `engine extension ${path}` : "an engine extension";
  return {
    message:
      `${where} uses the \`Deno\` namespace, which q2 does not provide: ` +
      `q2 runs TypeScript engines under Node. Replace \`Deno.*\` calls with ` +
      `\`node:\` equivalents (fs, child_process, path, process); see ${MIGRATION_DOC}. ` +
      `Original error: ${text}`,
    stack: e?.stack ?? null,
  };
}
