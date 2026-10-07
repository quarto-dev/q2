// A Map-backed CacheStorage double for the pandoc loader tests (hooks for failures and a gated `put`).
export class FakeCache {
  entries = new Map<string, Uint8Array>();
  putGate: Promise<void> | null = null;
  putError: Error | null = null;
  async match(key: string) {
    const v = this.entries.get(key);
    return v ? new Response(v.slice()) : undefined;
  }
  async put(key: string, res: Response) {
    if (this.putError) throw this.putError;
    const bytes = new Uint8Array(await res.arrayBuffer());
    if (this.putGate) await this.putGate;
    this.entries.set(key, bytes);
  }
  async delete(key: string | { url: string }) {
    return this.entries.delete(typeof key === 'string' ? key : key.url);
  }
  async keys() {
    return [...this.entries.keys()].map((url) => ({ url }));
  }
}
export const fakeStorage = (cache: FakeCache): CacheStorage => ({ open: async () => cache }) as unknown as CacheStorage;

