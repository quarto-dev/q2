#!/usr/bin/env python3
"""
Interactive utility to migrate tree-sitter tests after grammar refactoring.

This script reads an old test file, runs tree-sitter parse on each test,
and allows the user to interactively decide which tests to keep.
"""

import argparse
import re
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import List, Tuple


class TestEntry:
    """Represents a single test entry from the test file."""

    def __init__(self, name: str, input_text: str):
        self.name = name
        self.input_text = input_text

    def __repr__(self):
        return f"TestEntry(name={self.name!r}, input_length={len(self.input_text)})"


def parse_test_file(file_path: Path) -> List[TestEntry]:
    """
    Parse a tree-sitter test file into individual test entries.

    Returns a list of TestEntry objects containing test name and input text.

    Format:
    ================================================================================
    Test Name
    ================================================================================
    Input text

    --------------------------------------------------------------------------------

    Parse tree
    """
    with open(file_path, 'r', encoding='utf-8') as f:
        content = f.read()

    tests = []

    # Use regex to find test entries
    # Pattern: separator, name, separator, input, separator
    pattern = r'^={80}\n(.+?)\n={80}\n(.*?)\n-{80}\n'

    matches = re.finditer(pattern, content, flags=re.MULTILINE | re.DOTALL)

    for match in matches:
        name = match.group(1).strip()
        input_text = match.group(2)  # Preserve trailing newlines - they're part of the test input

        tests.append(TestEntry(name, input_text))

    return tests


def run_tree_sitter_parse(input_text: str) -> str:
    """
    Run 'tree-sitter parse --no-ranges' on the input text.

    Returns the parse tree output as a string.
    """
    # Create a temporary file with the input
    with tempfile.NamedTemporaryFile(mode='w', suffix='.md', delete=False, encoding='utf-8') as tmp:
        tmp.write(input_text)
        tmp_path = tmp.name

    try:
        # Run tree-sitter parse
        result = subprocess.run(
            ['tree-sitter', 'parse', '--no-ranges', tmp_path],
            capture_output=True,
            text=True,
            check=True
        )
        return result.stdout.strip()
    except subprocess.CalledProcessError as e:
        return f"ERROR: tree-sitter parse failed\n{e.stderr}"
    finally:
        # Clean up temporary file
        Path(tmp_path).unlink(missing_ok=True)


def format_test_entry(name: str, input_text: str, parse_tree: str) -> str:
    """
    Format a test entry in tree-sitter test file format.
    """
    lines = [
        '=' * 80,
        name,
        '=' * 80,
        input_text,
        '',
        '-' * 80,
        '',
        parse_tree,
        '',
    ]
    return '\n'.join(lines)


def display_test(test_num: int, total_tests: int, test: TestEntry, parse_tree: str):
    """
    Display a test entry for user review.
    """
    print("\n" + "=" * 80)
    print(f"Test {test_num}/{total_tests}")
    print("=" * 80)
    print(f"\nTest Name: {test.name}")
    print("\nInput:")
    print("-" * 40)
    print(test.input_text)
    print("-" * 40)
    print("\nNew Parse Tree:")
    print("-" * 40)
    print(parse_tree)
    print("-" * 40)


def interactive_review(tests: List[TestEntry], output_path: Path):
    """
    Interactively review tests and write accepted ones to output file.
    """
    total = len(tests)
    accepted = 0
    skipped = 0

    # Open output file in append mode
    with open(output_path, 'a', encoding='utf-8') as out:
        for i, test in enumerate(tests, 1):
            # Run tree-sitter parse
            parse_tree = run_tree_sitter_parse(test.input_text)

            # Display the test
            display_test(i, total, test, parse_tree)

            # Prompt user
            while True:
                response = input("\nInclude this test? (y/n/q to quit): ").strip().lower()

                if response == 'y':
                    # Write to output file
                    formatted = format_test_entry(test.name, test.input_text, parse_tree)
                    out.write(formatted)
                    out.flush()  # Ensure it's written
                    accepted += 1
                    print(f"✓ Test added ({accepted} accepted, {skipped} skipped)")
                    break
                elif response == 'n':
                    skipped += 1
                    print(f"✗ Test skipped ({accepted} accepted, {skipped} skipped)")
                    break
                elif response == 'q':
                    print(f"\nQuitting. Progress saved: {accepted} accepted, {skipped} skipped")
                    return
                else:
                    print("Please enter 'y', 'n', or 'q'")

    print(f"\n{'=' * 80}")
    print(f"Migration complete!")
    print(f"Total tests: {total}")
    print(f"Accepted: {accepted}")
    print(f"Skipped: {skipped}")
    print(f"Output written to: {output_path}")


def main():
    parser = argparse.ArgumentParser(
        description='Interactively migrate tree-sitter tests after grammar refactoring'
    )
    parser.add_argument(
        'input_file',
        type=Path,
        help='Path to old test file (e.g., spec.txt)'
    )
    parser.add_argument(
        'output_file',
        type=Path,
        help='Path to new test file (will be created/appended)'
    )

    args = parser.parse_args()

    # Validate input file exists
    if not args.input_file.exists():
        print(f"Error: Input file not found: {args.input_file}", file=sys.stderr)
        sys.exit(1)

    # Warn if output file exists
    if args.output_file.exists():
        response = input(f"Warning: {args.output_file} exists. Append to it? (y/n): ")
        if response.strip().lower() != 'y':
            print("Aborted.")
            sys.exit(0)

    # Parse the test file
    print(f"Parsing test file: {args.input_file}")
    tests = parse_test_file(args.input_file)
    print(f"Found {len(tests)} tests")

    if not tests:
        print("No tests found in input file")
        sys.exit(1)

    # Start interactive review
    try:
        interactive_review(tests, args.output_file)
    except KeyboardInterrupt:
        print("\n\nInterrupted by user. Progress has been saved.")
        sys.exit(0)


if __name__ == '__main__':
    main()
