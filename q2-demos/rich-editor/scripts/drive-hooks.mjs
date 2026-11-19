// Resolve hook for `scripts/drive.mjs`: a bare import that can't be found
// from the importing file is resolved from this package instead.

const fallbackParent = new URL('../package.json', import.meta.url).href

export async function resolve(specifier, context, nextResolve) {
  try {
    return await nextResolve(specifier, context)
  } catch (e) {
    const bare = !/^(\.|\/|[a-z][a-z0-9+.-]*:)/i.test(specifier)
    if (!bare || e?.code !== 'ERR_MODULE_NOT_FOUND') throw e
    return nextResolve(specifier, { ...context, parentURL: fallbackParent })
  }
}
