"""The quarto2 Python package: installs the native `q2` binary.

The binary itself is installed as a script (bin/q2, or Scripts\\q2.exe on
Windows), not inside this package. This module only helps Python code
find it.
"""

import shutil
from importlib import metadata

__all__ = ["binary_path", "__version__"]

try:
    __version__ = metadata.version("quarto2")
except metadata.PackageNotFoundError:  # running from a source checkout
    __version__ = "0+unknown"

_BINARY_NAMES = ("q2", "q2.exe")


def binary_path():
    """Absolute path of the `q2` binary this package installed.

    Reads the package's own RECORD, so it finds the right binary even when
    another `q2` comes first on PATH. Falls back to PATH lookup.
    """
    try:
        files = metadata.files("quarto2") or []
    except metadata.PackageNotFoundError:
        files = []
    for f in files:
        if f.name in _BINARY_NAMES and "quarto2" not in f.parts[:1]:
            path = f.locate()
            if path.exists():
                return str(path.resolve())
    found = shutil.which("q2")
    if found:
        return found
    raise FileNotFoundError("the q2 binary installed by quarto2 was not found")
