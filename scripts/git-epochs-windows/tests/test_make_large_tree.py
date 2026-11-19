"""Tests for make_large_tree.py (temporary Windows tooling, never lands)."""
import hashlib
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parent.parent / "make_large_tree.py"


def run(dest, *extra):
    r = subprocess.run([sys.executable, "-I", str(SCRIPT), str(dest), *extra], capture_output=True, text=True)
    assert r.returncode == 0, r.stderr
    return r.stdout


def git(dest, *args):
    return subprocess.run(["git", *args], cwd=dest, capture_output=True, text=True, check=True).stdout.strip()


def tree_hash(dest):
    h = hashlib.sha256()
    n = 0
    for p in sorted(Path(dest).rglob("*")):
        if ".git" in p.relative_to(dest).parts or not p.is_file():
            continue
        h.update(str(p.relative_to(dest).as_posix()).encode())
        h.update(p.read_bytes())
        n += 1
    return n, h.hexdigest()


class MakeLargeTree(unittest.TestCase):
    def test_same_seed_gives_identical_files_and_hash(self):
        with tempfile.TemporaryDirectory() as a, tempfile.TemporaryDirectory() as b:
            run(Path(a) / "r", "--seed", "7")
            run(Path(b) / "r", "--seed", "7")
            na, ha = tree_hash(Path(a) / "r")
            nb, hb = tree_hash(Path(b) / "r")
            self.assertEqual(na, nb)
            self.assertEqual(ha, hb)
            self.assertGreaterEqual(na, 4000)

    def test_two_branches_differ_in_a_known_number_of_files(self):
        with tempfile.TemporaryDirectory() as t:
            dest = Path(t) / "r"
            out = run(dest, "--seed", "1", "--files", "400", "--differing", "50")
            self.assertIn("differing=50", out)
            branches = git(dest, "branch", "--format=%(refname:short)").split()
            self.assertEqual(sorted(branches), ["a", "b"])
            self.assertEqual(len(git(dest, "diff", "--name-only", "a", "b").splitlines()), 50)
            self.assertEqual(len(git(dest, "ls-files").splitlines()), 400)
            self.assertEqual(git(dest, "rev-parse", "--abbrev-ref", "HEAD"), "a")

    def test_a_different_seed_changes_the_contents(self):
        with tempfile.TemporaryDirectory() as a, tempfile.TemporaryDirectory() as b:
            run(Path(a) / "r", "--seed", "1", "--files", "50", "--differing", "5")
            run(Path(b) / "r", "--seed", "2", "--files", "50", "--differing", "5")
            self.assertNotEqual(tree_hash(Path(a) / "r"), tree_hash(Path(b) / "r"))

    def test_refuses_a_non_empty_destination(self):
        with tempfile.TemporaryDirectory() as t:
            (Path(t) / "x").write_text("x")
            r = subprocess.run([sys.executable, "-I", str(SCRIPT), t], capture_output=True, text=True)
            self.assertNotEqual(r.returncode, 0)


if __name__ == "__main__":
    unittest.main()
