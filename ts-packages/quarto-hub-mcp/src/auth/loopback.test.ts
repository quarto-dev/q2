/**
 * Loopback redirect listener.
 *
 * Drives the real `http.Server` over `127.0.0.1` with `node:http`
 * requests (so the `Host` header can be forged for the DNS-rebinding
 * test, which `fetch` forbids).
 */

import * as http from 'node:http';
import { describe, it, expect } from 'vitest';

import {
  LoopbackAbortedError,
  LoopbackAuthorizationError,
  LoopbackStateMismatchError,
  LoopbackTimeoutError,
  startLoopbackListener,
} from './loopback.js';

interface RawResponse {
  status: number;
  headers: http.IncomingHttpHeaders;
  body: string;
}

function httpGet(
  port: number,
  pathQuery: string,
  headers: http.OutgoingHttpHeaders = {},
): Promise<RawResponse> {
  return new Promise((resolve, reject) => {
    const req = http.request(
      { host: '127.0.0.1', port, path: pathQuery, method: 'GET', headers },
      (res) => {
        let body = '';
        res.setEncoding('utf8');
        res.on('data', (c) => (body += c));
        res.on('end', () =>
          resolve({ status: res.statusCode ?? 0, headers: res.headers, body }),
        );
      },
    );
    req.on('error', reject);
    req.end();
  });
}

function delay(ms: number): Promise<'pending'> {
  return new Promise((r) => setTimeout(() => r('pending'), ms));
}

describe('startLoopbackListener', () => {
  it('resolves with the code and serves the security-headed success page', async () => {
    const listener = await startLoopbackListener({ expectedState: 'st-123' });
    expect(listener.redirectUri).toBe(`http://127.0.0.1:${listener.port}/callback`);

    const [resp, result] = await Promise.all([
      httpGet(listener.port, '/callback?code=abc&state=st-123'),
      listener.result,
    ]);

    expect(result.code).toBe('abc');
    expect(result.state).toBe('st-123');
    expect(resp.status).toBe(200);
    expect(resp.headers['content-security-policy']).toContain("default-src 'none'");
    expect(resp.headers['cache-control']).toBe('no-store');
    expect(resp.headers['referrer-policy']).toBe('no-referrer');
    expect(resp.headers['x-content-type-options']).toBe('nosniff');
    // No JavaScript and no external resources in the page.
    expect(resp.body).not.toContain('<script');
    expect(resp.body).not.toMatch(/src=|href=/);
  });

  it('rejects on state mismatch with a 400', async () => {
    const listener = await startLoopbackListener({ expectedState: 'good' });
    const respP = httpGet(listener.port, '/callback?code=abc&state=bad');
    await expect(listener.result).rejects.toBeInstanceOf(LoopbackStateMismatchError);
    expect((await respP).status).toBe(400);
  });

  it('rejects a forged Host header (DNS-rebinding) before completing the flow', async () => {
    const listener = await startLoopbackListener({ expectedState: 'st' });
    // Forged Host carries an otherwise-valid code+state; it must NOT be
    // able to complete the flow — the Host check fires first.
    const forged = await httpGet(listener.port, '/callback?code=abc&state=st', {
      host: 'evil.example',
    });
    expect(forged.status).toBe(400);

    // Listener is still bound: the result has not settled.
    const race = await Promise.race([listener.result.then(() => 'settled'), delay(50)]);
    expect(race).toBe('pending');

    // A legitimate callback still completes it.
    const [resp, result] = await Promise.all([
      httpGet(listener.port, '/callback?code=real&state=st'),
      listener.result,
    ]);
    expect(result.code).toBe('real');
    expect(resp.status).toBe(200);
  });

  it('rejects with a typed authorization error on ?error=', async () => {
    const listener = await startLoopbackListener({ expectedState: 'st' });
    const respP = httpGet(listener.port, '/callback?error=access_denied&state=st');
    const err = await listener.result.catch((e) => e);
    expect(err).toBeInstanceOf(LoopbackAuthorizationError);
    expect((err as LoopbackAuthorizationError).oauthError).toBe('access_denied');
    expect((await respP).status).toBe(200);
  });

  it('rejects when the callback carries neither code nor error', async () => {
    const listener = await startLoopbackListener({ expectedState: 'st' });
    const respP = httpGet(listener.port, '/callback?state=st');
    await expect(listener.result).rejects.toBeInstanceOf(LoopbackAuthorizationError);
    expect((await respP).status).toBe(400);
  });

  it('404s an unknown path without echoing it and stays bound', async () => {
    const listener = await startLoopbackListener({ expectedState: 'st' });
    const resp = await httpGet(listener.port, '/secret-probe?x=1');
    expect(resp.status).toBe(404);
    expect(resp.body).not.toContain('secret-probe');
    const race = await Promise.race([listener.result.then(() => 'settled'), delay(30)]);
    expect(race).toBe('pending');
    listener.close();
    await listener.result.catch(() => undefined);
  });

  it('rejects with LoopbackTimeoutError after the deadline', async () => {
    const listener = await startLoopbackListener({ expectedState: 'st', timeoutMs: 40 });
    await expect(listener.result).rejects.toBeInstanceOf(LoopbackTimeoutError);
  });

  it('rejects with LoopbackAbortedError when the signal fires', async () => {
    const ac = new AbortController();
    const listener = await startLoopbackListener({ expectedState: 'st', signal: ac.signal });
    ac.abort();
    await expect(listener.result).rejects.toBeInstanceOf(LoopbackAbortedError);
  });

  it('tears down on SIGINT and unregisters its own signal handler', async () => {
    const prior = process.listeners('SIGINT');
    process.removeAllListeners('SIGINT');
    try {
      const listener = await startLoopbackListener({ expectedState: 'st' });
      const rejected = expect(listener.result).rejects.toBeInstanceOf(LoopbackAbortedError);
      process.emit('SIGINT');
      await rejected;
      expect(process.listeners('SIGINT').length).toBe(0);
    } finally {
      for (const l of prior) process.on('SIGINT', l as never);
    }
  });
});
