import { describe, expect, it } from 'vitest';
import { readFile } from 'node:fs/promises';
import { parseLocalProdPort } from './local-prod-port.mjs';

describe('parseLocalProdPort', () => {
  it('uses the default port when no flag is provided', () => {
    expect(parseLocalProdPort([])).toBe(8080);
  });

  it('parses an explicit port', () => {
    expect(parseLocalProdPort(['--port', '9000'])).toBe(9000);
  });

  it('rejects a missing or invalid port', () => {
    expect(() => parseLocalProdPort(['--port'])).toThrow();
    expect(() => parseLocalProdPort(['--port', '70000'])).toThrow();
  });

  it('nginx launcher honors --port like the plain launcher', async () => {
    const nginxLauncher = await readFile(new URL('./local-prod-nginx.sh', import.meta.url), 'utf8');
    const nginxConfig = await readFile(new URL('../config/local-nginx.conf', import.meta.url), 'utf8');

    // The nginx launcher parses --port via the shared parser, same as local-prod.sh
    expect(nginxLauncher).toContain('NGINX_PORT="$(node "$SCRIPT_DIR/local-prod-port.mjs" "$@")"');
    expect(nginxLauncher).not.toMatch(/^NGINX_PORT=8080$/m);
    // ...and substitutes it into the generated nginx config
    expect(nginxLauncher).toContain('s|NGINX_PORT|$NGINX_PORT|g');
    // The template carries the placeholder, not a hardcoded main port
    expect(nginxConfig).toContain('listen NGINX_PORT;');
    expect(nginxConfig).not.toContain('listen 8080;');
  });

  it('nginx launcher hub readiness check accepts 401 (auth-enabled setups)', async () => {
    // With OIDC_CLIENT_ID in the environment the hub enables auth, and
    // /health requires credentials: `curl -f` fails on the 401 even though
    // the hub is up. Readiness must mean "any HTTP response", not "2xx".
    const nginxLauncher = await readFile(new URL('./local-prod-nginx.sh', import.meta.url), 'utf8');
    const hubHealth = nginxLauncher.match(/curl [^\n]*\/health[^\n]*/);
    expect(hubHealth).not.toBeNull();
    const healthCurl = hubHealth[0];
    expect(healthCurl).not.toContain(' -f');
  });

  it('keeps the hub proxy on port 3000', async () => {
    const serverScript = await readFile(new URL('./local-prod-server.mjs', import.meta.url), 'utf8');
    const launcherScript = await readFile(new URL('./local-prod.sh', import.meta.url), 'utf8');
    const nginxConfig = await readFile(new URL('../config/local-nginx.conf', import.meta.url), 'utf8');
    const composeConfig = await readFile(new URL('../docker-compose.local-prod.yml', import.meta.url), 'utf8');

    expect(serverScript).toContain("process.env.HUB_PORT || '3000'");
    expect(launcherScript).toContain('HUB_PORT=3000');
    expect(nginxConfig).not.toContain(':3001');
    expect(composeConfig).not.toContain(':3001');
  });
});
