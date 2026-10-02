export function jsFetchUrl(url: string): Promise<string>;
export function assertFetchableUrl(url: string): void;
export function jsFetchUrlHardened(
  url: string,
  maxBytes: number,
  timeoutMs: number,
  signal?: AbortSignal,
): Promise<string>;
