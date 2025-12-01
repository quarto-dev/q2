#!/usr/bin/env python3
"""
CSL Test Helper - Development utility for quarto-citeproc CSL conformance testing.

This script provides tools for analyzing, running, and debugging CSL conformance tests.

Usage:
    csl-test-helper.py status              # Overall test status and category breakdown
    csl-test-helper.py category <name>     # Details for tests in a category
    csl-test-helper.py quick-wins          # Find tests that pass but aren't enabled
    csl-test-helper.py regressions         # Find enabled tests that fail
    csl-test-helper.py inspect <test>      # Show test details and Pandoc status
    csl-test-helper.py run <pattern>       # Run tests matching pattern
    csl-test-helper.py enable <test>...    # Add tests to enabled_tests.txt
    csl-test-helper.py defer <test>...     # Add tests to deferred_tests.txt
"""

import argparse
import os
import re
import subprocess
import sys
from collections import defaultdict
from dataclasses import dataclass
from pathlib import Path
from typing import Optional


# =============================================================================
# Configuration
# =============================================================================

# Paths relative to repo root
CITEPROC_CRATE = Path("crates/quarto-citeproc")
TEST_DATA_DIR = CITEPROC_CRATE / "test-data" / "csl-suite"
TESTS_DIR = CITEPROC_CRATE / "tests"
ENABLED_FILE = TESTS_DIR / "enabled_tests.txt"
DEFERRED_FILE = TESTS_DIR / "deferred_tests.txt"
LOCKFILE = TESTS_DIR / "csl_conformance.lock"
PANDOC_SPEC = Path("external-sources/citeproc/test/Spec.hs")


# =============================================================================
# Test Name Normalization
# =============================================================================

def normalize_test_name(name: str) -> str:
    """
    Normalize a test name for comparison.

    Handles:
    - Case insensitivity
    - Hyphens vs underscores (test files may have hyphens, function names use underscores)
    - .txt extension removal

    Returns lowercase with underscores.
    """
    name = name.lower()
    name = name.replace("-", "_")
    if name.endswith(".txt"):
        name = name[:-4]
    return name


def get_test_category(name: str) -> str:
    """
    Extract the category prefix from a test name.

    Examples:
        name_AfterInvertedName -> name
        date_LocalizedDateFormats-af-ZA -> date
        bugreports_ApostropheOnParticle -> bugreports
    """
    normalized = normalize_test_name(name)
    # Category is everything before the first underscore
    if "_" in normalized:
        return normalized.split("_")[0]
    return normalized


def get_test_filename(normalized_name: str, test_dir: Path) -> Optional[Path]:
    """
    Find the actual test file for a normalized test name.

    Since normalization loses case and hyphen information, we need to search
    the directory for a matching file.
    """
    for f in test_dir.iterdir():
        if f.suffix == ".txt" and normalize_test_name(f.stem) == normalized_name:
            return f
    return None


# =============================================================================
# File Reading
# =============================================================================

def read_test_list(filepath: Path) -> set[str]:
    """
    Read a test list file (enabled_tests.txt or deferred_tests.txt).

    Returns normalized test names, ignoring comments and blank lines.
    """
    tests = set()
    if not filepath.exists():
        return tests

    with open(filepath) as f:
        for line in f:
            line = line.strip()
            # Skip comments and blank lines
            if not line or line.startswith("#"):
                continue
            tests.add(normalize_test_name(line))

    return tests


def get_all_test_files(test_dir: Path) -> dict[str, Path]:
    """
    Get all test files in the test directory.

    Returns dict mapping normalized name -> file path.
    """
    tests = {}
    for f in test_dir.iterdir():
        if f.suffix == ".txt":
            normalized = normalize_test_name(f.stem)
            tests[normalized] = f
    return tests


