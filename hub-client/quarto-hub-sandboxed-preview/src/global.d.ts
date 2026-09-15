// Vite's `?raw` suffix imports resolve to the file contents as a string
// at build time (same declaration hub-client and preview-renderer use).
declare module '*?raw' {
  const src: string;
  export default src;
}
