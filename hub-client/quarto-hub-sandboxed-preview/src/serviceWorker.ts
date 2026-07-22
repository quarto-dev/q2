/// <reference lib="webworker" />
declare var self: ServiceWorkerGlobalScope;

console.log('sandboxed preview service worker started')

const clearCache = async (cache: Cache) => Promise.all((await cache.keys()).map(k => cache.delete(k)))

// Cache sandboxed preview for offline
const precacheFilepaths = ['./', './serviceWorker.js']
const cacheName = 'previewOffline'
self.addEventListener("install", async (e) => {
    e.waitUntil(
        (async () => {
            const cache = await caches.open(cacheName);
            await clearCache(cache)
            await cache.addAll(precacheFilepaths);
        })(),
    );
    console.log('done installing sandboxed preview service worker')
    //console.log('heres the precache:', await (await caches.open(cacheName)).keys())
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
    return bytes
}

const getMimeType = (filename: string) => {
    const ext = filename.split('.').pop()?.toLowerCase();
    const mimeTypes: { [key: string]: string } = {
        'png': 'image/png',
        'jpg': 'image/jpeg',
        'jpeg': 'image/jpeg',
        'gif': 'image/gif',
        'svg': 'image/svg+xml',
        'webp': 'image/webp',
        'ico': 'image/x-icon',
        'pdf': 'application/pdf',
        'html': 'text/html',
        'css': 'text/css',
        'js': 'application/javascript',
        'json': 'application/json',
        'wasm': 'application/wasm',
        'ttf': 'font/ttf',
        'woff': 'font/woff',
        'woff2': 'font/woff2',
    };
    return mimeTypes[ext || ''] || 'application/octet-stream';
};

// forward fetch request over postmessage
const requestIframeToRequestFromVFS = (event: FetchEvent) => new Promise<Response>(async resolve => {
    const url = event.request.url
    const client = await self.clients.get(event.clientId)

    // wait for a single response
    const listener = (event: ExtendableMessageEvent) => {
        const data = event.data
        if (data.type === 'response' && data.url === url) {
            console.log("RESOLVING TO", data)
            resolve(new Response(
                base64StrToBinary(data.content),
                {
                    status: 200,
                    headers: { 'Content-Type': getMimeType(url) }
                }
            ))
            self.removeEventListener('message', listener)
        }
    }
    self.addEventListener('message', listener);
    client!.postMessage({ type: 'request', url })
})

// TODO: unify this list of extensions with `isBinary` in Q2SandboxedPreviewIframe.tsx. 
// We should have a standard way to decide whether or not the preview should be allowed 
// resolve an asset from the VFS.
const shouldRequestFromVFS = (url: string) => url.endsWith('gif') || url.endsWith('png') || url.endsWith('jpg')

self.addEventListener('fetch', function (event) {
    console.log("REQUEST:", event.clientId, event.request.url);
    if (event.request.method !== 'GET') return;

    if (shouldRequestFromVFS(event.request.url)) {
        event.respondWith(requestIframeToRequestFromVFS(event))
    } else { // cache asset or fetch cached asset
        event.respondWith(
            (async () => {
                const r = await caches.match(event.request);
                console.log(`[Service Worker] Fetching resource: ${event.request.url} ${r}`);
                if (r) {
                    return r;
                }
                const response = await fetch(event.request);
                const cache = await caches.open(cacheName);
                console.log(`[Service Worker] Caching new resource: ${event.request.url}`, event.request);
                cache.put(event.request, response.clone());
                return response;
            })(),
        );
    }
})
