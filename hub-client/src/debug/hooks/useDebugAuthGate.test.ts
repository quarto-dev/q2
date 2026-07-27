/**
 * Unit tests for useDebugAuthGate.
 *
 * The debug page does not initiate sign-in — it only reports whether the
 * user is already authenticated via the HttpOnly cookie.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { renderHook, waitFor } from '@testing-library/react'

vi.mock('../../services/authService', () => ({
  fetchAuthMe: vi.fn(),
}))

import { useDebugAuthGate, isLoopbackHost } from './useDebugAuthGate'
import { fetchAuthMe } from '../../services/authService'

const mockFetchAuthMe = vi.mocked(fetchAuthMe)

describe('isLoopbackHost', () => {
  it('recognizes loopback hosts (local dev / local-prod)', () => {
    expect(isLoopbackHost('localhost')).toBe(true)
    expect(isLoopbackHost('127.0.0.1')).toBe(true)
    expect(isLoopbackHost('::1')).toBe(true)
    expect(isLoopbackHost('[::1]')).toBe(true)
    expect(isLoopbackHost('foo.localhost')).toBe(true)
  })

  it('treats real deployment hosts as non-loopback', () => {
    expect(isLoopbackHost('hub.example.com')).toBe(false)
    expect(isLoopbackHost('quarto-hub.posit.co')).toBe(false)
    expect(isLoopbackHost('192.168.1.10')).toBe(false)
  })
})

describe('useDebugAuthGate', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('starts in the checking state', () => {
    mockFetchAuthMe.mockReturnValue(new Promise(() => {})) // never resolves
    const { result } = renderHook(() => useDebugAuthGate())
    expect(result.current.state).toBe('checking')
    expect(result.current.user).toBeUndefined()
  })

  it('resolves to authed with the user when /auth/me succeeds', async () => {
    const user = { email: 'a@b.com', name: 'A', picture: null }
    mockFetchAuthMe.mockResolvedValue(user)

    const { result } = renderHook(() => useDebugAuthGate())
    await waitFor(() => expect(result.current.state).toBe('authed'))
    expect(result.current.user).toEqual(user)
  })

  it('resolves to anon when /auth/me returns 401 (null) on a real (non-loopback) host', async () => {
    // A deployed hub that enforces auth: 401 means "sign in first".
    vi.stubGlobal('location', { hostname: 'hub.example.com' })
    mockFetchAuthMe.mockResolvedValue(null)

    const { result } = renderHook(() => useDebugAuthGate())
    await waitFor(() => expect(result.current.state).toBe('anon'))
    expect(result.current.user).toBeUndefined()
  })

  it('resolves to unverified when /auth/me returns 401 on a loopback host (auth-disabled local hub)', async () => {
    // `npm run local-prod` runs the hub with `--allow-insecure-auth`, so
    // `/auth/me` returns 401 (no session) even though there is no sign-in to
    // perform. On loopback we proceed into the inspector with a notice rather
    // than dead-ending at the sign-in gate.
    vi.stubGlobal('location', { hostname: '127.0.0.1' })
    mockFetchAuthMe.mockResolvedValue(null)

    const { result } = renderHook(() => useDebugAuthGate())
    await waitFor(() => expect(result.current.state).toBe('unverified'))
    expect(result.current.user).toBeUndefined()
    expect(result.current.reason).toMatch(/auth disabled/i)
  })

  it('resolves to unverified when /auth/me throws (auth-less deployment or proxy broken)', async () => {
    // `/auth/me` returning 500 (as happens when the hub server has no auth
    // system at all) must NOT block the debugger — the user should still
    // be able to use the inspector against an auth-less sync server. The
    // hook records the reason so the UI can display a subtle notice.
    mockFetchAuthMe.mockRejectedValue(new Error('/auth/me failed: 500'))

    const { result } = renderHook(() => useDebugAuthGate())
    await waitFor(() => expect(result.current.state).toBe('unverified'))
    expect(result.current.user).toBeUndefined()
    expect(result.current.reason).toMatch(/500/)
  })

  it('calls fetchAuthMe exactly once on mount', async () => {
    vi.stubGlobal('location', { hostname: 'hub.example.com' })
    mockFetchAuthMe.mockResolvedValue(null)
    const { result } = renderHook(() => useDebugAuthGate())
    await waitFor(() => expect(result.current.state).toBe('anon'))
    expect(mockFetchAuthMe).toHaveBeenCalledTimes(1)
  })
})