def read_lockfile(filepath: Path) -> dict:
    """
    Parse the lockfile to get current counts.
    """
    info = {"total": 0, "enabled": 0, "disabled": 0}
    if not filepath.exists():
        return info

    with open(filepath) as f:
        for line in f:
            if line.startswith("# Suite total:"):
                info["total"] = int(line.split(":")[1].strip())
            elif line.startswith("# Enabled:"):
                info["enabled"] = int(line.split(":")[1].strip())
            elif line.startswith("# Disabled:"):
                info["disabled"] = int(line.split(":")[1].strip())

    return info


# =============================================================================
# Test File Parsing
# =============================================================================

@dataclass
class TestFile:
    """Parsed contents of a CSL test file."""
    name: str
    path: Path
    mode: str  # "citation" or "bibliography"
    expected: str
    csl: str
    input_json: str
    citation_items: Optional[str] = None
    citations: Optional[str] = None
    version: str = "1.0"


def parse_test_file(filepath: Path) -> TestFile:
    """
    Parse a CSL test file into its components.

    Test file format uses markers like:
        >>===== MODE =====>>
        citation
        <<===== MODE =====<<
    """
    content = filepath.read_text()

    def extract_section(name: str) -> Optional[str]:
        # Handle both ===== and ==== (some files use 4 equals)
        pattern = rf'>>==+\s*{name}\s*==+>>\s*\n(.*?)\n<<==+\s*{name}\s*==+<<'
        match = re.search(pattern, content, re.DOTALL | re.IGNORECASE)
        if match:
            return match.group(1).strip()
        return None

    return TestFile(
        name=filepath.stem,
        path=filepath,
        mode=extract_section("MODE") or "citation",
        expected=extract_section("RESULT") or "",
        csl=extract_section("CSL") or "",
        input_json=extract_section("INPUT") or "[]",
        citation_items=extract_section("CITATION-ITEMS"),
        citations=extract_section("CITATIONS"),
        version=extract_section("VERSION") or "1.0",
    )


# =============================================================================
# Pandoc Comparison
# =============================================================================

def check_pandoc_fails(test_name: str, spec_path: Path) -> Optional[str]:
    """
    Check if Pandoc's citeproc also fails this test.

    Returns the context line if found, None if not in expected failures.
    """
    if not spec_path.exists():
        return None

    normalized = normalize_test_name(test_name)
    content = spec_path.read_text()

    # Search for the test name in various forms
    for variant in [test_name, normalized, test_name.replace("_", "-")]:
        # Look for patterns like: it "testname" $ or testname `shouldBe`
        pattern = rf'^\s*.*{re.escape(variant)}.*$'
        for line in content.split('\n'):
            if re.search(variant, line, re.IGNORECASE):
                return line.strip()

    return None


# =============================================================================
# Test Running
# =============================================================================

def run_tests(pattern: str = "", include_ignored: bool = False,
              no_fail_fast: bool = False) -> tuple[set[str], set[str]]:
    """
    Run tests matching a pattern and return (passing, failing) sets.

    Returns normalized test names.
    """
    cmd = ["cargo", "nextest", "run", "-p", "quarto-citeproc"]

    if no_fail_fast:
        cmd.append("--no-fail-fast")

    if pattern:
        cmd.append(pattern)

    cmd.append("--")

    if include_ignored:
        cmd.append("--include-ignored")

    result = subprocess.run(
        cmd,
        capture_output=True,
        text=True,
        cwd=find_repo_root(),
    )

    output = result.stdout + result.stderr

    passing = set()
    failing = set()

    for line in output.split('\n'):
        # Match lines like: PASS [   0.009s] (1/932) quarto-citeproc::csl_conformance csl_name_test
        if match := re.search(r'PASS\s+\[.*?\].*csl_(\w+)', line):
            passing.add(normalize_test_name(match.group(1)))
        elif match := re.search(r'FAIL\s+\[.*?\].*csl_(\w+)', line):
            failing.add(normalize_test_name(match.group(1)))

    return passing, failing


