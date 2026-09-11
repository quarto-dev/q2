/**
 * Message dispatcher for the sandboxed preview iframe entry.
 *
 * Adapted copy of hub-client's
 * `src/components/render/iframeMessageDispatch.ts` (the promise-ordered
 * dispatcher q2-debug uses), with one protocol difference: the sandboxed
 * `UPDATE_THEME` carries the compiled CSS **text** rather than a
 * parent-minted blob URL — blob URLs are scoped to the origin that
 * created them, so a cross-origin iframe could never fetch one. The
 * iframe mints its own blob URL from the text (see entry.tsx).
 *
 * The dispatcher coordinates three message kinds the parent posts:
 *
 *  - `LOAD_CUSTOM_COMPONENTS` — async, transpiled user-TSX modules
 *    are imported and merged into the iframe-side custom registry.
 *  - `UPDATE_AST` — incoming AST JSON to render. Must not run before
 *    the in-flight load (if any) has finished, otherwise the user's
 *    TSX overrides would not yet be available to the dispatcher
 *    chain and the AST would render against the pre-override
 *    registry.
 *  - `UPDATE_THEME` — routed through `applyTheme`.
 *
 * Every `UPDATE_AST` handler awaits the same shared `pendingLoad`
 * promise, so waiter continuations resolve in FIFO arrival order —
 * deterministic, unlike the 50ms-polling gate this replaces (see the
 * original module's history note for the misordering it caused).
 */

export interface LoadCustomComponentsMessage {
    type: 'LOAD_CUSTOM_COMPONENTS';
    componentsCode: Record<string, string>;
}

export interface UpdateAstMessage {
    type: 'UPDATE_AST';
    payload: unknown;
}

export interface UpdateThemeMessage {
    type: 'UPDATE_THEME';
    cssText: string | null;
}

export type IframeMessage =
    | LoadCustomComponentsMessage
    | UpdateAstMessage
    | UpdateThemeMessage;

export interface IframeMessageHandlers {
    /**
     * Imports user-TSX modules and merges them into the iframe's
     * custom registry. Must be idempotent — the parent re-sends
     * `LOAD_CUSTOM_COMPONENTS` whenever its `customComponentsCode`
     * reference changes.
     */
    loadCustomComponents: (
        componentsCode: Record<string, string>,
    ) => Promise<void>;
    /** Applies a new AST payload to the iframe's React root. */
    updateAst: (payload: unknown) => void;
    /**
     * Imperatively applies (or clears) the theme stylesheet in
     * `document.head` from the posted CSS text.
     */
    applyTheme?: (cssText: string | null) => void;
}

export type IframeMessageDispatcher = (
    message: IframeMessage,
) => Promise<void>;

/**
 * Construct an iframe message dispatcher closed over the supplied
 * handlers. The returned function is a single message-listener
 * callback suitable for `window.addEventListener('message', …)`
 * (after pulling `event.data` out of the MessageEvent).
 */
export function makeIframeMessageDispatcher(
    handlers: IframeMessageHandlers,
): IframeMessageDispatcher {
    // Holds the promise for the currently in-flight
    // loadCustomComponents call (or `null` when no load is pending).
    // Every UPDATE_AST handler `await`s this reference; FIFO microtask
    // ordering on a single shared promise is what guarantees message
    // arrival order is preserved.
    let pendingLoad: Promise<void> | null = null;

    return async function dispatch(message) {
        if (message.type === 'LOAD_CUSTOM_COMPONENTS') {
            const load = handlers.loadCustomComponents(message.componentsCode);
            pendingLoad = load;
            try {
                await load;
            } finally {
                // Only clear when no newer load has replaced ours.
                // Without this guard, a second LOAD that started while
                // the first was still in flight would lose its
                // pendingLoad pointer when the first settled, and any
                // UPDATE_AST queued for the second load would run
                // before it finished.
                if (pendingLoad === load) {
                    pendingLoad = null;
                }
            }
        } else if (message.type === 'UPDATE_AST') {
            if (pendingLoad) {
                await pendingLoad;
            }
            handlers.updateAst(message.payload);
        } else if (message.type === 'UPDATE_THEME') {
            handlers.applyTheme?.(message.cssText);
        }
    };
}
