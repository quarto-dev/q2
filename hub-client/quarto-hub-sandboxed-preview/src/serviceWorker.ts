/// <reference lib="webworker" />
declare var self: ServiceWorkerGlobalScope;

import {
    isBinaryPath,
    mimeTypeFor,
    vfsPathForRequestUrl,
} from './assetPolicy';

// Caching/offline support disabled for now (commit 103af4445) — the
// service worker only proxies document assets out of the parent's VFS.
// Everything outside the __q2_vfs__ namespace (the page, assets/*,
// serviceWorker.js, KaTeX fonts) falls through to the network.

self.addEventListener('install', () => {
    // Activate updated workers without waiting for old clients to close.
    void self.skipWaiting();
});

self.addEventListener('activate', (e) => {
    e.waitUntil(self.clients.claim());
});

const base64StrToBinary = (base64Data: string) => {
    const binaryString = atob(base64Data);
    const bytes = new Uint8Array(binaryString.length);
    for (let i = 0; i < binaryString.length; i++) {
        bytes[i] = binaryString.charCodeAt(i);
    }
    return bytes;
};

interface VfsResponseMessage {
    type: 'response';
    id: string;
    success: boolean;
    content?: string;
    error?: string;
    isBinary?: boolean;
}

/** In-flight proxy requests, keyed by request id. */
const pending = new Map<string, (msg: VfsResponseMessage) => void>();

// One persistent listener resolves whichever request a response belongs
// to — the old implementation registered (and could leak) one listener
// per request.
self.addEventListener('message', (event: ExtendableMessageEvent) => {
    const data = event.data as VfsResponseMessage | undefined;
    if (!data || data.type !== 'response' || typeof data.id !== 'string') return;
    const resolve = pending.get(data.id);
    if (resolve) {
        pending.delete(data.id);
        resolve(data);
    }
});

const REQUEST_TIMEOUT_MS = 10_000;

/**
 * Forward a __q2_vfs__ fetch to the page (which relays it to the parent
 * over postMessage), and synthesize an HTTP response from the bytes that
 * come back.
 */
const proxyVfsRequest = (event: FetchEvent, vfsPath: string): Promise<Response> =>
    new Promise<Response>((resolve) => {
        const id = crypto.randomUUID();

        const timeout = setTimeout(() => {
            pending.delete(id);
            resolve(new Response(`VFS proxy timeout for ${vfsPath}`, { status: 504 }));
        }, REQUEST_TIMEOUT_MS);

        pending.set(id, (msg) => {
            clearTimeout(timeout);
            if (!msg.success || msg.content === undefined) {
                resolve(new Response(msg.error ?? 'Not found in VFS', { status: 404 }));
                return;
            }
            const body: BodyInit = msg.isBinary
                ? base64StrToBinary(msg.content)
                : msg.content;
            resolve(new Response(body, {
                status: 200,
                headers: { 'Content-Type': mimeTypeFor(vfsPath) },
            }));
        });

        void (async () => {
            const client = await self.clients.get(event.clientId);
            if (!client) {
                const entry = pending.get(id);
                pending.delete(id);
                clearTimeout(timeout);
                if (entry) resolve(new Response('No client for VFS proxy request', { status: 502 }));
                return;
            }
            client.postMessage({
                type: 'request',
                id,
                vfsPath,
                isBinary: isBinaryPath(vfsPath),
            });
        })();
    });

self.addEventListener('fetch', function (event) {
    if (event.request.method !== 'GET') return;
    const vfsPath = vfsPathForRequestUrl(event.request.url);
    if (vfsPath === null) return; // app asset / page — network as usual
    event.respondWith(proxyVfsRequest(event, vfsPath));
});
