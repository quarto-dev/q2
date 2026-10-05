export const MAX_INPUT_BYTES: number;
export const TIMEOUT_MS: number;
export function setRasterizer(
  fn: ((svg: Uint8Array, maxSide: number) => Promise<Uint8Array>) | null,
): void;
export function jsCanRasterizeSvg(): boolean;
export function intrinsicSize(root: Element): { width: number; height: number; viewBox: boolean };
export function targetSize(
  size: { width: number; height: number },
  maxSide: number,
): { width: number; height: number };
export function withPhys(png: Uint8Array): Uint8Array;
export function jsRasterizeSvg(
  svg: Uint8Array,
  maxSide: number,
  signal?: AbortSignal,
): Promise<Uint8Array>;
