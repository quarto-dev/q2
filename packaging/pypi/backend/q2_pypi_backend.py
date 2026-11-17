"""In-tree PEP 517 build backend for the quarto2 PyPI package (bd-2p7dwmqe).

Why a custom backend instead of setuptools: the wheel must carry a
~190 MB native executable as a *script* (so pip/uv install the real `q2`
into the environment's bin/ or Scripts\\ with no Python wrapper), tagged
for the build host only. setuptools' script handling reads scripts as
text, and its wheel tags would need overriding anyway; writing the wheel
directly is a few dozen lines and needs no build dependencies.

- `build_sdist` packs the sources plus the generated release.json (version
  and per-platform SHA-256 pins). The sdist holds no binary.
- `build_wheel` runs on the installing machine: it downloads this host's
  archive from the GitHub Release (or $QUARTO2_DOWNLOAD_BASE), verifies it
  against release.json, and writes a `py3-none-<host>` wheel whose
  `.data/scripts/` holds the binary.
- `prepare_metadata_for_build_wheel` answers metadata queries without
  downloading anything.
"""

import base64
import csv
import gzip
import hashlib
import io
import json
import os
import platform
import shutil
import sys
import tarfile
import tempfile
import zipfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import q2_pypi_lib as lib  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
NAME = "quarto2"
SUMMARY = "Quarto 2: an open-source scientific and technical publishing system"
SOURCE_FILES = ["pyproject.toml", "README.md", "LICENSE", "release.json"]
SOURCE_DIRS = ["backend", "quarto2"]
# 1980-01-01, the earliest timestamp a zip entry can hold; keeps builds
# reproducible.
EPOCH = (1980, 1, 1, 0, 0, 0)
EPOCH_SECONDS = 315532800


def _info():
    path = ROOT / "release.json"
    if not path.exists():
        raise RuntimeError(
            "release.json is missing; generate it with "
            "`python -I backend/q2_pypi_lib.py prepare --tag vX.Y.Z "
            "--checksums checksums.sha256 --license ../../LICENSE`"
        )
    return json.loads(path.read_text())


def _metadata(info):
    readme = (ROOT / "README.md").read_text(encoding="utf-8")
    headers = [
        ("Metadata-Version", "2.1"),
        ("Name", NAME),
        ("Version", info["pypi_version"]),
        ("Summary", SUMMARY),
        ("Author", "Posit Software, PBC"),
        ("License", "MIT"),
        ("Project-URL", "Homepage, https://quarto.org"),
        ("Project-URL", "Source, https://github.com/quarto-dev/q2"),
        ("Project-URL", "Issues, https://github.com/quarto-dev/q2/issues"),
        ("Classifier", "Development Status :: 4 - Beta"),
        ("Classifier", "Environment :: Console"),
        ("Classifier", "Intended Audience :: Science/Research"),
        ("Classifier", "License :: OSI Approved :: MIT License"),
        ("Classifier", "Operating System :: MacOS"),
        ("Classifier", "Operating System :: Microsoft :: Windows"),
        ("Classifier", "Operating System :: POSIX :: Linux"),
        ("Classifier", "Topic :: Text Processing :: Markup"),
        ("Requires-Python", ">=3.9"),
        ("Description-Content-Type", "text/markdown"),
    ]
    head = "".join(f"{k}: {v}\n" for k, v in headers)
    return f"{head}\n{readme}"


def _wheel_file(tag):
    return (
        "Wheel-Version: 1.0\n"
        "Generator: quarto2 in-tree backend\n"
        "Root-Is-Purelib: false\n"
        f"Tag: {tag}\n"
    )


def _dist_info(info):
    return f"{NAME}-{info['pypi_version']}.dist-info"


# --- PEP 517 hooks ----------------------------------------------------------


def get_requires_for_build_sdist(config_settings=None):
    return []


def get_requires_for_build_wheel(config_settings=None):
    return []


