#!/usr/bin/env python3
"""
Error Code Audit Script

Audits error code consistency between error_catalog.json and source code.
This script performs the mechanical phases of the workflow:
- Phase 2: Find all code references
- Phase 3: Compare and classify
- Phase 6: Generate audit report

Requirements:
    - Python 3.6 or later
    - ripgrep (rg command) - REQUIRED for fast code searching
      Install: https://github.com/BurntSushi/ripgrep#installation
        macOS:   brew install ripgrep
        Ubuntu:  apt install ripgrep
        Other:   See link above

Usage:
    ./scripts/audit-error-codes.py [--format json|markdown|text] [--output FILE]

Ignore Markers:
    Line-level: Add `quarto-error-code-audit-ignore` as a comment on the same
    line as an error code to exclude it from audit results.

    Examples:
        "Q-999-999"  // quarto-error-code-audit-ignore
        assert_eq!(get_subsystem("Q-999-999"), None); // quarto-error-code-audit-ignore
        # Test invalid code Q-999-999  # quarto-error-code-audit-ignore

    File-level: Add `quarto-error-code-audit-ignore-file` anywhere in a file
    (usually at the top) to ignore ALL error codes in that file.

    Examples:
        // quarto-error-code-audit-ignore-file
        <!-- quarto-error-code-audit-ignore-file -->
        # quarto-error-code-audit-ignore-file

    Useful for test data files, example documentation, or design documents
    that reference many error codes.
"""

import argparse
import json
import re
import subprocess
import sys
from collections import defaultdict
from dataclasses import dataclass, field, asdict
from pathlib import Path
from typing import Dict, List, Set, Optional


@dataclass
class CodeLocation:
    """A location where an error code is used."""
    file: str
    line: int
    context: str
    ignored: bool = False  # True if line has ignore marker

    def __hash__(self):
        return hash((self.file, self.line))


@dataclass
class CodeUsage:
    """Information about how an error code is used."""
    code: str
    locations: List[CodeLocation] = field(default_factory=list)

    @property
    def count(self) -> int:
        """Count of non-ignored locations."""
        return len([loc for loc in self.locations if not loc.ignored])

    @property
    def all_ignored(self) -> bool:
        """True if all locations have ignore marker."""
        return all(loc.ignored for loc in self.locations) if self.locations else False

    @property
    def files(self) -> Set[str]:
        """Set of files with non-ignored locations."""
        return {loc.file for loc in self.locations if not loc.ignored}

    @property
    def first_non_ignored_location(self) -> Optional[CodeLocation]:
        """Get the first non-ignored location."""
        for loc in self.locations:
            if not loc.ignored:
                return loc
        return None

    @property
    def subsystem_num(self) -> Optional[int]:
        """Extract subsystem number from code (e.g., Q-2-5 -> 2)."""
        match = re.match(r'Q-(\d+)-\d+', self.code)
        return int(match.group(1)) if match else None

    @property
    def subsystem_name(self) -> Optional[str]:
        """Map subsystem number to name."""
        mapping = {
            0: "internal",
            1: "yaml",
            2: "markdown",
            3: "writer"
        }
        num = self.subsystem_num
        return mapping.get(num) if num is not None else None


@dataclass
class CatalogEntry:
    """An entry in error_catalog.json."""
    code: str
    subsystem: str
    title: str
    message_template: str
    docs_url: str
    since_version: str


