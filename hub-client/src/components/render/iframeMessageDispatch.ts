/**
 * Shared message dispatcher for the q2-debug and q2-preview iframe
 * entries.
 *
 * The dispatcher coordinates three message kinds the parent
 * (`Q2DebugIframe` / `Q2PreviewIframe`) posts:
 *
 *  - `LOAD_CUSTOM_COMPONENTS` — async, transpiled user-TSX modules
 *    are imported and merged into the iframe-side custom registry.
 *  - `UPDATE_AST` — incoming AST JSON to render. Must not run before
 *    the in-flight load (if any) has finished, otherwise the user's
 *    TSX overrides would not yet be available to the dispatcher
 *    chain and the AST would render against the pre-override
 *    registry.
 *  - `UPDATE_THEME` — q2-preview only; routed through `applyTheme`
 *    when supplied.
 *
 * History — the previous in-line implementation gated `UPDATE_AST`
 * on a `componentsLoading: boolean` flag using a 50-ms polling
 * `setInterval`. Each waiter spawned its own interval, so two
 * `UPDATE_AST` messages queued while the load was in flight could
 * resolve in the wrong order — one waiter's interval phase happened
 * to align such that the *second-arrived* message fired first and
 * the *first-arrived* message overwrote it. In the attribution
 * pipeline this manifested as the no-attribution AST clobbering the
 * with-attribution AST, so the Attribution colouring never appeared
 * on first render for large files with `render-components: -
 * html.tsx`. See `iframeMessageDispatch.test.ts` for the
 * deterministic reproduction.
 *
 * This module replaces the polling with a single shared promise
 * (`pendingLoad`). Every `UPDATE_AST` handler `await`s the same
 * promise, so when it resolves, the waiter continuations are
 * scheduled as microtasks in FIFO insertion order — which is the
 * order the messages arrived. Message ordering is then preserved
 * deterministically without timer-phase sensitivity.
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
    cssUrl: string | null;
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
     * q2-preview only — imperatively applies (or clears) the theme
     * stylesheet `<link>` in `document.head`. Omitted by q2-debug,
     * which has no theme channel.
     */
    applyTheme?: (cssUrl: string | null) => void;
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
            handlers.applyTheme?.(message.cssUrl);
        }
    };
}
