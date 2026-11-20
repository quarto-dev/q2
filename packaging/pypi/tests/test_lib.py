"""Unit tests for the pure helpers in backend/q2_pypi_lib.py."""

import hashlib
import io
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path

import helpers  # noqa: F401  (puts backend/ on sys.path)
import q2_pypi_lib as lib


class DetectPlatform(unittest.TestCase):
    def test_all_six_platforms(self):
        cases = {
            ("linux", "x86_64"): "linux_amd64",
            ("linux", "aarch64"): "linux_arm64",
            ("linux", "arm64"): "linux_arm64",
            ("darwin", "x86_64"): "darwin_amd64",
            ("darwin", "arm64"): "darwin_arm64",
            ("win32", "AMD64"): "windows_amd64",
            ("win32", "ARM64"): "windows_arm64",
        }
        for (sys_platform, machine), expected in cases.items():
            with self.subTest(sys_platform=sys_platform, machine=machine):
                self.assertEqual(lib.detect_platform(sys_platform, machine), expected)

    def test_unsupported_hosts_fail_with_guidance(self):
        for sys_platform, machine in [
            ("linux", "ppc64le"),
            ("linux", "i686"),
            ("freebsd14", "amd64"),
            ("cygwin", "x86_64"),
        ]:
            with self.subTest(sys_platform=sys_platform, machine=machine):
                with self.assertRaises(lib.UnsupportedPlatform) as cm:
                    lib.detect_platform(sys_platform, machine)
                self.assertIn("--from-source", str(cm.exception))


class ArchiveNames(unittest.TestCase):
    def test_archive_and_member_names(self):
        self.assertEqual(
            lib.archive_name("0.45.0", "linux_amd64"), "q2-0.45.0-linux_amd64.tar.gz"
        )
        self.assertEqual(
            lib.archive_name("0.45.0", "windows_arm64"), "q2-0.45.0-windows_arm64.zip"
        )
        self.assertEqual(lib.binary_name("darwin_arm64"), "q2")
        self.assertEqual(lib.binary_name("windows_amd64"), "q2.exe")


class Pep440Version(unittest.TestCase):
    def test_release(self):
        self.assertEqual(lib.pep440_version("0.45.0"), "0.45.0")
        self.assertEqual(lib.pep440_version("1.0.0"), "1.0.0")

    def test_prereleases(self):
        self.assertEqual(lib.pep440_version("0.46.0-rc1"), "0.46.0rc1")
        self.assertEqual(lib.pep440_version("0.46.0-rc.2"), "0.46.0rc2")
        self.assertEqual(lib.pep440_version("1.0.0-beta.3"), "1.0.0b3")
        self.assertEqual(lib.pep440_version("1.0.0-alpha1"), "1.0.0a1")

    def test_nightlies_and_garbage_are_rejected(self):
        for bad in ["0.46.0-nightly.20261112", "0.45", "v0.45.0", "0.45.0-foo", ""]:
            with self.subTest(version=bad):
                with self.assertRaises(ValueError):
                    lib.pep440_version(bad)


