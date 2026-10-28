import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[1]


def load_runner():
    spec = importlib.util.spec_from_file_location("failure_formal", ROOT / "scripts/check_failure_recovery_formal.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class FailureRecoveryRunnerTests(unittest.TestCase):
    def test_models_are_copied_and_every_log_is_checked(self):
        module = load_runner()
        with tempfile.TemporaryDirectory(prefix="failure-formal-test-") as folder:
            scratch = Path(folder) / "scratch"
            scratch.mkdir()
            output = Path(folder) / "report"

            def run_checker(command, **kwargs):
                model = Path(command[3]).stem
                source = ROOT / "docs/形式化证明/failure-recovery/models"
                self.assertEqual((scratch / (model + ".tla")).read_bytes(), (source / (model + ".tla")).read_bytes())
                self.assertIn("Fixed = TRUE", (scratch / (model + "Fixed.cfg")).read_text(encoding="utf-8"))
                report = Path(command[command.index("--out") + 1])
                (report / "tlc/fixed").mkdir(parents=True)
                count = next(count for name, _, count in module.MODELS if name == model)
                for index in range(count):
                    parent = report / "tlc/fixed" if index % 2 else report / "tlc"
                    (parent / (str(index) + ".out")).write_text("Model checking completed\n", encoding="utf-8")
                (report / "summary.json").write_text(json.dumps({
                    "uncited": {}, "chartStatus": {"wrong": 0}, "brokenProperties": [], "modelStates": 1,
                }), encoding="utf-8")
                return subprocess.CompletedProcess(command, 0)

            with mock.patch.object(sys, "argv", ["runner", "--statechart", str(Path(folder) / "fixture.mjs"), "--out", str(output)]), \
                    mock.patch.object(module.tempfile, "mkdtemp", return_value=str(scratch)), \
                    mock.patch.object(module.subprocess, "run", side_effect=run_checker) as checker, \
                    mock.patch.dict(module.os.environ, {"TLA2TOOLS": ""}):
                module.main()
                self.assertEqual(checker.call_count, len(module.MODELS))

    def test_repository_output_is_rejected(self):
        module = load_runner()
        with tempfile.TemporaryDirectory(prefix="failure-formal-test-") as folder:
            with mock.patch.object(sys, "argv", ["runner", "--statechart", "fixture.mjs", "--out", str(ROOT / "proof-report")]), \
                    mock.patch.object(module.tempfile, "mkdtemp", return_value=folder), \
                    mock.patch.object(module.subprocess, "run") as checker:
                with self.assertRaises(SystemExit) as raised:
                    module.main()
                self.assertEqual(raised.exception.code, 2)
                checker.assert_not_called()


if __name__ == "__main__":
    unittest.main()
