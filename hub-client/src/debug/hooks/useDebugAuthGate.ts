import { useEffect, useState } from 'react'
import { fetchAuthMe, type AuthState } from '../../services/authService'

export type DebugAuthGateState =
  | { state: 'checking'; user?: undefined; reason?: undefined }
  | { state: 'authed'; user: AuthState; reason?: undefined }
  | { state: 'anon'; user?: undefined; reason?: undefined }
  | { state: 'unverified'; user?: undefined; reason: string }

/**
 * Reports whether the browser is authenticated with the hub server
 * (HttpOnly cookie present and accepted).
 *
 * Three terminal states:
 * - `authed` — `/auth/me` returned a user (200). Enter the inspector.
 * - `anon` — `/auth/me` returned 401/403. The hub enforces auth and the
 *   user needs to sign in via the main app first. Show the gate.
 * - `unverified` — `/auth/me` failed for some other reason (500, 404,
 *   network error). This is the normal case for hub deployments that run
 *   without authentication — `/auth/me` may not exist at all. Enter the
 *   inspector; the sync server will either accept the connection (auth-
 *   less hub) or refuse it (user will see the disconnect).
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
        if (user) setGate({ state: 'authed', user })
        else setGate({ state: 'anon' })
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
