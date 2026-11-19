/**
 * Initialize the Automerge wasm once. Slim entrypoints compile no wasm at
 * module load; the wasm ships as a base64 string module and is initialized
 * asynchronously on first use, which keeps activation instant and avoids the
 * synchronous WebAssembly compilation that Electron forbids in some process
 * types. No `vscode` import: the headless peer and the tests use it too.
 */
import { initializeBase64Wasm, isWasmInitialized } from '@automerge/automerge/slim';
import { automergeWasmBase64 } from '@automerge/automerge/automerge.wasm.base64';

export async function ensureWasm(): Promise<void> {
  if (isWasmInitialized()) return;
  await initializeBase64Wasm(automergeWasmBase64);
}
