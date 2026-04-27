/**
 * Parse `location.hash` looking for an initial document to seed into the
 * debugger. Recognized form: `#doc=<automerge-url-or-id>`.
 *
 * The hash is a one-shot seed only; once the UI is running, the user
 * manages subscriptions through the normal DocumentList input.
 */
export function parseDebugHashSeed(hash: string): string | null {
  const stripped = hash.startsWith('#') ? hash.slice(1) : hash
  if (!stripped) return null

  const params = new URLSearchParams(stripped)
  const doc = params.get('doc')
  if (!doc) return null
  return doc
}
