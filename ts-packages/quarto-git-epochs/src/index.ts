/**
 * Git-aware logic shared by the preview clients and the VS Code extension. Browser-safe: no `node:` import and no
 * `vscode` import. The git subprocess probe is the node-only `./probe` entry.
 * Names come from claude-notes/designs/git-epoch-registry-contract.md; a rename is a change to the contract.
 */
export const GIT_EPOCHS_VERSION = '0.0.1';

export * from './arrival.js';
export * from './classify.js';
export * from './controller.js';
export * from './paths.js';
export * from './probe-types.js';
export * from './registry.js';
export * from './startup.js';
export * from './vocabulary.js';
