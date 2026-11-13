"""Helpers for the quarto2 PyPI package (bd-2p7dwmqe).

The quarto2 sdist carries no binary. When pip or uv builds it into a
wheel, the in-tree backend (q2_pypi_backend.py) uses these helpers to
download this host's q2 archive from the GitHub Release, check it against
the SHA-256 pinned in the sdist's release.json, and place the binary in
the wheel's scripts directory.

Stdlib only: this runs inside the build frontend's isolated environment,
which has nothing installed (pyproject.toml declares `requires = []`).

Also a small CLI used by .github/workflows/publish-pypi.yml:

    python -I q2_pypi_lib.py prepare --tag v0.45.0 \\
        --checksums checksums.sha256 --license ../../LICENSE [--post N]

writes release.json and LICENSE next to pyproject.toml. Both files are
generated and gitignored.
"""

import argparse
import hashlib
import json
import os
import re
import shutil
import sys
import sysconfig
import tarfile
import tempfile
import urllib.request
import zipfile
from pathlib import Path

DEFAULT_DOWNLOAD_BASE = "https://github.com/quarto-dev/q2/releases/download"
DOWNLOAD_BASE_ENV = "QUARTO2_DOWNLOAD_BASE"

PLATFORMS = (
    "darwin_amd64",
    "darwin_arm64",
    "linux_amd64",
    "linux_arm64",
    "windows_amd64",
    "windows_arm64",
)

_OS = {"linux": "linux", "darwin": "darwin", "win32": "windows"}
_ARCH = {
    "x86_64": "amd64",
    "amd64": "amd64",
    "aarch64": "arm64",
    "arm64": "arm64",
}


class UnsupportedPlatform(Exception):
    pass


class ChecksumMismatch(Exception):
    pass


def detect_platform(sys_platform, machine):
    """Map `sys.platform` and `platform.machine()` to a release platform name.

    Linux needs no libc check: the release's linux binaries are static musl.
    """
    os_name = _OS.get(sys_platform)
    arch = _ARCH.get(machine.lower())
    if os_name is None or arch is None:
        raise UnsupportedPlatform(
            f"q2 publishes no binary for this platform "
            f"(sys.platform={sys_platform!r}, machine={machine!r}). "
            f"Build it from source instead: "
            f"https://github.com/quarto-dev/q2#installation (install.sh --from-source)."
        )
    return f"{os_name}_{arch}"


def archive_name(version, platform_name):
    ext = "zip" if platform_name.startswith("windows") else "tar.gz"
    return f"q2-{version}-{platform_name}.{ext}"


def binary_name(platform_name):
    return "q2.exe" if platform_name.startswith("windows") else "q2"


_CARGO_VERSION = re.compile(
    r"^(\d+\.\d+\.\d+)(?:-(alpha|beta|rc)\.?(\d+))?$"
)
_PRE = {"alpha": "a", "beta": "b", "rc": "rc"}


def pep440_version(cargo_version):
    """Cargo `X.Y.Z[-rcN]` → PEP 440. Nightlies are never published."""
    m = _CARGO_VERSION.match(cargo_version)
    if not m:
        raise ValueError(f"not a publishable q2 version: {cargo_version!r}")
    base, pre, num = m.groups()
    return base + (f"{_PRE[pre]}{num}" if pre else "")


def release_info_from_checksums(tag, checksums_text, post=0):
    """Build the release.json payload from a release's checksums.sha256."""
    if not tag.startswith("v"):
        raise ValueError(f"release tags start with 'v': {tag!r}")
    version = tag[1:]
    pypi_version = pep440_version(version) + (f".post{post}" if post else "")
    digests = {}
    for line in checksums_text.splitlines():
        if not line.strip():
            continue
        parts = line.split()
        if len(parts) != 2 or not re.fullmatch(r"[0-9a-f]{64}", parts[0]):
            raise ValueError(f"malformed checksums line: {line!r}")
        digests[parts[1].lstrip("*")] = parts[0]
    assets = {}
    missing = []
    for plat in PLATFORMS:
        name = archive_name(version, plat)
        if name in digests:
            assets[plat] = {"name": name, "sha256": digests[name]}
        else:
            missing.append(plat)
    if missing:
        raise ValueError(f"checksums for {tag} lack platforms: {', '.join(missing)}")
    return {
        "tag": tag,
        "version": version,
        "pypi_version": pypi_version,
        "assets": assets,
    }