class ReleaseInfo(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        _, self.checksums = helpers.make_fake_release(self.tmp.name, "0.45.0")

    def tearDown(self):
        self.tmp.cleanup()

    def test_from_checksums(self):
        info = lib.release_info_from_checksums("v0.45.0", self.checksums)
        self.assertEqual(info["tag"], "v0.45.0")
        self.assertEqual(info["version"], "0.45.0")
        self.assertEqual(info["pypi_version"], "0.45.0")
        self.assertEqual(sorted(info["assets"]), sorted(helpers.PLATFORMS))
        asset = info["assets"]["linux_amd64"]
        self.assertEqual(asset["name"], "q2-0.45.0-linux_amd64.tar.gz")
        self.assertRegex(asset["sha256"], r"^[0-9a-f]{64}$")

    def test_post_release_keeps_the_binary_tag(self):
        info = lib.release_info_from_checksums("v0.45.0", self.checksums, post=1)
        self.assertEqual(info["pypi_version"], "0.45.0.post1")
        self.assertEqual(info["tag"], "v0.45.0")

    def test_missing_platform_is_an_error(self):
        text = "\n".join(
            line for line in self.checksums.splitlines() if "darwin_arm64" not in line
        )
        with self.assertRaises(ValueError) as cm:
            lib.release_info_from_checksums("v0.45.0", text)
        self.assertIn("darwin_arm64", str(cm.exception))

    def test_tag_must_be_v_prefixed(self):
        with self.assertRaises(ValueError):
            lib.release_info_from_checksums("0.45.0", self.checksums)

    def test_malformed_digest_is_an_error(self):
        text = self.checksums.replace(self.checksums[:64], "xyz", 1)
        with self.assertRaises(ValueError):
            lib.release_info_from_checksums("v0.45.0", text)


class DownloadUrl(unittest.TestCase):
    def test_default_is_github(self):
        with helpers.EnvPatch(QUARTO2_DOWNLOAD_BASE=None):
            self.assertEqual(
                lib.download_url("v0.45.0", "q2-0.45.0-linux_amd64.tar.gz"),
                "https://github.com/quarto-dev/q2/releases/download/"
                "v0.45.0/q2-0.45.0-linux_amd64.tar.gz",
            )

    def test_mirror_override(self):
        with helpers.EnvPatch(QUARTO2_DOWNLOAD_BASE="https://mirror.example/q2/"):
            self.assertEqual(
                lib.download_url("v0.45.0", "x.zip"),
                "https://mirror.example/q2/v0.45.0/x.zip",
            )


class VerifyAndExtract(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def test_checksum_mismatch_raises(self):
        path = self.dir / "a.tar.gz"
        path.write_bytes(b"hello")
        lib.verify_sha256(path, hashlib.sha256(b"hello").hexdigest())
        with self.assertRaises(lib.ChecksumMismatch):
            lib.verify_sha256(path, hashlib.sha256(b"other").hexdigest())

    def test_extracts_binary_byte_for_byte(self):
        for plat in ["linux_amd64", "windows_amd64"]:
            with self.subTest(plat=plat):
                archive = self.dir / f"{plat}.archive"
                helpers.write_archive(archive, plat, helpers.BINARY_BYTES)
                out = lib.extract_binary(archive, plat, self.dir / plat)
                self.assertEqual(out.name, lib.binary_name(plat))
                self.assertEqual(out.read_bytes(), helpers.BINARY_BYTES)

    def test_rejects_unexpected_members(self):
        tar_path = self.dir / "extra.tar.gz"
        with tarfile.open(tar_path, "w:gz") as tf:
            for name in ["q2", "../evil"]:
                info = tarfile.TarInfo(name)
                info.size = 1
                tf.addfile(info, io.BytesIO(b"x"))
        with self.assertRaises(ValueError):
            lib.extract_binary(tar_path, "linux_amd64", self.dir / "out1")

        zip_path = self.dir / "extra.zip"
        with zipfile.ZipFile(zip_path, "w") as zf:
            zf.writestr("q2.exe", b"x")
            zf.writestr("README", b"x")
        with self.assertRaises(ValueError):
            lib.extract_binary(zip_path, "windows_amd64", self.dir / "out2")

    def test_rejects_a_symlinked_binary(self):
        tar_path = self.dir / "link.tar.gz"
        with tarfile.open(tar_path, "w:gz") as tf:
            info = tarfile.TarInfo("q2")
            info.type = tarfile.SYMTYPE
            info.linkname = "/bin/sh"
            tf.addfile(info)
        with self.assertRaises(ValueError):
            lib.extract_binary(tar_path, "linux_amd64", self.dir / "out3")


class WheelTag(unittest.TestCase):
    def test_tag_is_host_specific(self):
        tag = lib.wheel_tag()
        python, abi, plat = tag.split("-")
        self.assertEqual((python, abi), ("py3", "none"))
        self.assertNotEqual(plat, "any")
        self.assertRegex(plat, r"^[a-z0-9_]+$")


if __name__ == "__main__":
    unittest.main()