def run_single_test(test_name: str, include_ignored: bool = True) -> tuple[bool, str]:
    """
    Run a single test and return (passed, output).
    """
    normalized = normalize_test_name(test_name)
    pattern = f"csl_{normalized}"

    cmd = ["cargo", "nextest", "run", "-p", "quarto-citeproc", pattern]
    if include_ignored:
        cmd.extend(["--", "--include-ignored"])

    result = subprocess.run(
        cmd,
        capture_output=True,
        text=True,
        cwd=find_repo_root(),
    )

    output = result.stdout + result.stderr
    passed = "PASS" in output and "FAIL" not in output

    return passed, output


# =============================================================================
# Utility Functions
# =============================================================================

def find_repo_root() -> Path:
    """Find the repository root (directory containing Cargo.toml)."""
    current = Path.cwd()
    while current != current.parent:
        if (current / "Cargo.toml").exists() and (current / "crates").exists():
            return current
        current = current.parent

    # Fallback: assume we're in the repo
    return Path.cwd()


def format_percentage(part: int, total: int) -> str:
    """Format a percentage string."""
    if total == 0:
        return "0.0%"
    return f"{100 * part / total:.1f}%"


# =============================================================================
# Commands
# =============================================================================

def cmd_status(args):
    """Show overall test status and category breakdown."""
    root = find_repo_root()

    all_tests = get_all_test_files(root / TEST_DATA_DIR)
    enabled = read_test_list(root / ENABLED_FILE)
    deferred = read_test_list(root / DEFERRED_FILE)
    lockfile = read_lockfile(root / LOCKFILE)

    total = len(all_tests)
    enabled_count = len(enabled)
    deferred_count = len(deferred)

    # Tests that are neither enabled nor deferred
    unknown = set(all_tests.keys()) - enabled - deferred
    unknown_count = len(unknown)

    print("=" * 60)
    print("CSL Conformance Test Status")
    print("=" * 60)
    print()
    print(f"Total test files:     {total}")
    print(f"Enabled (passing):    {enabled_count} ({format_percentage(enabled_count, total)})")
    print(f"Deferred (skipped):   {deferred_count} ({format_percentage(deferred_count, total)})")
    print(f"Unknown (to address): {unknown_count} ({format_percentage(unknown_count, total)})")
    print()

    if lockfile["enabled"] != enabled_count:
        print(f"WARNING: Lockfile says {lockfile['enabled']} enabled, but enabled_tests.txt has {enabled_count}")
        print()

    # Category breakdown for unknown tests
    if unknown:
        print("-" * 60)
        print("Unknown tests by category (not enabled or deferred):")
        print("-" * 60)

        by_category = defaultdict(list)
        for name in unknown:
            cat = get_test_category(name)
            by_category[cat].append(name)

        for cat in sorted(by_category.keys(), key=lambda c: -len(by_category[c])):
            count = len(by_category[cat])
            print(f"  {cat:25} {count:4} tests")
        print()

    # Summary of enabled by category
    print("-" * 60)
    print("Enabled tests by category:")
    print("-" * 60)

    by_category = defaultdict(int)
    for name in enabled:
        cat = get_test_category(name)
        by_category[cat] += 1

    for cat in sorted(by_category.keys(), key=lambda c: -by_category[c]):
        count = by_category[cat]
        print(f"  {cat:25} {count:4} tests")


