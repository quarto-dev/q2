// sends a request (from the preview in the iframe) to the authoring app
// with type `'url'` and expects a single response with type `'url_reponse'`.
const requestVFS = (path: string) => {
    const ret = new Promise((resolve, reject) => {
        const handleMessage = (event: MessageEvent) => {
            if (event.data.type === 'url_response') {
                window.removeEventListener('message', handleMessage);

                if (event.data.success === true) resolve(event.data.content)
                else reject(event.data.error)
            }
        };
        window.addEventListener('message', handleMessage);
    })
    window.parent.postMessage({ type: 'url', path }, '*');
    return ret
}

export const init = async () => {
    if ('serviceWorker' in navigator && navigator.serviceWorker.controller === null) {
        try {
            const registration = await navigator.serviceWorker.register('serviceWorker.js')
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
        // set up communication with SW
        navigator.serviceWorker.addEventListener('message', async (event) => {
            console.log('fulfilling request', event.data)
            if (event.data?.type === 'request') {
                const modifiedUrl = event.data.url.split('/').at(-1)

                console.log('got request in preview', modifiedUrl)
                const content = await requestVFS(modifiedUrl)
                navigator.serviceWorker.controller!.postMessage({ type: 'response', content, url: event.data.url })
            }
        });
    }
}