import { describe, it, expect, vi, afterEach } from "vitest";
import { assertFetchableUrl, jsFetchUrlHardened } from "./fetch.js";

function okResponse(body: Uint8Array, init: Record<string, unknown> = {}) {
  const res = new Response(body, {
    status: 200,
    headers: { "content-type": "image/png", ...(init.headers as object) },
  });
  if (init.url) Object.defineProperty(res, "url", { value: init.url });
  return res;
}

function decode(json: string): { mimeType: string; bytes: number[] } {
  const { mimeType, content } = JSON.parse(json);
  return { mimeType, bytes: [...Buffer.from(content, "base64")] };
}

describe("jsFetchUrlHardened", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("omits credentials and the referrer, and returns the body", async () => {
    const fetchMock = vi.fn(async () => okResponse(new Uint8Array([1, 2, 3])));
    vi.stubGlobal("fetch", fetchMock);
    const out = await jsFetchUrlHardened("https://example.com/a.png", 1000, 5000);
    expect(decode(out)).toEqual({ mimeType: "image/png", bytes: [1, 2, 3] });
    const [url, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit];
    expect(url).toBe("https://example.com/a.png");
    expect(init.credentials).toBe("omit");
    expect(init.referrerPolicy).toBe("no-referrer");
  });

  it.each([
    "a.png",
    "/a.png",
    "./a.png",
    "//example.com/a.png",
    "http://example.com/a.png",
    "data:image/png;base64,AAAA",
    "file:///etc/passwd",
    "https://",
    "",
  ])("refuses %j without calling fetch", async (url) => {
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);
    await expect(jsFetchUrlHardened(url, 1000, 5000)).rejects.toThrow();
    expect(fetchMock).not.toHaveBeenCalled();
    expect(() => assertFetchableUrl(url)).toThrow();
  });

  it("refuses a body over the cap, declared or streamed", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () =>
        okResponse(new Uint8Array(10), { headers: { "content-length": "10" } }),
      ),
    );
    await expect(
      jsFetchUrlHardened("https://example.com/a.png", 5, 5000),
    ).rejects.toThrow(/limit/);
    vi.stubGlobal("fetch", vi.fn(async () => okResponse(new Uint8Array(10))));
    await expect(
      jsFetchUrlHardened("https://example.com/a.png", 5, 5000),
    ).rejects.toThrow(/limit/);
  });

  it("refuses a redirect that ends off https", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () =>
        okResponse(new Uint8Array([1]), { url: "http://example.com/a.png" }),
      ),
    );
    await expect(
      jsFetchUrlHardened("https://example.com/a.png", 1000, 5000),
    ).rejects.toThrow(/non-https/);
  });

  it("times out", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(
        (_url: string, init: RequestInit) =>
          new Promise((_resolve, reject) => {
            init.signal!.addEventListener("abort", () => reject(init.signal!.reason));
          }),
      ),
    );
    await expect(
      jsFetchUrlHardened("https://example.com/a.png", 1000, 20),
    ).rejects.toThrow(/timed out/);
  });

  it("is cancelled by the caller's signal", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(
        (_url: string, init: RequestInit) =>
          new Promise((_resolve, reject) => {
            init.signal!.addEventListener("abort", () => reject(init.signal!.reason));
          }),
      ),
    );
    const click = new AbortController();
    const pending = jsFetchUrlHardened(
      "https://example.com/a.png",
      1000,
      60_000,
      click.signal,
    );
    click.abort(new Error("download cancelled"));
    await expect(pending).rejects.toThrow(/download cancelled/);
    // Already aborted before the call.
    await expect(
      jsFetchUrlHardened("https://example.com/a.png", 1000, 60_000, click.signal),
    ).rejects.toThrow(/download cancelled/);
  });

  it("a non-ok status is an error", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => new Response("no", { status: 404, statusText: "Not Found" })),
    );
    await expect(
      jsFetchUrlHardened("https://example.com/a.png", 1000, 5000),
    ).rejects.toThrow(/404/);
  });
});
