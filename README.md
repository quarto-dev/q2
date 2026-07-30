# Quarto 2

[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/quarto-dev/q2)

> **Experimental** - This project is under active development. It's not yet ready for production use.

This repository is a Rust implementation of the next version of [Quarto](https://quarto.org). The goal is to replace parts of the TypeScript/Deno runtime with a unified Rust implementation.

## Installing

```sh
curl -fsSL https://raw.githubusercontent.com/quarto-dev/q2/main/install.sh | bash
```

On Windows (PowerShell):

```powershell
irm https://raw.githubusercontent.com/quarto-dev/q2/main/install.ps1 | iex
```

The installer downloads the release archive for your platform, verifies
its SHA-256 checksum **and** its Ed25519 signature ([minisign](https://jedisct1.github.io/minisign/),
required — `brew install minisign` / `apt install minisign`), and
installs `q2` to `~/.local/bin` (override with `--dest` or
`Q2_INSTALL_DIR`). The signing public key, pinned in `install.sh`:

```
RWR2A9ILpZX1kVF3Q6uk5TRus8FDM25H2F+KKKHEuqlxv+JJSLyPalvN
```

To verify a manually downloaded archive:

```sh
minisign -Vm q2-<version>-<platform>.tar.gz -P RWR2A9ILpZX1kVF3Q6uk5TRus8FDM25H2F+KKKHEuqlxv+JJSLyPalvN
```

The trusted comment should name exactly the file you downloaded.

`q2 mcp` (the Quarto Hub MCP server) additionally needs
[Node.js](https://nodejs.org) 24+ at runtime; release binaries come
with the MCP server and quarto-hub.com connection defaults built in, so
no further configuration is needed. Private hub operators can still
point elsewhere via the `QUARTO_HUB_MCP_CLIENT_ID`,
`QUARTO_HUB_MCP_CLIENT_SECRET`, and `QUARTO_HUB_SERVER` environment
variables, which always win over the built-in defaults.

To register it with an MCP client, add an entry that launches `q2 mcp`
(run `q2 mcp --print-config` to print this snippet):

```json
{
  "mcpServers": {
    "quarto-hub": {
      "command": "q2",
      "args": ["mcp"]
    }
  }
}
```

For a non-canonical hub, append the server flag:
`"args": ["mcp", "--server", "wss://your-hub/ws"]`. Run `q2 mcp --help`
for the full launcher + server option list, and `q2 mcp --launcher-info`
to diagnose the embedded bundle and Node.js discovery.

## Building

Requires Rust nightly (edition 2024).

```bash
# Installs nodejs dependencies for the vite/WASM dependencies

npm install
# Build all Rust crates and binaries and test; build hub-client and its TS test suite
cargo xtask verify

# Just build everything instead (use --release for optimized binaries)
cargo xtask build-all

# Run tests (uses nextest)
cargo nextest run
```

## Contributing

We welcome discussions about the project via GitHub issues.

## Status

This is experimental software. All API should be considered unstable and may completely change.

## License

MIT - See [LICENSE](LICENSE) for details.
