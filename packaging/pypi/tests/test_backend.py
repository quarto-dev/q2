"""End-to-end tests for the in-tree PEP 517 backend (backend/q2_pypi_backend.py).

Each hook runs in a subprocess with the source tree as its working
directory, the way pip and uv call a build backend. Downloads come from a
fake release on disk through QUARTO2_DOWNLOAD_BASE, so nothing here needs
the network.
"""

import base64
import csv
import hashlib
import io
import json
import os
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest
import venv
import zipfile
from pathlib import Path

import helpers
import q2_pypi_lib as lib

VERSION = "0.45.0"
SOURCE_ENTRIES = ["pyproject.toml", "README.md", "backend", "quarto2"]


def run_hook(source_dir, hook, *args, env=None):
    """Call a backend hook the way a build frontend does; return its result."""
    code = (
        "import json, sys\n"
        "sys.path.insert(0, 'backend')\n"
        "import q2_pypi_backend as b\n"
        f"print(json.dumps(b.{hook}(*json.loads(sys.argv[1]))))\n"
    )
    proc = subprocess.run(
        [sys.executable, "-I", "-c", code, json.dumps(list(args))],
        cwd=source_dir,
        env=env,
        capture_output=True,
        text=True,
    )
    if proc.returncode != 0:
        raise RuntimeError(f"{hook} failed:\n{proc.stderr}")
    return json.loads(proc.stdout.strip().splitlines()[-1])