def build_sdist(sdist_directory, config_settings=None):
    info = _info()
    base = f"{NAME}-{info['pypi_version']}"
    entries = [("PKG-INFO", _metadata(info).encode("utf-8"), 0o644)]
    for name in SOURCE_FILES:
        path = ROOT / name
        if not path.exists():
            raise RuntimeError(f"{name} is missing from {ROOT}; run `prepare` first")
        entries.append((name, path.read_bytes(), 0o644))
    for d in SOURCE_DIRS:
        for path in sorted((ROOT / d).glob("*.py")):
            entries.append((f"{d}/{path.name}", path.read_bytes(), 0o644))

    filename = f"{base}.tar.gz"
    target = Path(sdist_directory) / filename
    with open(target, "wb") as raw, gzip.GzipFile(
        filename="", fileobj=raw, mode="wb", mtime=EPOCH_SECONDS
    ) as gz, tarfile.open(fileobj=gz, mode="w", format=tarfile.PAX_FORMAT) as tf:
        for name, data, mode in entries:
            ti = tarfile.TarInfo(f"{base}/{name}")
            ti.size = len(data)
            ti.mode = mode
            ti.mtime = EPOCH_SECONDS
            tf.addfile(ti, io.BytesIO(data))
    return filename


def prepare_metadata_for_build_wheel(metadata_directory, config_settings=None):
    info = _info()
    dist_info = Path(metadata_directory) / _dist_info(info)
    dist_info.mkdir(parents=True, exist_ok=True)
    (dist_info / "METADATA").write_text(_metadata(info), encoding="utf-8")
    (dist_info / "WHEEL").write_text(_wheel_file(lib.wheel_tag()))
    shutil.copyfile(ROOT / "LICENSE", dist_info / "LICENSE")
    return dist_info.name


def build_wheel(wheel_directory, config_settings=None, metadata_directory=None):
    info = _info()
    plat = lib.detect_platform(sys.platform, platform.machine())
    tag = lib.wheel_tag()
    dist_info = _dist_info(info)
    data_scripts = f"{NAME}-{info['pypi_version']}.data/scripts"
    filename = f"{NAME}-{info['pypi_version']}-{tag}.whl"

    with tempfile.TemporaryDirectory() as tmp:
        binary = lib.fetch_binary(info, plat, Path(tmp) / "bin")
        staged = Path(tmp) / filename
        with _WheelWriter(staged) as whl:
            for path in sorted((ROOT / "quarto2").glob("*.py")):
                whl.add(f"quarto2/{path.name}", path.read_bytes())
            whl.add_file(f"{data_scripts}/{binary.name}", binary, mode=0o755)
            whl.add(f"{dist_info}/METADATA", _metadata(info).encode("utf-8"))
            whl.add(f"{dist_info}/WHEEL", _wheel_file(tag).encode())
            whl.add(f"{dist_info}/LICENSE", (ROOT / "LICENSE").read_bytes())
            whl.finish(f"{dist_info}/RECORD")
        # Move into place only once complete, so a failure leaves nothing.
        shutil.move(str(staged), os.path.join(wheel_directory, filename))
    return filename


class _WheelWriter:
    """A zip writer that keeps the RECORD rows for what it writes."""

    def __init__(self, path):
        self.zf = zipfile.ZipFile(path, "w", compression=zipfile.ZIP_DEFLATED)
        self.records = []

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.zf.close()
        return False

    def _zipinfo(self, name, mode):
        zi = zipfile.ZipInfo(name, date_time=EPOCH)
        zi.external_attr = (0o100000 | mode) << 16
        zi.compress_type = zipfile.ZIP_DEFLATED
        return zi

    @staticmethod
    def _digest(h):
        return "sha256=" + base64.urlsafe_b64encode(h.digest()).rstrip(b"=").decode()

    def add(self, name, data, mode=0o644):
        self.zf.writestr(self._zipinfo(name, mode), data)
        self.records.append((name, self._digest(hashlib.sha256(data)), str(len(data))))

    def add_file(self, name, path, mode=0o644):
        # Stream: the binary is ~190 MB, so avoid holding it in memory.
        h = hashlib.sha256()
        size = 0
        with open(path, "rb") as src, self.zf.open(self._zipinfo(name, mode), "w") as dst:
            for chunk in iter(lambda: src.read(1 << 20), b""):
                h.update(chunk)
                size += len(chunk)
                dst.write(chunk)
        self.records.append((name, self._digest(h), str(size)))

    def finish(self, record_name):
        buf = io.StringIO()
        writer = csv.writer(buf, lineterminator="\n")
        writer.writerows(self.records)
        writer.writerow((record_name, "", ""))
        self.zf.writestr(self._zipinfo(record_name, 0o644), buf.getvalue())
