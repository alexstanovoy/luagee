"""End-to-end checks for measurement recording and output refusal."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import shlex
import unittest

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent


class BenchmarkTests(unittest.TestCase):
    def test_wasm_template_preserves_nested_kernel_paths(self):
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            kernels = folder / "kernels"
            (kernels / "nested").mkdir(parents=True)
            (kernels / "nested/answer.lua").write_text("print(42)\n")
            (kernels / "nested/answer.out").write_text("42\n")
            executable = folder / "interpreter"
            executable.write_text(f"#!{sys.executable}\nimport pathlib, sys\nassert pathlib.Path(sys.argv[1]).is_file()\nprint(42)\n")
            executable.chmod(0o700)
            command = shlex.join([str(executable), str(kernels / "{}.lua")])
            result = subprocess.run(
                [sys.executable, str(HERE / "bench.py"), "--runs", "1", "--warmups", "0",
                 "--kernels", str(kernels), "--candidate", f"native={executable}",
                 "--source", f"native={ROOT}", "--wasm", command],
                capture_output=True, timeout=20, check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr.decode())

    def run_comparison(self, expected, body="print(42)", stale=False, timeout=5):
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            kernels = folder / "kernels"
            kernels.mkdir()
            (kernels / "answer.lua").write_text("print(42)\n")
            (kernels / "answer.out").write_text(expected)
            executable = folder / "interpreter"
            executable.write_text(f"#!{sys.executable}\n{body}\n")
            executable.chmod(0o700)
            output = folder / "samples.json"
            if stale:
                output.write_text('{"status":"passed","stale":true}')
            result = subprocess.run(
                [sys.executable, str(HERE / "bench.py"), "--runs", "5", "--warmups", "1",
                 "--kernels", str(kernels), "--output", str(output), "--timeout", str(timeout),
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
            self.assertEqual(len(candidate["source"]["tree"]), 40)
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
        self.assertEqual(recorded["status"], "failed")

    def test_failed_run_replaces_stale_success_with_failure(self):
        result, recorded = self.run_comparison("43\n", stale=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(recorded["status"], "failed")
        self.assertNotIn("stale", recorded)

    def test_nonzero_exit_is_refused(self):
        result, recorded = self.run_comparison("42\n", "raise SystemExit(7)")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("process failed (7)", recorded["error"])

    def test_stderr_is_refused_even_with_correct_stdout(self):
        result, recorded = self.run_comparison("42\n", "import sys\nprint(42)\nprint('warning', file=sys.stderr)")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("warning", recorded["error"])

    def test_timeout_is_refused(self):
        result, recorded = self.run_comparison("42\n", "while True: pass", timeout=0.05)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("deadline exceeded", recorded["error"])

    def test_untracked_file_contents_have_distinct_provenance(self):
        import bench
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            subprocess.run(["git", "init", "-q", str(root)], check=True)
            subprocess.run(["git", "-C", str(root), "-c", "user.name=Test", "-c", "user.email=test@example.invalid",
                            "commit", "-q", "--allow-empty", "-m", "Empty"], check=True)
            script = root / "new.lua"
            script.write_text("print(1)")
            first = bench.source_provenance(root)
            script.write_text("print(2)")
            second = bench.source_provenance(root)
            self.assertEqual(first["tree"], second["tree"])
            self.assertNotEqual(first["untracked"]["new.lua"]["sha256"], second["untracked"]["new.lua"]["sha256"])


if __name__ == "__main__":
    unittest.main()