class BackendTestCase(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.base_url, checksums = helpers.make_fake_release(self.root / "mirror", VERSION)
        # Work on a copy of the package sources so the repo stays clean.
        self.src = self.root / "src"
        self.src.mkdir()
        for entry in SOURCE_ENTRIES:
            origin = helpers.PYPI_DIR / entry
            if origin.is_dir():
                shutil.copytree(
                    origin, self.src / entry, ignore=shutil.ignore_patterns("__pycache__")
                )
            else:
                shutil.copy2(origin, self.src / entry)
        license_file = self.root / "LICENSE"
        license_file.write_text("MIT License\n")
        lib.prepare(self.src, f"v{VERSION}", checksums, license_file)
        self.env = dict(os.environ, QUARTO2_DOWNLOAD_BASE=self.base_url)

    def tearDown(self):
        self.tmp.cleanup()

    def build_sdist(self):
        out = self.root / "sdist"
        out.mkdir(exist_ok=True)
        name = run_hook(self.src, "build_sdist", str(out), env=self.env)
        return out / name

    def unpack_sdist(self, sdist):
        dest = self.root / "unpacked"
        with tarfile.open(sdist) as tf:
            tf.extractall(dest, filter="data")
        return dest / f"quarto2-{VERSION}"


class Prepare(BackendTestCase):
    def test_writes_release_json_and_license(self):
        info = json.loads((self.src / "release.json").read_text())
        self.assertEqual(info["tag"], f"v{VERSION}")
        self.assertEqual(len(info["assets"]), 6)
        self.assertEqual((self.src / "LICENSE").read_text(), "MIT License\n")


class BuildSdist(BackendTestCase):
    def test_sdist_is_small_and_complete(self):
        sdist = self.build_sdist()
        self.assertEqual(sdist.name, f"quarto2-{VERSION}.tar.gz")
        self.assertLess(sdist.stat().st_size, 1_000_000)
        with tarfile.open(sdist) as tf:
            names = set(tf.getnames())
            prefix = f"quarto2-{VERSION}/"
            for required in [
                "PKG-INFO",
                "pyproject.toml",
                "README.md",
                "LICENSE",
                "release.json",
                "backend/q2_pypi_backend.py",
                "backend/q2_pypi_lib.py",
                "quarto2/__init__.py",
                "quarto2/__main__.py",
            ]:
                self.assertIn(prefix + required, names)
            self.assertFalse(any("/tests/" in n or "__pycache__" in n for n in names))
            pkg_info = tf.extractfile(prefix + "PKG-INFO").read().decode()
        self.assertIn("Name: quarto2\n", pkg_info)
        self.assertIn(f"Version: {VERSION}\n", pkg_info)
        self.assertIn("Requires-Python: >=3.9\n", pkg_info)

    def test_sdist_requires_prepare(self):
        (self.src / "release.json").unlink()
        with self.assertRaises(RuntimeError) as cm:
            self.build_sdist()
        self.assertIn("release.json", str(cm.exception))


class BuildWheel(BackendTestCase):
    def build_wheel(self, env=None):
        unpacked = self.unpack_sdist(self.build_sdist())
        out = self.root / "wheel"
        out.mkdir(exist_ok=True)
        name = run_hook(unpacked, "build_wheel", str(out), env=env or self.env)
        return out / name

    def test_wheel_holds_the_verified_binary_as_a_script(self):
        wheel = self.build_wheel()
        self.assertEqual(wheel.name, f"quarto2-{VERSION}-{lib.wheel_tag()}.whl")
        plat = helpers.host_platform()
        script = f"quarto2-{VERSION}.data/scripts/{lib.binary_name(plat)}"
        with zipfile.ZipFile(wheel) as zf:
            names = zf.namelist()
            self.assertIn(script, names)
            self.assertEqual(zf.read(script), helpers.fake_program(VERSION))
            mode = zf.getinfo(script).external_attr >> 16
            self.assertEqual(mode & 0o755, 0o755)
            wheel_meta = zf.read(f"quarto2-{VERSION}.dist-info/WHEEL").decode()
            self.assertIn("Root-Is-Purelib: false\n", wheel_meta)
            self.assertIn(f"Tag: {lib.wheel_tag()}\n", wheel_meta)
            self.assertIn("quarto2/__init__.py", names)
            self.assertIn(f"quarto2-{VERSION}.dist-info/LICENSE", names)
            self.assert_record_matches(zf)

    def assert_record_matches(self, zf):
        record_name = f"quarto2-{VERSION}.dist-info/RECORD"
        rows = list(csv.reader(io.StringIO(zf.read(record_name).decode())))
        recorded = {row[0]: row for row in rows}
        self.assertEqual(set(recorded), set(zf.namelist()))
        for name, digest, size in rows:
            if name == record_name:
                self.assertEqual((digest, size), ("", ""))
                continue
            data = zf.read(name)
            expected = base64.urlsafe_b64encode(hashlib.sha256(data).digest())
            self.assertEqual(digest, "sha256=" + expected.rstrip(b"=").decode())
            self.assertEqual(int(size), len(data))

    def test_binary_bytes_survive_unchanged(self):
        plat = helpers.host_platform()
        _, checksums = helpers.make_fake_release(
            self.root / "mirror", VERSION, payloads={plat: helpers.BINARY_BYTES}
        )
        lib.prepare(self.src, f"v{VERSION}", checksums, self.root / "LICENSE")
        wheel = self.build_wheel()
        with zipfile.ZipFile(wheel) as zf:
            data = zf.read(f"quarto2-{VERSION}.data/scripts/{lib.binary_name(plat)}")
        self.assertEqual(data, helpers.BINARY_BYTES)

    def test_checksum_mismatch_aborts_the_build(self):
        unpacked = self.unpack_sdist(self.build_sdist())
        # Swap the archive on the mirror after the sdist pinned its digest.
        plat = helpers.host_platform()
        archive = self.root / "mirror" / f"v{VERSION}" / lib.archive_name(VERSION, plat)
        helpers.write_archive(archive, plat, b"tampered")
        out = self.root / "wheel"
        out.mkdir()
        with self.assertRaises(RuntimeError) as cm:
            run_hook(unpacked, "build_wheel", str(out), env=self.env)
        self.assertIn("ChecksumMismatch", str(cm.exception))
        self.assertEqual(list(out.iterdir()), [])

    def test_metadata_hook_does_not_download(self):
        unpacked = self.unpack_sdist(self.build_sdist())
        out = self.root / "meta"
        out.mkdir()
        env = dict(self.env, QUARTO2_DOWNLOAD_BASE="file:///nonexistent")
        name = run_hook(unpacked, "prepare_metadata_for_build_wheel", str(out), env=env)
        self.assertEqual(name, f"quarto2-{VERSION}.dist-info")
        self.assertIn("Name: quarto2\n", (out / name / "METADATA").read_text())


@unittest.skipIf(os.name == "nt", "the fake q2 is a POSIX shell script")
class PipInstall(BackendTestCase):
    """Install the sdist with pip, offline, and run what it installed."""

    def test_pip_installs_a_working_q2(self):
        sdist = self.build_sdist()
        env_dir = self.root / "venv"
        venv.create(env_dir, with_pip=True)
        python = env_dir / "bin" / "python"
        subprocess.run(
            [str(python), "-m", "pip", "install", "--no-index", "--no-cache-dir", str(sdist)],
            env=self.env,
            check=True,
            capture_output=True,
        )
        expected = f"q2 (quarto 2) {VERSION}"
        q2 = env_dir / "bin" / "q2"
        out = subprocess.run([str(q2), "--version"], capture_output=True, text=True)
        self.assertEqual(out.stdout.strip(), expected)

        out = subprocess.run(
            [str(python), "-m", "quarto2", "--version"], capture_output=True, text=True
        )
        self.assertEqual(out.stdout.strip(), expected)

        out = subprocess.run(
            [str(python), "-c", "import quarto2; print(quarto2.binary_path()); print(quarto2.__version__)"],
            capture_output=True,
            text=True,
            check=True,
        )
        path, version = out.stdout.strip().splitlines()
        self.assertEqual(Path(path).resolve(), q2.resolve())
        self.assertEqual(version, VERSION)


if __name__ == "__main__":
    unittest.main()
