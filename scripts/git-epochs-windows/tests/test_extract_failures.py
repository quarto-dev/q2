"""Tests for extract_failures.py (temporary Windows tooling, never lands)."""
import importlib.util
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("extract_failures", HERE.parent / "extract_failures.py")
ef = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ef)
LOG = HERE / "fixtures" / "nextest-failures.log"


class ExtractFailures(unittest.TestCase):
    def test_three_failures_with_output_and_the_summary(self):
        blocks, summary = ef.parse_nextest(LOG)
        self.assertEqual(len(blocks), 3)
        self.assertEqual(summary, ["Summary [   1.000s] 9 tests run: 6 passed, 3 failed, 0 skipped"])

    def test_groups_by_first_panic_message_with_temp_names_normalized(self):
        blocks, _ = ef.parse_nextest(LOG)
        groups = ef.group_by_message(blocks)
        sizes = sorted(len(v) for v in groups.values())
        self.assertEqual(sizes, [1, 2])  # the two drive-letter failures share a cause once <tmp> is normalized
        shared = [v for v in groups.values() if len(v) == 2][0]
        self.assertEqual({b["module"] for b in shared}, {"quarto-git-epochs::integration probe_real_repo"})

    def test_per_module_counts(self):
        blocks, _ = ef.parse_nextest(LOG)
        counts = ef.module_counts(blocks)
        self.assertEqual(counts["quarto-git-epochs::integration probe_real_repo"], 2)
        self.assertEqual(counts["quarto-hub::integration git_epochs"], 1)

    def test_the_repeated_failure_lines_at_the_end_add_no_blocks(self):
        blocks, _ = ef.parse_nextest(LOG)
        self.assertEqual(sorted(b["id"] for b in blocks.values()), [
            "quarto-git-epochs::integration probe_real_repo::git_path_has_drive_letter",
            "quarto-git-epochs::integration probe_real_repo::reflog_subject_parses",
            "quarto-hub::integration git_epochs::gate_holds",
        ])


if __name__ == "__main__":
    unittest.main()
