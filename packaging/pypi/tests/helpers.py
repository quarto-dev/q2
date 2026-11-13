"""Shared fixtures for the quarto2 PyPI packaging tests.

The tests never touch the network: they build a fake release (archives
holding a stand-in `q2` program, plus the matching `checksums.sha256`) in
a temp directory and point `QUARTO2_DOWNLOAD_BASE` at it with a
`file://` URL.
"""

import hashlib
import io
import os
import sys
import tarfile
import zipfile
from pathlib import Path

PYPI_DIR = Path(__file__).resolve().parent.parent
BACKEND_DIR = PYPI_DIR / "backend"

if str(BACKEND_DIR) not in sys.path:
    sys.path.insert(0, str(BACKEND_DIR))

PLATFORMS = [
    "darwin_amd64",
    "darwin_arm64",
    "linux_amd64",
    "linux_arm64",
    "windows_amd64",
    "windows_arm64",
]

# Bytes that are not valid UTF-8 and contain NUL and newline bytes, like a
# real executable. The packager must carry them through byte for byte.
BINARY_BYTES = b"\x7fELF\x02\x01\x01\x00\xff\xfe\n\x00\x80binary payload\r\n\xc3\x28"


def fake_program(version):
    """A POSIX shell script that behaves like `q2 --version`."""
    return (f'#!/bin/sh\necho "q2 (quarto 2) {version}"\n').encode()


def write_archive(path, platform_name, payload):
    """Write a release archive in the same layout as release-pipeline.yml."""
    if platform_name.startswith("windows"):
        with zipfile.ZipFile(path, "w") as zf:
            zf.writestr("q2.exe", payload)
    else:
        with tarfile.open(path, "w:gz") as tf:
            info = tarfile.TarInfo("q2")
            info.size = len(payload)
            info.mode = 0o755
            tf.addfile(info, io.BytesIO(payload))


def make_fake_release(root, version, payloads=None):
    """Create `<root>/v<version>/` with six archives and checksums.sha256.

    Returns (download_base_url, checksums_text). `payloads` maps a platform
    name to the bytes its archive should hold; by default every platform
    gets the fake shell program.
    """
    tag = f"v{version}"
    release_dir = Path(root) / tag
    release_dir.mkdir(parents=True, exist_ok=True)
    lines = []
    for plat in PLATFORMS:
        ext = "zip" if plat.startswith("windows") else "tar.gz"
        name = f"q2-{version}-{plat}.{ext}"
        payload = (payloads or {}).get(plat, fake_program(version))
        write_archive(release_dir / name, plat, payload)
        digest = hashlib.sha256((release_dir / name).read_bytes()).hexdigest()
        lines.append(f"{digest}  {name}")
    # Real checksum files also list non-platform assets; they must be ignored.
    lines.append(f"{'0' * 64}  quarto-hub-mcp-{version}.tar.gz")
    checksums = "\n".join(lines) + "\n"
    (release_dir / "checksums.sha256").write_text(checksums)
    return Path(root).resolve().as_uri(), checksums


def host_platform():
    """Our platform name for the machine running the tests."""
    import q2_pypi_lib

    import platform

    return q2_pypi_lib.detect_platform(sys.platform, platform.machine())


class EnvPatch:
    """Context manager that sets environment variables and restores them."""

    def __init__(self, **values):
        self.values = values
        self.saved = {}

    def __enter__(self):
        for key, value in self.values.items():
            self.saved[key] = os.environ.get(key)
            if value is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = value
        return self

    def __exit__(self, *exc):
        for key, value in self.saved.items():
            if value is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = value
        return False
