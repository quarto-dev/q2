# q2-sandboxed-preview.html Separate Domain Design

## Problem

The `q2-sandboxed-preview.html` iframe renders raw AST JSON in a sandboxed context. For security isolation, it should be served from a separate domain (cross-origin) rather than the same origin as the main hub-client application.

## Solution

### Production Architecture

```
Main app:     https://your-hub-domain.com/
              ├─ Serves the main React app
              ├─ WebSocket connection to sync server
              └─ Contains Q2SandboxedPreviewIframe component

q2-sandboxed-preview:       https://raw.your-hub-domain.com/q2-sandboxed-preview.html
              └─ Single static HTML file
                 ├─ No external dependencies
                 ├─ Inline JavaScript only
                 └─ Communicates via postMessage
```

### Security Benefits

1. **Origin isolation**: q2-sandboxed-preview.html runs in a completely separate origin
2. **No cookie access**: raw domain can't access hub cookies
3. **No localStorage access**: raw domain has separate storage
4. **Minimal attack surface**: Single static file, no build artifacts
5. **CSP enforcement**: Strict CSP on raw domain prevents XSS

### Local Development

For local development, we simulate the separate domain using ports:

```
Main app:     http://127.0.0.1:8080/    (local-prod.sh)
q2-sandboxed-preview:       http://127.0.0.1:8081/    (q2-sandboxed-preview-server.mjs)
```

This mimics the cross-origin setup and allows testing the postMessage communication.

## Implementation Details

### Files

- **`hub-client/q2-sandboxed-preview.html`**: The sandboxed HTML file
- **`scripts/q2-sandboxed-preview-server.mjs`**: Dedicated static server for local-prod
- **`hub-client/src/components/render/q2-sandboxed-preview/Q2SandboxedPreviewIframe.tsx`**: React component that loads the iframe

### Environment Variables

- **`VITE_Q2_SANDBOXED_PREVIEW_URL`**: URL to load q2-sandboxed-preview.html from
  - Dev: `q2-sandboxed-preview.html` (served by Vite from same origin)
  - Local-prod: `http://127.0.0.1:8081/q2-sandboxed-preview.html`
  - Production: `https://raw.your-hub-domain.com/q2-sandboxed-preview.html` (configure as needed)

### Build Configuration

```bash
# Local-prod build
VITE_Q2_SANDBOXED_PREVIEW_URL=http://127.0.0.1:8081/q2-sandboxed-preview.html npm run build

# Production build (use your actual raw domain)
VITE_Q2_SANDBOXED_PREVIEW_URL=https://raw.your-hub-domain.com/q2-sandboxed-preview.html npm run build
```

### Nginx Configuration

#### Main domain

```nginx
server {
    listen 443 ssl http2;
    server_name your-hub-domain.com;
    
    # Normal hub-client serving
    location / {
        root /var/www/hub-client/dist;
        try_files $uri $uri/ /index.html;
    }
}
```

#### Raw subdomain (separate for security isolation)

```nginx
server {
    listen 443 ssl http2;
    server_name raw.your-hub-domain.com;
    
    # Only serve q2-sandboxed-preview.html
    location = /q2-sandboxed-preview.html {
        root /var/www/hub-client/dist;
        
        # Strict security headers
        add_header X-Frame-Options "ALLOWALL" always;
        add_header Content-Security-Policy "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline';" always;
        add_header X-Content-Type-Options "nosniff" always;
        
        # No caching
        add_header Cache-Control "no-cache, no-store, must-revalidate" always;
    }
    
    # Deny all other requests
    location / {
        return 404;
    }
}
```

## Communication Protocol

The iframe communicates with the parent via postMessage:

### Parent → iframe

```typescript
iframe.contentWindow.postMessage({
  type: 'UPDATE_AST',
  payload: { astJson: string }
}, '*')
```

### iframe → Parent

```typescript
// Ready signal
window.parent.postMessage({ type: 'IFRAME_READY' }, '*')

// VFS read request (currently unused by q2-sandboxed-preview, kept for compatibility)
window.parent.postMessage({
  type: 'url',
  path: string
}, '*')
```

## Testing

### Local-prod mode

```bash
# Build with local-prod URL
cd hub-client
npm run build:local-prod

# Start all servers
cd ..
./scripts/local-prod.sh

# Open http://127.0.0.1:8080
# Navigate to a document with q2-sandboxed-preview format
```

### Verification

1. Open browser DevTools → Network tab
2. Find the q2-sandboxed-preview.html request
3. Verify it loads from `http://127.0.0.1:8081`
4. Check Console for postMessage events
5. Verify AST renders correctly

## Deployment Checklist

- [ ] Choose subdomain for raw rendering (e.g., `raw.your-hub-domain.com`)
- [ ] Set up DNS: subdomain → server IP
- [ ] TLS certificate for raw subdomain
- [ ] Nginx config for raw subdomain
- [ ] Build with production URL: `VITE_Q2_SANDBOXED_PREVIEW_URL=https://raw.your-hub-domain.com/q2-sandboxed-preview.html`
- [ ] Deploy q2-sandboxed-preview.html to raw subdomain
- [ ] Test cross-origin postMessage
- [ ] Verify CSP headers
- [ ] Check iframe sandbox attributes

## Future Enhancements

1. **Subdomain isolation for other renderers**: Apply same pattern to q2-debug, q2-preview
2. **CDN deployment**: Serve from CDN for global edge caching
3. **Version pinning**: URL with hash for cache-busting (`q2-sandboxed-preview-abc123.html`)
4. **Multiple raw formats**: Extend pattern to other simple renderers

## References

- MDN: [Window.postMessage()](https://developer.mozilla.org/en-US/docs/Web/API/Window/postMessage)
- MDN: [iframe sandbox attribute](https://developer.mozilla.org/en-US/docs/Web/HTML/Element/iframe#attr-sandbox)
- OWASP: [Clickjacking Defense](https://cheatsheetseries.owasp.org/cheatsheets/Clickjacking_Defense_Cheat_Sheet.html)