@dataclass
class AuditResults:
    """Results of the error code audit."""
    catalog_codes: Set[str]
    source_codes: Dict[str, CodeUsage]

    # Derived sets
    consistent_codes: Set[str] = field(init=False)
    missing_codes: Set[str] = field(init=False)
    orphaned_codes: Set[str] = field(init=False)

    # Categorized missing codes
    legitimate_missing: Dict[str, CodeUsage] = field(default_factory=dict)
    test_example_codes: Dict[str, CodeUsage] = field(default_factory=dict)
    invalid_format_codes: Dict[str, CodeUsage] = field(default_factory=dict)

    def __post_init__(self):
        # Filter out codes where ALL locations are ignored
        active_source_codes = {
            code: usage
            for code, usage in self.source_codes.items()
            if not usage.all_ignored
        }

        source_set = set(active_source_codes.keys())
        self.consistent_codes = self.catalog_codes & source_set
        self.missing_codes = source_set - self.catalog_codes
        self.orphaned_codes = self.catalog_codes - source_set

        # Update source_codes to only include active ones for reporting
        self.source_codes = active_source_codes

        # Categorize missing codes
        self._categorize_missing()

    def _categorize_missing(self):
        """Categorize missing codes by type."""
        for code in self.missing_codes:
            usage = self.source_codes[code]

            # Check if it's a test/example code
            if self._is_test_or_example(usage):
                self.test_example_codes[code] = usage
            # Check if it has formatting issues
            elif self._has_format_issues(code):
                self.invalid_format_codes[code] = usage
            # Otherwise it's a legitimate missing code
            else:
                self.legitimate_missing[code] = usage

    def _is_test_or_example(self, usage: CodeUsage) -> bool:
        """Check if a code is only used in tests or documentation."""
        for loc in usage.locations:
            path_lower = loc.file.lower()
            # If ANY location is in production code, not just test/example
            if not any(x in path_lower for x in [
                '/test', 'tests/', 'claude-notes/', '/doc', 'example',
                'README', '.md', 'snapshot'
            ]):
                return False
        return True

    def _has_format_issues(self, code: str) -> bool:
        """Check if code has formatting issues."""
        # Leading zeros (Q-1-010) (quarto-error-code-audit-ignore)
        if re.search(r'Q-\d+-0\d+', code):
            return True
        # Very high numbers (likely test data)
        if re.search(r'Q-\d+-(\d{4,})', code):
            return True
        # Invalid subsystems (Q-4+)
        if re.match(r'Q-([4-9]|\d{2,})-', code):
            return True
        # Test sentinel values
        if code in ['Q-999-999', 'Q-9999-9999']: # quarto-error-code-audit-ignore
            return True
        return False

    def get_stats(self) -> Dict:
        """Get summary statistics."""
        return {
            'catalog_total': len(self.catalog_codes),
            'source_total': len(self.source_codes),
            'consistent': len(self.consistent_codes),
            'missing_total': len(self.missing_codes),
            'missing_legitimate': len(self.legitimate_missing),
            'missing_test_example': len(self.test_example_codes),
            'missing_invalid_format': len(self.invalid_format_codes),
            'orphaned': len(self.orphaned_codes),
        }

    def get_subsystem_breakdown(self) -> Dict:
        """Get breakdown by subsystem."""
        breakdown = {}

        for subsystem_num in [0, 1, 2, 3]:
            subsystem_name = {
                0: 'internal', 1: 'yaml', 2: 'markdown', 3: 'writer'
            }[subsystem_num]

            prefix = f"Q-{subsystem_num}-"
            catalog_count = len([c for c in self.catalog_codes if c.startswith(prefix)])
            source_count = len([c for c in self.source_codes if c.startswith(prefix)])

            breakdown[subsystem_name] = {
                'prefix': prefix,
                'catalog': catalog_count,
                'source': source_count,
                'gap': source_count - catalog_count
            }

        return breakdown