def cmd_category(args):
    """Show details for tests in a specific category."""
    root = find_repo_root()
    category = args.name.lower()

    all_tests = get_all_test_files(root / TEST_DATA_DIR)
    enabled = read_test_list(root / ENABLED_FILE)
    deferred = read_test_list(root / DEFERRED_FILE)

    # Filter to category
    cat_tests = {name: path for name, path in all_tests.items()
                 if get_test_category(name) == category}

    if not cat_tests:
        print(f"No tests found in category '{category}'")
        print(f"Available categories: {sorted(set(get_test_category(n) for n in all_tests))}")
        return

    cat_enabled = [n for n in cat_tests if n in enabled]
    cat_deferred = [n for n in cat_tests if n in deferred]
    cat_unknown = [n for n in cat_tests if n not in enabled and n not in deferred]

    print(f"Category: {category}")
    print(f"Total: {len(cat_tests)}, Enabled: {len(cat_enabled)}, "
          f"Deferred: {len(cat_deferred)}, Unknown: {len(cat_unknown)}")
    print()

    if args.run:
        print("Running tests...")
        passing, failing = run_tests(f"csl_{category}_", include_ignored=True, no_fail_fast=True)

        # Filter to this category
        passing = {n for n in passing if get_test_category(n) == category}
        failing = {n for n in failing if get_test_category(n) == category}

        print(f"Results: {len(passing)} passing, {len(failing)} failing")
        print()

        # Show quick wins (passing but not enabled AND not deferred)
        quick_wins = passing - enabled - deferred
        if quick_wins:
            print("Quick wins (passing but not enabled or deferred):")
            for name in sorted(quick_wins):
                print(f"  + {name}")
            print()

        # Show regressions (enabled but failing)
        regressions = failing & enabled
        if regressions:
            print("REGRESSIONS (enabled but failing):")
            for name in sorted(regressions):
                print(f"  ! {name}")
            print()

    if cat_unknown and not args.run:
        print("Unknown tests (not enabled or deferred):")
        for name in sorted(cat_unknown):
            print(f"  ? {name}")
        print()
        print(f"Run with --run to check which pass/fail")


def cmd_quick_wins(args):
    """Find tests that pass but aren't enabled."""
    root = find_repo_root()

    enabled = read_test_list(root / ENABLED_FILE)
    deferred = read_test_list(root / DEFERRED_FILE)

    print("Running all tests (this may take a minute)...")
    passing, failing = run_tests(include_ignored=True, no_fail_fast=True)

    # Filter to CSL tests only (not unit tests)
    all_tests = get_all_test_files(root / TEST_DATA_DIR)
    passing = passing & set(all_tests.keys())

    # Quick wins are tests that pass but aren't enabled AND aren't deferred
    quick_wins = passing - enabled - deferred

    if not quick_wins:
        print("\nNo quick wins found - all passing tests are already enabled or deferred!")
        return

    print(f"\nFound {len(quick_wins)} quick wins (tests that pass but aren't enabled):")
    print()

    by_category = defaultdict(list)
    for name in quick_wins:
        cat = get_test_category(name)
        by_category[cat].append(name)

    for cat in sorted(by_category.keys()):
        print(f"{cat}:")
        for name in sorted(by_category[cat]):
            # Find original filename for proper casing
            filepath = get_test_filename(name, root / TEST_DATA_DIR)
            original_name = filepath.stem if filepath else name
            print(f"  {original_name}")

    print()
    print("To enable these tests:")
    print("  1. Add them to crates/quarto-citeproc/tests/enabled_tests.txt")
    print("  2. Run: UPDATE_CSL_LOCKFILE=1 cargo nextest run -p quarto-citeproc csl_validate_manifest")


def cmd_regressions(args):
    """Find enabled tests that fail."""
    root = find_repo_root()

    enabled = read_test_list(root / ENABLED_FILE)

    print("Running enabled tests...")
    # Don't include ignored - we only care about enabled tests
    result = subprocess.run(
        ["cargo", "nextest", "run", "-p", "quarto-citeproc"],
        capture_output=True,
        text=True,
        cwd=root,
    )

    output = result.stdout + result.stderr

    failing = set()
    for line in output.split('\n'):
        if match := re.search(r'FAIL\s+\[.*?\].*csl_(\w+)', line):
            failing.add(normalize_test_name(match.group(1)))

    regressions = failing & enabled

    if not regressions:
        print("\nNo regressions found - all enabled tests pass!")
        return

    print(f"\nFound {len(regressions)} regressions (enabled tests that fail):")
    for name in sorted(regressions):
        print(f"  ! {name}")


