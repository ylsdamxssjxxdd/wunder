import contextlib
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[1]


def load_runner():
    spec = importlib.util.spec_from_file_location("work_process_formal", ROOT / "scripts/check_work_process_formal.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class WorkProcessFormalTests(unittest.TestCase):
    def run_fixture(self, missing=False, incomplete=False):
        module = load_runner()
        with tempfile.TemporaryDirectory(prefix="work-process-fixture-") as folder:
            scratch = Path(folder) / "scratch"
            scratch.mkdir()
            output = Path(folder) / "report"

            def checker(command, **kwargs):
                name = Path(command[3]).stem
                source = ROOT / "docs/形式化证明/聊天状态一致性验证/models"
                for suffix in (".tla", ".cfg", ".map.mjs"):
                    self.assertEqual((scratch / (name + suffix)).read_bytes(), (source / (name + suffix)).read_bytes())
                if name == "ThreadHead":
                    self.assertEqual((scratch / (name + "Fixed.tla")).read_bytes(), (source / (name + "Fixed.tla")).read_bytes())
                    self.assertEqual((scratch / (name + "Fixed.cfg")).read_bytes(), (source / (name + "Fixed.cfg")).read_bytes())
                else:
                    self.assertIn("Fixed = TRUE", (scratch / (name + "Fixed.cfg")).read_text(encoding="utf-8"))
                    self.assertEqual((scratch / (name + ".cfg")).read_text(encoding="utf-8").replace("Fixed = FALSE", "Fixed = TRUE"),
                                     (scratch / (name + "Fixed.cfg")).read_text(encoding="utf-8"))
                target = Path(command[command.index("--out") + 1])
                (target / "tlc/fixed").mkdir(parents=True)
                invariants = [line.split()[1] for line in (source / (name + ".cfg")).read_text(encoding="utf-8").splitlines() if line.startswith("INVARIANT ")]
                for fixed in (False, True):
                    for invariant in [None] + invariants:
                        if missing and fixed and invariant == invariants[-1]:
                            continue
                        path = target / "tlc" / ("fixed" if fixed else "") / (name + ("Fixed" if fixed else "") + ("-" + invariant if invariant else "") + ".out")
                        path.write_text("TLC threw an exception" if incomplete else "Model checking completed", encoding="utf-8")
                (target / "summary.json").write_text(json.dumps({"uncited": {}, "chartStatus": {"wrong": 0},
                    "brokenProperties": [], "modelStates": 1, "classes": []}), encoding="utf-8")
                return subprocess.CompletedProcess(command, 0)

            with mock.patch.object(sys, "argv", ["runner", "--statechart", "fixture.mjs", "--out", str(output)]), \
                    mock.patch.object(module.tempfile, "mkdtemp", return_value=str(scratch)), \
                    mock.patch.object(module.subprocess, "run", side_effect=checker) as run, \
                    contextlib.redirect_stdout(io.StringIO()):
                module.main()
                self.assertEqual(run.call_count, 3)

    def test_all_invariants_and_canonical_sources_are_checked(self):
        self.run_fixture()

    def test_missing_invariant_is_rejected(self):
        with self.assertRaisesRegex(SystemExit, "Missing invariant"):
            self.run_fixture(missing=True)

    def test_incomplete_exploration_is_rejected(self):
        with self.assertRaisesRegex(SystemExit, "Incomplete exploration"):
            self.run_fixture(incomplete=True)

    def test_repository_output_is_rejected(self):
        module = load_runner()
        with mock.patch.object(sys, "argv", ["runner", "--statechart", "fixture.mjs", "--out", str(ROOT / "fixture-report")]), \
                mock.patch.object(module.subprocess, "run") as checker, contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit):
                module.main()
            checker.assert_not_called()


if __name__ == "__main__":
    unittest.main()
