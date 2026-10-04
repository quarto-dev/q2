// rtf.js ships no `exports` map and its types sit under `dist/src`; we import the two small renderer
// bundles directly (never the package root, which pulls in the 2.1 MB RTF bundle). Both are UMD wrappers.
declare module 'rtf.js/dist/EMFJS.bundle.min.js' {
  export const Renderer: new (blob: ArrayBuffer) => {
    render(info: { width: string; height: string; wExt: number; hExt: number; xExt: number; yExt: number; mapMode: number }): SVGElement;
  };
}
declare module 'rtf.js/dist/WMFJS.bundle.min.js' {
  export const Renderer: new (blob: ArrayBuffer) => {
    render(info: { width: string; height: string; xExt: number; yExt: number; mapMode: number }): SVGElement;
  };
}
