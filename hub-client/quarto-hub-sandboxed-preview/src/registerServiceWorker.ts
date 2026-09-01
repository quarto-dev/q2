/**
 * Page-side half of the asset-proxy chain: registers the service worker
 * and bridges its VFS requests to the parent window.
 *
 *   SW  ──{type:'request', id, vfsPath}──▶  this page
 *   this page  ──{type:'url', id, path}──▶  parent (Q2SandboxedPreviewIframe)
 *   parent  ──{type:'url_response', id, …}──▶  this page
 *   this page  ──{type:'response', id, …}──▶  SW controller
 *
 * Requests are correlated by id (never by path/url — two concurrent
 * fetches of the same path must not steal each other's responses), and
 * every await has a timeout so a lost message can't leak a listener.
 */

interface UrlResponseMessage {
    type: 'url_response';
    id: string;
    path: string;
    success: boolean;
    content?: string;
    error?: string;
    isBinary?: boolean;
}

const REQUEST_TIMEOUT_MS = 10_000;

/** Ask the parent for a VFS file; resolves with the url_response fields. */
const requestVFS = (id: string, path: string): Promise<UrlResponseMessage> =>
    new Promise((resolve, reject) => {
        const handleMessage = (event: MessageEvent) => {
            const data = event.data as UrlResponseMessage | undefined;
            if (data?.type === 'url_response' && data.id === id) {
                window.removeEventListener('message', handleMessage);
                clearTimeout(timeout);
                resolve(data);
            }
        };
        const timeout = setTimeout(() => {
            window.removeEventListener('message', handleMessage);
            reject(new Error(`VFS request timed out: ${path}`));
        }, REQUEST_TIMEOUT_MS);
        window.addEventListener('message', handleMessage);
        window.parent.postMessage({ type: 'url', id, path }, '*');
    });

export const init = async () => {
    if ('serviceWorker' in navigator && navigator.serviceWorker.controller === null) {
        try {
            const registration = await navigator.serviceWorker.register('serviceWorker.js');
            // wait for page to be claimed so that the controller is working
            await new Promise((resolve) => {
                navigator.serviceWorker.addEventListener('controllerchange', resolve, { once: true });
            });
            console.log('ServiceWorker registration successful with scope: ', registration.scope);
        } catch (err) {
            console.log('ServiceWorker registration failed: ', err);
        };
    }

    // this should be guaranteed by the setup above
    if (navigator.serviceWorker.controller) {
        // Relay SW requests to the parent and responses back to the SW.
        navigator.serviceWorker.addEventListener('message', async (event) => {
            const data = event.data as
                | { type: 'request'; id: string; vfsPath: string }
                | undefined;
            if (data?.type !== 'request') return;

            try {
                const response = await requestVFS(data.id, data.vfsPath);
                navigator.serviceWorker.controller!.postMessage({
                    type: 'response',
                    id: data.id,
                    success: response.success,
                    content: response.content,
                    error: response.error,
                    isBinary: response.isBinary,
                });
            } catch (err) {
                navigator.serviceWorker.controller!.postMessage({
                    type: 'response',
                    id: data.id,
                    success: false,
                    error: err instanceof Error ? err.message : String(err),
                });
            }
        });
    }
}
