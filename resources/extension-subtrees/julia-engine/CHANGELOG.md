# Quarto Julia Engine changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

- Ported the engine from Deno APIs to Node (`node:fs`, `node:child_process`, `node:net`, `node:crypto`), so the same bundle runs under Quarto 1 (Deno) and Quarto 2 (Node). The bundle is now built with esbuild (`npm run build`) instead of `quarto call build-ts-extension`.
- Replaced the socket reader with a buffering line framer: a server reply split across TCP chunks (including inside a UTF-8 character), or several replies in one chunk, are handled, and a connection that closes mid-reply now reports that rather than a JSON parse error. A connection that times out waiting for `isready` is now closed.
- Added static engine declarations (`name`, `claims`, `file-extensions`) to the engine's `_extension.yml` entry, allowing Quarto 2 to resolve `{julia}` cells and `.jl` files to this engine without loading the engine module. These keys are reserved for Quarto 2 and rejected by Quarto 1's `external-engine` schema today; merging requires a quarto-cli schema change (accept-and-ignore) to ship in a stable Quarto 1 release.
- Fixed oneShot renders failing when the file's worker on the shared control server was left busy by a previous client that vanished mid-run: the pre-run close now recovers by force-closing the abandoned worker instead of surfacing the bare "worker is busy" protocol error, and a failed post-run cleanup close warns instead of discarding an otherwise successful run's results.
- The error-path worker close added in 0.2.2 now escalates to `forceclose` when the worker is still busy (a transport-level failure can leave it running), so a failed run cannot leave the abandoned-busy state the pre-run recovery has to clean up later.
- Fixed the detached control server inheriting the launching process's stdout/stderr: the server's early startup output (Julia banners, precompile messages) could interleave with quarto's own output, and corrupt structured-output channels in embedding hosts. The server's stdio is now redirected to devnull; the server log file is unaffected.

## [0.2.2](https://github.com/PumasAI/quarto-julia-engine/releases/tag/v0.2.2) - 2026-09-08

- A failed render without an execution daemon no longer leaves the notebook's worker process running [#14](https://github.com/PumasAI/quarto-julia-engine/pull/14).

## [0.2.1](https://github.com/PumasAI/quarto-julia-engine/releases/tag/v0.2.1) - 2026-09-02

- Updated to QuartoNotebookRunner 0.18.2 [#13](https://github.com/PumasAI/quarto-julia-engine/pull/13):
  - Shell (`;`), help (`?`), and Pkg (`]`) mode cells now work when the cell has `#|` options.
- Large documents no longer fail with `ERROR: Internal Error` when the payload sent to the Julia server exceeds the socket send buffer ([quarto-cli#14834](https://github.com/quarto-dev/quarto-cli/issues/14834), [#12](https://github.com/PumasAI/quarto-julia-engine/pull/12)).

## [0.2.0](https://github.com/PumasAI/quarto-julia-engine/releases/tag/v0.2.0) - 2026-03-24

- Added `keep-ipynb` support. When `keep-ipynb: true` is set in document YAML, the executed notebook is written to `<stem>.ipynb` alongside the source file [#8](https://github.com/PumasAI/quarto-julia-engine/pull/8).
- Updated to QuartoNotebookRunner 0.18.1 [#11](https://github.com/PumasAI/quarto-julia-engine/pull/11):
  - **`fig-format: retina` support**: Normalized to `png` with doubled `fig-dpi`, matching Quarto's Jupyter and knitr backends. Fixes blurry plots with default HTML settings.
  - **Improved cache invalidation**: Cache key now hashes full `Manifest.toml` content (SHA-224) so dependency version changes correctly invalidate the cache.
- Updated to QuartoNotebookRunner 0.18.0 [#7](https://github.com/PumasAI/quarto-julia-engine/pull/7):
  - **Shared worker processes**: Multiple notebooks can share a single worker process when their configs match. Enable with `share_worker_process: true`.
  - **`execute-dir` support**: Quarto's `execute-dir` option (e.g. `execute-dir: project`) is now passed through correctly.
  - **Cached worker environments**: Worker envs persist across sessions, skipping environment setup on subsequent runs when Julia and QNR versions haven't changed.
  - **Relaxed manifest version checking**: Only major.minor is checked by default, matching Julia's Pkg behavior. Opt into strict major.minor.patch via `julia: strict_manifest_versions: true`.
  - **Diagnostic file logging**: Set `QUARTONOTEBOOKRUNNER_LOG` to a directory path to enable timestamped log files.
  - **Echo handling for Python/R cells**: Fixed duplicate YAML keys and added proper support for `echo: false` and `echo: fenced`.

## [0.1.0](https://github.com/PumasAI/quarto-julia-engine/releases/tag/v0.1.0) - 2026-03-19

- Initial release as a standalone Quarto extension, extracted from [quarto-cli](https://github.com/quarto-dev/quarto-cli).
- Bundled Julia resource files (`Project.toml`, `ensure_environment.jl`, `quartonotebookrunner.jl`, `start_quartonotebookrunner_detached.jl`) into the extension, making it fully self-contained [#6](https://github.com/PumasAI/quarto-julia-engine/pull/6).
- Added CI workflow with tests on Linux, macOS, and Windows [#6](https://github.com/PumasAI/quarto-julia-engine/pull/6).