def cmd_inspect(args):
    """Show details for a specific test."""
    root = find_repo_root()

    normalized = normalize_test_name(args.test)
    filepath = get_test_filename(normalized, root / TEST_DATA_DIR)

    if not filepath:
        print(f"Test not found: {args.test}")
        return

    enabled = read_test_list(root / ENABLED_FILE)
    deferred = read_test_list(root / DEFERRED_FILE)

    # Status
    status = "unknown"
    if normalized in enabled:
        status = "enabled"
    elif normalized in deferred:
        status = "deferred"

    # Pandoc status
    pandoc_line = check_pandoc_fails(normalized, root / PANDOC_SPEC)

    # Parse test file
    test = parse_test_file(filepath)

    print("=" * 60)
    print(f"Test: {test.name}")
    print("=" * 60)
    print(f"File: {filepath}")
    print(f"Status: {status}")
    print(f"Mode: {test.mode}")
    print()

    if pandoc_line:
        print(f"Pandoc status: Also fails in Pandoc citeproc")
        print(f"  {pandoc_line}")
        print()
    else:
        print("Pandoc status: Not in Pandoc's expected failures (or not found)")
        print()

    # Run the test
    print("Running test...")
    passed, output = run_single_test(normalized)
    print(f"Result: {'PASS' if passed else 'FAIL'}")
    print()

    if not passed and args.diff:
        # Extract diff from output
        diff_match = re.search(
            r'--- Expected ---\n(.*?)\n\s*--- Actual ---\n(.*?)\n\s*--- Diff ---',
            output, re.DOTALL)
        if diff_match:
            print("-" * 60)
            print("Expected:")
            print("-" * 60)
            print(diff_match.group(1).strip())
            print()
            print("-" * 60)
            print("Actual:")
            print("-" * 60)
            print(diff_match.group(2).strip())
            print()

    if args.csl:
        print("-" * 60)
        print("CSL Style:")
        print("-" * 60)
        print(test.csl[:2000] + "..." if len(test.csl) > 2000 else test.csl)
        print()

    if args.input:
        print("-" * 60)
        print("Input JSON:")
        print("-" * 60)
        print(test.input_json[:2000] + "..." if len(test.input_json) > 2000 else test.input_json)


def cmd_run(args):
    """Run tests matching a pattern."""
    pattern = args.pattern

    print(f"Running tests matching: {pattern}")
    print()

    passing, failing = run_tests(pattern, include_ignored=args.include_ignored,
                                  no_fail_fast=True)

    print(f"\nResults: {len(passing)} passing, {len(failing)} failing")

    if failing and args.verbose:
        print("\nFailing tests:")
        for name in sorted(failing):
            print(f"  {name}")


def cmd_enable(args):
    """Add tests to enabled_tests.txt."""
    root = find_repo_root()
    enabled_file = root / ENABLED_FILE

    # Read current enabled tests
    current = set()
    lines = []
    if enabled_file.exists():
        with open(enabled_file) as f:
            lines = f.readlines()
            for line in lines:
                stripped = line.strip()
                if stripped and not stripped.startswith("#"):
                    current.add(normalize_test_name(stripped))

    # Find tests to add
    all_tests = get_all_test_files(root / TEST_DATA_DIR)
    to_add = []

    for test in args.tests:
        normalized = normalize_test_name(test)
        if normalized in current:
            print(f"Already enabled: {test}")
            continue

        # Find proper filename
        filepath = get_test_filename(normalized, root / TEST_DATA_DIR)
        if not filepath:
            print(f"Test not found: {test}")
            continue

        to_add.append(filepath.stem)

    if not to_add:
        print("No tests to add.")
        return

    # Append to file
    with open(enabled_file, 'a') as f:
        for name in to_add:
            f.write(f"{name}\n")
            print(f"Added: {name}")

    print()
    print("Now run: UPDATE_CSL_LOCKFILE=1 cargo nextest run -p quarto-citeproc csl_validate_manifest")


