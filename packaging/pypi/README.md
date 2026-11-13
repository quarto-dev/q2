# quarto2

[Quarto 2](https://github.com/quarto-dev/q2) is an open-source scientific
and technical publishing system. This package installs its command-line
tool, `q2`.

## Install

```bash
pip install quarto2
# or, as an isolated tool
uv tool install quarto2
pipx install quarto2
```

Then:

```bash
q2 --version
q2 render document.qmd
```

For a one-off run without installing: `uvx --from quarto2 q2 render document.qmd`.

## How this package works

The `q2` binary is too large to upload to PyPI for every platform, so this
package is a source distribution that contains no binary. When pip or uv
installs it, the package downloads the matching `q2` archive for your
machine from the [GitHub release](https://github.com/quarto-dev/q2/releases)
of the same version and checks it against a SHA-256 checksum recorded in
the package. The installed `q2` is the native binary, with no Python
wrapper.

Installing therefore needs access to `github.com`. To install from a
mirror, set `QUARTO2_DOWNLOAD_BASE` to a URL laid out like
`<base>/v<version>/<archive>`:

```bash
QUARTO2_DOWNLOAD_BASE=https://mirror.example.com/q2 pip install quarto2
```

Supported platforms: Linux (x86-64, ARM64; any libc), macOS (Intel, Apple
silicon) and Windows (x86-64, ARM64).

From Python, `quarto2.binary_path()` returns the path of the installed
binary, and `python -m quarto2 ...` runs it.

Other ways to install Quarto 2, including a standalone installer, are in
the [Quarto 2 README](https://github.com/quarto-dev/q2#readme).
