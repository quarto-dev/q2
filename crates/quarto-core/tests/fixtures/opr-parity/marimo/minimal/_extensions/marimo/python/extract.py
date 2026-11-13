#!/usr/bin/env python3

"""Run the packaged Quarto marimo compiler."""

from __future__ import annotations

from quarto_marimo.cli import main

__version__ = "0.7.0"


if __name__ == "__main__":
    raise SystemExit(main())
