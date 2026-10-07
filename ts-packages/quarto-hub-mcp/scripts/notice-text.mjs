/**
 * The third-party notices text shipped in the npm tarball (CAP-15) and
 * the `.mcpb` one-click bundle — one source so the two channels can't
 * drift. `vendoredKeyring` describes how the keyring addon travels:
 * inside the payload (`.mcpb`, like the release tarball) or as an npm
 * dependency resolved at install time.
 */
export function noticeText({ vendoredKeyring }) {
  const keyring = vendoredKeyring
    ? `\`@napi-rs/keyring\` (MIT) ships inside \`dist-bundle/node_modules/@napi-rs\`
with a native addon build for every supported platform.`
    : `\`@napi-rs/keyring\` (MIT) is a runtime dependency with a per-platform
native addon; npm installs the build for your platform.`;
  return `# Quarto Hub MCP server — third-party notices

Copyright (c) Posit, PBC. MIT licensed (see LICENSE).

This package's \`dist-bundle/\` inlines code from the following third-party
packages at build time (esbuild). Each is distributed under its own license
(MIT at the versions bundled); consult each project for full terms. License
texts preserved by the bundler are collected at the end of
\`dist-bundle/index.mjs\`.

- @modelcontextprotocol/server, @modelcontextprotocol/core (MIT)
- @automerge/automerge (MIT)
- jose (MIT)
- oauth4webapi (MIT)
- ws (MIT)
- zod (MIT)

${keyring}

\`dist-bundle/node_modules/wasm-qmd-parser\` is built from this repository
(the Quarto 2 qmd parser, MIT, same copyright) and ships inside the bundle
because it is not independently published.
`;
}
