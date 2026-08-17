/**
 * The `auth_error` redirect parameter: reading it, and turning it into
 * something worth showing a user.
 *
 * `POST /auth/callback` can fail about a dozen ways. The hub collapses
 * them into four coarse reasons, because the reason lands in a URL the
 * user can read and anyone can craft — the fine distinctions (tampered
 * blob, `kid` mismatch, nonce mismatch, which claim was wrong) stay in
 * the hub's audit log. The reason is only ever a lookup key here; it is
 * never rendered.
 */

/** Copy for a reason the hub did not send, or one we do not recognize. */
const RESTART_COPY = "Sign-in didn't complete. Please try again.";

/**
 * A `Map`, not an object literal, precisely because the key comes from a
 * craftable URL: `({})['__proto__']` is truthy and is not copy at all,
 * so an object lookup would slip past the fallback below.
 */
const COPY = new Map<string, string>([
  ['stale_client', 'This app is out of date and updating. Please try again in a few minutes.'],
  ['restart', RESTART_COPY],
  ['denied', 'Sign-in failed. Your account is not authorized to access this hub.'],
  ['server', 'Something went wrong on the hub. Please try again shortly.'],
]);

/**
 * Read the `auth_error` parameter out of a query string.
 *
 * Presence and value are separate questions. A hub predating the reason
 * codes — or a cached redirect — emits a bare `/?auth_error`, for which
 * `.get()` returns `''`; a truthiness check on the value would drop the
 * error entirely and show the normal sign-in prompt. `undefined` is
 * returned only when the parameter is genuinely absent.
 */
export function readAuthErrorReason(search: string): string | undefined {
  const params = new URLSearchParams(search);
  if (!params.has('auth_error')) return undefined;
  return params.get('auth_error') ?? '';
}

/**
 * Copy for a reason, falling back to the retry sentence.
 *
 * Unknown or empty means client/server skew, and retry is the safer
 * default: a false "try again" costs one retry, whereas a false "not
 * authorized" sends a user to an administrator over nothing — and would
 * let any `/?auth_error=anything` link render that sentence. A real
 * refusal is never hidden by this: `denied` is in the vocabulary from day
 * one, so skew can only affect reasons added later.
 */
export function authErrorMessage(reason: string): string {
  return COPY.get(reason) ?? RESTART_COPY;
}
