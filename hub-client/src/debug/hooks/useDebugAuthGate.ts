import { useEffect, useState } from 'react'
import { fetchAuthMe, type AuthState } from '../../services/authService'

export type DebugAuthGateState =
  | { state: 'checking'; user?: undefined; reason?: undefined }
  | { state: 'authed'; user: AuthState; reason?: undefined }
  | { state: 'anon'; user?: undefined; reason?: undefined }
  | { state: 'unverified'; user?: undefined; reason: string }

/**
 * True for loopback hosts (local dev / `npm run local-prod`). Used to relax
 * the auth gate: a local hub run with `--allow-insecure-auth` still returns
 * 401 from `/auth/me` (no session), which is indistinguishable from an
 * auth-enforcing hub. On loopback we assume the former and let the inspector
 * open; on a real domain we keep the strict sign-in gate.
 */
export function isLoopbackHost(hostname: string): boolean {
  return (
    hostname === 'localhost' ||
    hostname === '127.0.0.1' ||
    hostname === '::1' ||
    hostname === '[::1]' ||
    hostname.endsWith('.localhost')
  )
}

/**
 * Reports whether the browser is authenticated with the hub server
 * (HttpOnly cookie present and accepted).
 *
 * Terminal states:
 * - `authed` — `/auth/me` returned a user (200). Enter the inspector.
 * - `anon` — `/auth/me` returned 401/403 on a **non-loopback** host. The
 *   hub enforces auth and the user must sign in via the main app first.
 *   Show the gate.
 * - `unverified` — either `/auth/me` failed for some other reason (500, 404,
 *   network error — the normal case for auth-less deployments where the
 *   endpoint may not exist), **or** it returned 401/403 on a loopback host
 *   (local-prod / dev, where the hub runs with `--allow-insecure-auth` and
 *   401 just means "no session"). Enter the inspector; the sync server is
 *   the real boundary and will refuse the connection if it enforces auth.
 *
 * The debug page intentionally does not initiate sign-in itself; that
 * remains the main app's responsibility.
 */
export function useDebugAuthGate(): DebugAuthGateState {
  const [gate, setGate] = useState<DebugAuthGateState>({ state: 'checking' })

  useEffect(() => {
    let cancelled = false
    fetchAuthMe()
      .then((user) => {
        if (cancelled) return
        if (user) {
          setGate({ state: 'authed', user })
        } else if (isLoopbackHost(window.location.hostname)) {
          // Local/dev hub with auth disabled — 401 is expected; proceed.
          setGate({
            state: 'unverified',
            reason: 'Local hub with auth disabled (loopback host): /auth/me returned 401. Proceeding read-only.',
          })
        } else {
          setGate({ state: 'anon' })
        }
      })
      .catch((err: unknown) => {
        if (cancelled) return
        const reason = err instanceof Error ? err.message : String(err)
        setGate({ state: 'unverified', reason })
      })
    return () => {
      cancelled = true
    }
  }, [])

  return gate
}
