"""End-to-end checks for measurement recording and output refusal."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent


class BenchmarkTests(unittest.TestCase):
    def run_comparison(self, expected):
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            kernels = folder / "kernels"
            kernels.mkdir()
            (kernels / "answer.lua").write_text("print(42)\n")
            (kernels / "answer.out").write_text(expected)
            executable = folder / "interpreter"
            executable.write_text(f"#!{sys.executable}\nprint(42)\n")
            executable.chmod(0o700)
            output = folder / "samples.json"
            result = subprocess.run(
                [sys.executable, str(HERE / "bench.py"), "--runs", "5", "--warmups", "1",
                 "--kernels", str(kernels), "--output", str(output),
                 "--candidate", f"before={executable}", "--source", f"before={ROOT}",
                 "--candidate", f"after={executable}", "--source", f"after={ROOT}"],
                capture_output=True, timeout=20, check=False,
            )
            recorded = json.loads(output.read_text()) if output.exists() else None
            return result, recorded

    def test_records_interleaved_samples_and_reproducible_provenance(self):
        result, recorded = self.run_comparison("42\n")
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        self.assertEqual(len(recorded["samples"]), 12)
        self.assertEqual([s["interpreter"] for s in recorded["samples"][:4]],
                         ["before", "after", "after", "before"])
        self.assertEqual(sum(s["warmup"] for s in recorded["samples"]), 2)
        for sample in recorded["samples"]:
            self.assertTrue(sample["output_checked"])
        for candidate in recorded["interpreters"].values():
            self.assertEqual(len(candidate["binary_sha256"]), 64)
            self.assertEqual(len(candidate["source"]["revision"]), 40)
        self.assertIn("rustc", recorded["compiler"])
        self.assertEqual(len(recorded["kernels"]["answer.lua"]["sha256"]), 64)
        summary = recorded["summaries"]["answer.lua"]["before"]
        self.assertEqual(summary["runs"], 5)
        self.assertLessEqual(summary["min_seconds"], summary["median_seconds"])
        self.assertLessEqual(summary["median_seconds"], summary["max_seconds"])

    def test_output_mismatch_fails_without_publishing_successful_samples(self):
        result, recorded = self.run_comparison("43\n")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b"answer.lua: before: output mismatch", result.stderr)
        self.assertIsNone(recorded)


if __name__ == "__main__":
    unittest.main()
