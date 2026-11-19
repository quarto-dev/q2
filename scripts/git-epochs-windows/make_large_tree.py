#!/usr/bin/env python3
"""Builds a deterministic git repository with a large tree and two branches (temporary tooling, never lands).

    uv run scripts/git-epochs-windows/make_large_tree.py DEST [--seed N] [--files 4000] [--differing 1200]

DEST must not exist or must be empty. The repository has branches `a` and `b` (checked out on `a`); exactly
`--differing` files differ between them. The same seed gives identical file contents; commit hashes differ because
git records the time, so tests compare the files, not the commits. Standard library only.
"""
import argparse
import os
import random
import subprocess
import sys
from pathlib import Path


def git(dest, *args):
    env = dict(os.environ, GIT_TERMINAL_PROMPT="0",
               GIT_AUTHOR_NAME="T", GIT_AUTHOR_EMAIL="t@example.com",
               GIT_COMMITTER_NAME="T", GIT_COMMITTER_EMAIL="t@example.com")
    subprocess.run(["git", "-c", "core.autocrlf=false", *args], cwd=dest, check=True, env=env,
                   stdout=subprocess.DEVNULL)


def write_tree(dest: Path, variant: str, files: int, differing: int, seed: int) -> None:
    for i in range(files):
        rng = random.Random(f"{seed}:{i}:{variant if i < differing else 'shared'}")
        body = f"file {i} variant {variant if i < differing else 'shared'}\n"
        body += "".join(rng.choice("abcdefghijklmnopqrstuvwxyz ") for _ in range(600)) + "\n"
        sub = dest / f"d{i % 40:02d}"
        sub.mkdir(exist_ok=True)
        (sub / f"f{i:04d}.qmd").write_text(body, encoding="utf-8", newline="\n")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("dest")
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--files", type=int, default=4000)
    ap.add_argument("--differing", type=int, default=1200)
    a = ap.parse_args()
    if a.differing > a.files:
        sys.exit("--differing cannot exceed --files")
    dest = Path(a.dest)
    if dest.exists() and any(dest.iterdir()):
        sys.exit(f"{dest} exists and is not empty")
    dest.mkdir(parents=True, exist_ok=True)
    git(dest, "init", "-q", "-b", "a")
    write_tree(dest, "a", a.files, a.differing, a.seed)
    git(dest, "add", "-A")
    git(dest, "commit", "-q", "-m", "tree a")
    git(dest, "switch", "-q", "-c", "b")
    write_tree(dest, "b", a.files, a.differing, a.seed)
    git(dest, "add", "-A")
    git(dest, "commit", "-q", "-m", "tree b")
    git(dest, "switch", "-q", "a")
    print(f"files={a.files} differing={a.differing} seed={a.seed} repo={dest}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
