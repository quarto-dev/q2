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
