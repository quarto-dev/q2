"""`python -m quarto2 ...` runs the installed q2 binary."""

import os
import subprocess
import sys

from quarto2 import binary_path


def main():
    path = binary_path()
    if os.name == "posix":
        # Replace this process: signals, exit status and the terminal go
        # straight to q2.
        os.execv(path, [path, *sys.argv[1:]])
    try:
        sys.exit(subprocess.call([path, *sys.argv[1:]]))
    except KeyboardInterrupt:
        sys.exit(130)


if __name__ == "__main__":
    main()