def cmd_defer(args):
    """Add tests to deferred_tests.txt with a reason."""
    root = find_repo_root()
    deferred_file = root / DEFERRED_FILE

    # Read current deferred tests
    current = set()
    if deferred_file.exists():
        with open(deferred_file) as f:
            for line in f:
                stripped = line.strip()
                if stripped and not stripped.startswith("#"):
                    current.add(normalize_test_name(stripped))

    # Find tests to add
    all_tests = get_all_test_files(root / TEST_DATA_DIR)
    to_add = []

    for test in args.tests:
        normalized = normalize_test_name(test)
        if normalized in current:
            print(f"Already deferred: {test}")
            continue

        # Find proper filename
        filepath = get_test_filename(normalized, root / TEST_DATA_DIR)
        if not filepath:
            print(f"Test not found: {test}")
            continue

        to_add.append(filepath.stem)

    if not to_add:
        print("No tests to add.")
        return

    # Append to file with reason comment
    with open(deferred_file, 'a') as f:
        if args.reason:
            f.write(f"\n# {args.reason}\n")
        for name in to_add:
            f.write(f"{name}\n")
            print(f"Deferred: {name}")

    print()
    print("Now run: UPDATE_CSL_LOCKFILE=1 cargo nextest run -p quarto-citeproc csl_validate_manifest")


# =============================================================================
# Main
# =============================================================================

def main():
    parser = argparse.ArgumentParser(
        description="CSL Test Helper - Development utility for quarto-citeproc",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  %(prog)s status                    # Show overall test status
  %(prog)s category name --run       # Run all name_ tests
  %(prog)s quick-wins                # Find tests that pass but aren't enabled
  %(prog)s inspect name_AsianGlyphs  # Show details for a specific test
  %(prog)s enable test1 test2        # Add tests to enabled list
  %(prog)s defer test1 -r "Also fails in Pandoc"  # Add tests to deferred list
        """
    )

    subparsers = parser.add_subparsers(dest="command", required=True)

    # status
    status_parser = subparsers.add_parser("status", help="Show overall test status")
    status_parser.set_defaults(func=cmd_status)

    # category
    cat_parser = subparsers.add_parser("category", help="Show tests in a category")
    cat_parser.add_argument("name", help="Category name (e.g., name, date, sort)")
    cat_parser.add_argument("--run", action="store_true", help="Run tests to check pass/fail")
    cat_parser.set_defaults(func=cmd_category)

    # quick-wins
    qw_parser = subparsers.add_parser("quick-wins", help="Find passing tests not enabled")
    qw_parser.set_defaults(func=cmd_quick_wins)

    # regressions
    reg_parser = subparsers.add_parser("regressions", help="Find enabled tests that fail")
    reg_parser.set_defaults(func=cmd_regressions)

    # inspect
    insp_parser = subparsers.add_parser("inspect", help="Show details for a test")
    insp_parser.add_argument("test", help="Test name")
    insp_parser.add_argument("--diff", action="store_true", help="Show expected vs actual")
    insp_parser.add_argument("--csl", action="store_true", help="Show CSL style")
    insp_parser.add_argument("--input", action="store_true", help="Show input JSON")
    insp_parser.set_defaults(func=cmd_inspect)

    # run
    run_parser = subparsers.add_parser("run", help="Run tests matching pattern")
    run_parser.add_argument("pattern", help="Test pattern (e.g., csl_name_)")
    run_parser.add_argument("--include-ignored", action="store_true",
                           help="Include disabled tests")
    run_parser.add_argument("-v", "--verbose", action="store_true",
                           help="Show failing test names")
    run_parser.set_defaults(func=cmd_run)

    # enable
    enable_parser = subparsers.add_parser("enable", help="Add tests to enabled list")
    enable_parser.add_argument("tests", nargs="+", help="Test names to enable")
    enable_parser.set_defaults(func=cmd_enable)

    # defer
    defer_parser = subparsers.add_parser("defer", help="Add tests to deferred list")
    defer_parser.add_argument("tests", nargs="+", help="Test names to defer")
    defer_parser.add_argument("-r", "--reason", help="Reason for deferring (added as comment)")
    defer_parser.set_defaults(func=cmd_defer)

    args = parser.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
