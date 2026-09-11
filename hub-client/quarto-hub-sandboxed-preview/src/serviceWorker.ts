/// <reference lib="webworker" />
declare var self: ServiceWorkerGlobalScope;

import {
    isBinaryPath,
    mimeTypeFor,
    vfsPathForRequestUrl,
} from './assetPolicy';

// Caching/offline support disabled for now (commit 103af4445) — the
// service worker only proxies document assets out of the parent's VFS.
// The frame's own app files (the page, serviceWorker.js, assets/* incl.
// KaTeX fonts) fall through to the network; every other in-scope path is
// a document asset proxied from the VFS (bd-00bgt5cy).

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
 * Forward a document-asset fetch to the page (which relays it to the parent
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
    // Any in-scope relative path is a document asset served from the
    // parent's VFS; only the frame's own app files (the page,
    // serviceWorker.js, assets/*) fall through to the network.
    const vfsPath = vfsPathForRequestUrl(event.request.url, self.registration.scope);
    if (vfsPath === null) return;
    event.respondWith(proxyVfsRequest(event, vfsPath));
});