def prepare(pypi_dir, tag, checksums_text, license_path, post=0):
    """Write release.json and LICENSE into the package directory."""
    pypi_dir = Path(pypi_dir)
    info = release_info_from_checksums(tag, checksums_text, post=post)
    (pypi_dir / "release.json").write_text(json.dumps(info, indent=2) + "\n")
    shutil.copyfile(license_path, pypi_dir / "LICENSE")
    return info


def download_url(tag, name):
    base = os.environ.get(DOWNLOAD_BASE_ENV) or DEFAULT_DOWNLOAD_BASE
    return f"{base.rstrip('/')}/{tag}/{name}"


def download(url, dest):
    with urllib.request.urlopen(url) as resp, open(dest, "wb") as out:
        shutil.copyfileobj(resp, out, length=1 << 20)


def sha256_file(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def verify_sha256(path, expected):
    actual = sha256_file(path)
    if actual != expected:
        raise ChecksumMismatch(
            f"{Path(path).name}: expected sha256 {expected}, got {actual}"
        )


def extract_binary(archive, platform_name, dest_dir):
    """Extract the single q2 binary from a release archive.

    Release archives hold exactly one regular file, the binary. Anything
    else (extra members, links, paths) is refused rather than skipped.
    """
    member = binary_name(platform_name)
    dest_dir = Path(dest_dir)
    dest_dir.mkdir(parents=True, exist_ok=True)
    out = dest_dir / member
    if platform_name.startswith("windows"):
        with zipfile.ZipFile(archive) as zf:
            names = zf.namelist()
            if names != [member]:
                raise ValueError(f"unexpected archive contents: {names}")
            with zf.open(member) as src, open(out, "wb") as dst:
                shutil.copyfileobj(src, dst)
    else:
        with tarfile.open(archive, "r:gz") as tf:
            members = tf.getmembers()
            if [m.name for m in members] != [member] or not members[0].isfile():
                raise ValueError(
                    f"unexpected archive contents: {[m.name for m in members]}"
                )
            with tf.extractfile(members[0]) as src, open(out, "wb") as dst:
                shutil.copyfileobj(src, dst)
    return out


def fetch_binary(info, platform_name, dest_dir):
    """Download, verify and extract this platform's binary from a release."""
    asset = info["assets"][platform_name]
    with tempfile.TemporaryDirectory() as tmp:
        archive = Path(tmp) / asset["name"]
        url = download_url(info["tag"], asset["name"])
        print(f"quarto2: downloading {url}", file=sys.stderr)
        download(url, archive)
        verify_sha256(archive, asset["sha256"])
        return extract_binary(archive, platform_name, dest_dir)


def wheel_tag():
    """`py3-none-<platform>` for this host; never `any`.

    The wheel a frontend builds from our sdist holds this host's binary,
    so its tag must keep a shared wheel cache from reusing it elsewhere.
    """
    plat = sysconfig.get_platform().replace("-", "_").replace(".", "_")
    return f"py3-none-{plat}"


def _main(argv=None):
    parser = argparse.ArgumentParser(prog="q2_pypi_lib")
    sub = parser.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("prepare", help="write release.json and LICENSE")
    p.add_argument("--tag", required=True)
    p.add_argument("--checksums", required=True, type=Path)
    p.add_argument("--license", required=True, type=Path)
    p.add_argument("--post", type=int, default=0)
    p.add_argument("--dir", type=Path, default=Path(__file__).resolve().parent.parent)
    args = parser.parse_args(argv)
    info = prepare(args.dir, args.tag, args.checksums.read_text(), args.license, args.post)
    print(json.dumps(info, indent=2))


if __name__ == "__main__":
    _main()
