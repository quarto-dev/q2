/**
 * Test doubles of `@quarto/preview-runtime` define the engine's
 * synchronous helpers. The SPA calls their `...Async` forms, which run the
 * engine in the render worker in production and call the synchronous
 * helper in-process otherwise. This adds those async forms to a double,
 * each delegating to the double's synchronous namesake, so assertions on
 * the synchronous mocks keep their meaning.
 */
type Fn = (...args: never[]) => unknown;

export function withAsyncEngineHelpers<T extends Record<string, unknown>>(runtime: T): T {
  const call = (name: string) => (...args: unknown[]) =>
    Promise.resolve().then(() => (runtime[name] as (...a: unknown[]) => unknown)(...args));
  const pairs: Array<[string, string]> = [
    ['resolvePandocFormatsAsync', 'resolvePandocFormats'],
    ['parseQmdContentAsync', 'parseQmdContentSync'],
    ['applyNodeEditAsync', 'applyNodeEdit'],
    ['regenerateNestedBuffersAsync', 'regenerateNestedBuffers'],
  ];
  const added: Record<string, Fn> = {};
  for (const [asyncName, syncName] of pairs) {
    if (syncName in runtime && !(asyncName in runtime)) added[asyncName] = call(syncName) as Fn;
  }
  return { ...runtime, ...added };
}