class ErrorCodeAuditor:
    """Main auditor class."""

    def __init__(self, repo_root: Path):
        self.repo_root = repo_root
        self.catalog_path = repo_root / "crates/quarto-error-reporting/error_catalog.json"

    def run(self) -> AuditResults:
        """Run the complete audit."""
        print("📚 Loading error catalog...", file=sys.stderr)
        catalog_codes = self._load_catalog()
        print(f"   Found {len(catalog_codes)} codes in catalog", file=sys.stderr)

        print("🔍 Searching source code...", file=sys.stderr)
        source_codes = self._search_source()
        print(f"   Found {len(source_codes)} unique codes in source", file=sys.stderr)

        print("📊 Analyzing results...", file=sys.stderr)
        results = AuditResults(catalog_codes=catalog_codes, source_codes=source_codes)

        return results

    def _load_catalog(self) -> Set[str]:
        """Load error codes from catalog."""
        if not self.catalog_path.exists():
            raise FileNotFoundError(f"Catalog not found: {self.catalog_path}")

        with open(self.catalog_path) as f:
            catalog = json.load(f)

        return set(catalog.keys())

    def _search_source(self) -> Dict[str, CodeUsage]:
        """Search source code for error codes."""
        # Use ripgrep for fast searching
        cmd = [
            'rg',
            r'Q-\d+-\d+',  # Pattern
            '--type', 'rust',
            '--type', 'json',
            '--type', 'markdown',
            '--json',  # JSON output for parsing
            '--glob', '!target/',
            '--glob', '!external-sources/',
            '--glob', '!external-sites/',
        ]

        try:
            result = subprocess.run(
                cmd,
                cwd=self.repo_root,
                capture_output=True,
                text=True,
                check=False  # Don't raise on non-zero exit (no matches)
            )
        except FileNotFoundError:
            print("❌ Error: 'rg' (ripgrep) command not found.", file=sys.stderr)
            print("", file=sys.stderr)
            print("This script requires ripgrep for fast code searching.", file=sys.stderr)
            print("Install: https://github.com/BurntSushi/ripgrep#installation", file=sys.stderr)
            print("", file=sys.stderr)
            print("Quick install:", file=sys.stderr)
            print("  macOS:   brew install ripgrep", file=sys.stderr)
            print("  Ubuntu:  apt install ripgrep", file=sys.stderr)
            print("  Fedora:  dnf install ripgrep", file=sys.stderr)
            print("  Windows: choco install ripgrep", file=sys.stderr)
            sys.exit(1)

        # Parse ripgrep JSON output
        codes: Dict[str, CodeUsage] = {}
        files_with_matches: Set[str] = set()

        for line in result.stdout.splitlines():
            if not line.strip():
                continue

            try:
                entry = json.loads(line)
            except json.JSONDecodeError:
                continue

            # Only process match entries
            if entry.get('type') != 'match':
                continue

            data = entry['data']
            file_path = data['path']['text']
            line_num = data['line_number']
            line_text = data['lines']['text']

            # Track files that have matches
            files_with_matches.add(file_path)

            # Check if line has ignore marker
            has_ignore = 'quarto-error-code-audit-ignore' in line_text

            # Extract all Q-*-* codes from this line
            found_codes = re.findall(r'Q-\d+-\d+', line_text)

            for code in found_codes:
                if code not in codes:
                    codes[code] = CodeUsage(code=code)

                codes[code].locations.append(CodeLocation(
                    file=file_path,
                    line=line_num,
                    context=line_text.strip()[:100],  # First 100 chars
                    ignored=has_ignore
                ))

        # Post-process: Check for file-level ignore markers
        # Only check files that had matches (for performance)
        file_ignore_cache = self._check_file_ignores(files_with_matches)

        # Mark all locations in ignored files
        for usage in codes.values():
            for location in usage.locations:
                if file_ignore_cache.get(location.file, False):
                    location.ignored = True

        return codes

    def _check_file_ignores(self, files: Set[str]) -> Dict[str, bool]:
        """Check which files have file-level ignore markers.

        Returns dict mapping file path -> has_ignore_marker.
        Only checks files in the input set for performance.
        """
        cache = {}
        for file_path in files:
            full_path = self.repo_root / file_path
            try:
                # Read file and check for marker
                with open(full_path, 'r', encoding='utf-8', errors='ignore') as f:
                    content = f.read()
                    cache[file_path] = 'quarto-error-code-audit-ignore-file' in content
            except (IOError, OSError):
                # If we can't read the file, assume not ignored
                cache[file_path] = False

        return cache


class ReportFormatter:
    """Format audit results in various formats."""

    @staticmethod
    def format_text(results: AuditResults) -> str:
        """Format as plain text."""
        lines = []
        stats = results.get_stats()

        lines.append("=" * 60)
        lines.append("ERROR CODE AUDIT RESULTS")
        lines.append("=" * 60)
        lines.append("")

        # Summary
        lines.append("SUMMARY")
        lines.append("-" * 60)
        lines.append(f"  Codes in catalog:    {stats['catalog_total']}")
        lines.append(f"  Codes in source:     {stats['source_total']}")
        lines.append(f"  Consistent:          {stats['consistent']} ✅")
        lines.append(f"  Missing (catalog):   {stats['missing_total']} " +
                    ("❌" if stats['missing_total'] > 0 else "✅"))
        lines.append(f"    - Legitimate:      {stats['missing_legitimate']} (HIGH PRIORITY)")
        lines.append(f"    - Test/Examples:   {stats['missing_test_example']} (LOW PRIORITY)")
        lines.append(f"    - Invalid format:  {stats['missing_invalid_format']} (INVESTIGATE)")
        lines.append(f"  Orphaned (unused):   {stats['orphaned']} " +
                    ("⚠️" if stats['orphaned'] > 0 else "✅"))
        lines.append("")

        # Subsystem breakdown
        lines.append("SUBSYSTEM BREAKDOWN")
        lines.append("-" * 60)
        breakdown = results.get_subsystem_breakdown()
        for name, data in breakdown.items():
            lines.append(f"  {name:10s} ({data['prefix']}*)")
            lines.append(f"    Catalog: {data['catalog']:3d}  Source: {data['source']:3d}  "
                        f"Gap: {data['gap']:+3d}")
        lines.append("")

        # Legitimate missing codes
        if results.legitimate_missing:
            lines.append("LEGITIMATE MISSING CODES (HIGH PRIORITY)")
            lines.append("-" * 60)
            lines.append("Add these to error_catalog.json:")
            lines.append("")

            for code in sorted(results.legitimate_missing.keys()):
                usage = results.legitimate_missing[code]
                lines.append(f"  • {code}")
                lines.append(f"    Occurrences: {usage.count}")
                lines.append(f"    Files: {len(usage.files)}")
                # Show first non-ignored location
                loc = usage.first_non_ignored_location
                if loc:
                    lines.append(f"    First use: {loc.file}:{loc.line}")
            lines.append("")

        # Test/example codes
        if results.test_example_codes:
            lines.append("TEST/EXAMPLE CODES (LOW PRIORITY)")
            lines.append("-" * 60)
            lines.append("Used only in tests, docs, or examples:")
            lines.append("")
            for code in sorted(results.test_example_codes.keys()):
                usage = results.test_example_codes[code]
                lines.append(f"  • {code} ({usage.count} occurrences)")
            lines.append("")

        # Invalid format codes
        if results.invalid_format_codes:
            lines.append("INVALID FORMAT CODES (INVESTIGATE)")
            lines.append("-" * 60)
            lines.append("These may be typos, test data, or need cleanup:")
            lines.append("")
            for code in sorted(results.invalid_format_codes.keys()):
                usage = results.invalid_format_codes[code]
                lines.append(f"  • {code} ({usage.count} occurrences)")
                loc = usage.first_non_ignored_location
                if loc:
                    lines.append(f"    Example: {loc.file}:{loc.line}")
            lines.append("")

        # Orphaned codes
        if results.orphaned_codes:
            lines.append("ORPHANED CODES (IN CATALOG BUT NOT USED)")
            lines.append("-" * 60)
            lines.append("Consider removing or documenting:")
            lines.append("")
            for code in sorted(results.orphaned_codes):
                lines.append(f"  • {code}")
            lines.append("")

        lines.append("=" * 60)

        return "\n".join(lines)

    @staticmethod
    def format_json(results: AuditResults) -> str:
        """Format as JSON."""
        output = {
            'summary': results.get_stats(),
            'subsystem_breakdown': results.get_subsystem_breakdown(),
            'consistent_codes': sorted(results.consistent_codes),
            'legitimate_missing': {
                code: {
                    'count': usage.count,
                    'files': sorted(usage.files),
                    'locations': [
                        {'file': loc.file, 'line': loc.line, 'context': loc.context}
                        for loc in usage.locations
                    ]
                }
                for code, usage in results.legitimate_missing.items()
            },
            'test_example_codes': sorted(results.test_example_codes.keys()),
            'invalid_format_codes': sorted(results.invalid_format_codes.keys()),
            'orphaned_codes': sorted(results.orphaned_codes),
        }

        return json.dumps(output, indent=2)

    @staticmethod
    def format_markdown(results: AuditResults) -> str:
        """Format as Markdown."""
        lines = []
        stats = results.get_stats()

        lines.append("# Error Code Audit Report")
        lines.append("")
        lines.append("## Summary")
        lines.append("")
        lines.append("| Metric | Count | Status |")
        lines.append("|--------|-------|--------|")
        lines.append(f"| Codes in catalog | {stats['catalog_total']} | ✅ |")
        lines.append(f"| Codes in source | {stats['source_total']} | ⚠️ |")
        lines.append(f"| Consistent | {stats['consistent']} | ✅ |")
        lines.append(f"| Missing from catalog | {stats['missing_total']} | "
                    f"{'❌' if stats['missing_total'] > 0 else '✅'} |")
        lines.append(f"| - Legitimate missing | {stats['missing_legitimate']} | ❌ HIGH |")
        lines.append(f"| - Test/Example codes | {stats['missing_test_example']} | ℹ️ LOW |")
        lines.append(f"| - Invalid format | {stats['missing_invalid_format']} | ⚠️ INVESTIGATE |")
        lines.append(f"| Orphaned in catalog | {stats['orphaned']} | "
                    f"{'⚠️' if stats['orphaned'] > 0 else '✅'} |")
        lines.append("")

        # Subsystem breakdown
        lines.append("## Subsystem Breakdown")
        lines.append("")
        lines.append("| Subsystem | Catalog | Source | Gap |")
        lines.append("|-----------|---------|--------|-----|")
        breakdown = results.get_subsystem_breakdown()
        for name, data in breakdown.items():
            gap_str = f"{data['gap']:+d}" if data['gap'] != 0 else "0"
            lines.append(f"| {name} ({data['prefix']}*) | {data['catalog']} | "
                        f"{data['source']} | {gap_str} |")
        lines.append("")

        # Legitimate missing
        if results.legitimate_missing:
            lines.append("## Legitimate Missing Codes (HIGH PRIORITY)")
            lines.append("")
            lines.append("These codes are used in production code but missing from catalog:")
            lines.append("")
            lines.append("| Code | Occurrences | Files | Example Location |")
            lines.append("|------|-------------|-------|------------------|")
            for code in sorted(results.legitimate_missing.keys()):
                usage = results.legitimate_missing[code]
                loc = usage.first_non_ignored_location
                loc_str = f"{loc.file}:{loc.line}" if loc else "N/A"
                lines.append(f"| `{code}` | {usage.count} | {len(usage.files)} | {loc_str} |")
            lines.append("")

        # Test/example codes
        if results.test_example_codes:
            lines.append("## Test/Example Codes (LOW PRIORITY)")
            lines.append("")
            lines.append("Used only in tests, documentation, or examples:")
            lines.append("")
            for code in sorted(results.test_example_codes.keys()):
                usage = results.test_example_codes[code]
                lines.append(f"- `{code}` ({usage.count} occurrences)")
            lines.append("")

        # Invalid format
        if results.invalid_format_codes:
            lines.append("## Invalid Format Codes (INVESTIGATE)")
            lines.append("")
            lines.append("These may be typos, test data, or need cleanup:")
            lines.append("")
            for code in sorted(results.invalid_format_codes.keys()):
                usage = results.invalid_format_codes[code]
                loc = usage.first_non_ignored_location
                loc_str = f" - Example: {loc.file}:{loc.line}" if loc else ""
                lines.append(f"- `{code}` ({usage.count} occurrences){loc_str}")
            lines.append("")

        # Orphaned
        if results.orphaned_codes:
            lines.append("## Orphaned Codes (IN CATALOG BUT NOT USED)")
            lines.append("")
            lines.append("Consider removing or documenting:")
            lines.append("")
            for code in sorted(results.orphaned_codes):
                lines.append(f"- `{code}`")
            lines.append("")

        return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser(
        description='Audit error code consistency between catalog and source code'
    )
    parser.add_argument(
        '--format',
        choices=['text', 'json', 'markdown'],
        default='text',
        help='Output format (default: text)'
    )
    parser.add_argument(
        '--output', '-o',
        type=Path,
        help='Output file (default: stdout)'
    )
    parser.add_argument(
        '--repo-root',
        type=Path,
        default=None,
        help='Repository root directory (default: auto-detect from script location)'
    )

    args = parser.parse_args()

    # Determine repo root
    if args.repo_root:
        repo_root = args.repo_root.resolve()
    else:
        # Assume script is in scripts/ directory
        script_dir = Path(__file__).parent
        repo_root = script_dir.parent

    if not repo_root.exists():
        print(f"❌ Error: Repository root not found: {repo_root}", file=sys.stderr)
        sys.exit(1)

    # Run audit
    auditor = ErrorCodeAuditor(repo_root)
    try:
        results = auditor.run()
    except Exception as e:
        print(f"❌ Error during audit: {e}", file=sys.stderr)
        sys.exit(1)

    # Format output
    formatter = ReportFormatter()
    if args.format == 'json':
        output = formatter.format_json(results)
    elif args.format == 'markdown':
        output = formatter.format_markdown(results)
    else:  # text
        output = formatter.format_text(results)

    # Write output
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        with open(args.output, 'w') as f:
            f.write(output)
        print(f"✅ Report written to: {args.output}", file=sys.stderr)
    else:
        print(output)

    # Exit with error code if issues found
    stats = results.get_stats()
    if stats['missing_legitimate'] > 0 or stats['orphaned'] > 0:
        sys.exit(1)
    else:
        sys.exit(0)


if __name__ == '__main__':
    main()
