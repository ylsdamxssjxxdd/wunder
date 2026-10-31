import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[1]
PROOFS = ROOT / "docs" / "形式化证明"
RUNNERS = (
    ("check_cron_formal", "cron/models", "CronLifecycle", 2),
    ("check_service_history_formal", "service-history-20261027", "ServiceHistory", 10),
    ("check_thread_history_replay_formal", "thread-history-replay/models", "ThreadHistoryReplay", 10),
    ("check_postgres_io_formal", "postgres-io/models", "PostgresIo", 6),
)


def load_runner(name):
    spec = importlib.util.spec_from_file_location(name, ROOT / "scripts" / (name + ".py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class FormalProofLayoutTests(unittest.TestCase):
    def test_models_have_one_central_home(self):
        models = list(PROOFS.rglob("*.tla"))
        self.assertTrue(models)
        self.assertTrue(all(PROOFS in path.parents for path in (ROOT / "docs").rglob("*.tla")))
        self.assertFalse((ROOT / "tests" / "formal").exists())

    def test_every_topic_has_an_indexed_readme(self):
        index = (PROOFS / "README.md").read_text(encoding="utf-8-sig")
        topics = [path for path in PROOFS.iterdir() if path.is_dir()]
        self.assertTrue(topics)
        for topic in topics:
            with self.subTest(topic=topic.name):
                self.assertTrue((topic / "README.md").is_file())
                self.assertIn("./" + topic.name + "/README.md", index)
        for target in re.findall(r"\]\((\./[^)#]+)(?:#[^)]*)?\)", index):
            self.assertTrue((PROOFS / target).exists(), target)

    def test_archived_json_is_readable_and_source_keys_are_portable(self):
        archives = list(PROOFS.rglob("*.json"))
        self.assertTrue(archives)
        for archive in archives:
            with self.subTest(archive=archive.relative_to(PROOFS).as_posix()):
                payload = json.loads(archive.read_text(encoding="utf-8-sig"))
                if isinstance(payload, dict):
                    for source in payload.get("uncited", {}):
                        self.assertFalse(re.match(r"^(?:[A-Za-z]:[\\/]|/|\\\\)", source))

    def test_local_preview_imports_resolve_after_migration(self):
        for preview in PROOFS.rglob("*.slint"):
            for target in re.findall(r'from\s+"([^"]+)"', preview.read_text(encoding="utf-8-sig")):
                if target.startswith("."):
                    self.assertTrue((preview.parent / target).resolve().is_file(), target)

    def test_runners_copy_canonical_models_before_starting_the_checker(self):
        for name, relative_source, model, log_count in RUNNERS:
            with self.subTest(runner=name), tempfile.TemporaryDirectory(prefix="formal-layout-") as folder:
                scratch = Path(folder) / "scratch"
                scratch.mkdir()
                output = Path(folder) / "report"
                source = PROOFS / relative_source
                module = load_runner(name)

                def run_checker(command, **kwargs):
                    self.assertEqual(command[0], "node")
                    self.assertEqual(Path(command[3]), scratch / (model + ".tla"))
                    self.assertEqual(Path(command[command.index("--out") + 1]), output)
                    self.assertNotIn(ROOT, scratch.parents)
                    self.assertNotIn(ROOT, output.parents)
                    for suffix in (".tla", ".cfg", ".map.mjs"):
                        self.assertEqual(
                            (scratch / (model + suffix)).read_bytes(),
                            (source / (model + suffix)).read_bytes(),
                        )
                    self.assertTrue((scratch / (model + "Fixed.tla")).is_file())
                    self.assertTrue((scratch / (model + "Fixed.cfg")).is_file())
                    logs = output / "tlc"
                    (logs / "fixed").mkdir(parents=True)
                    for index in range(log_count):
                        parent = logs / "fixed" if index % 2 else logs
                        (parent / ("fixture-{:02d}.out".format(index))).write_text(
                            "Model checking completed\n", encoding="utf-8"
                        )
                    (output / "summary.json").write_text(
                        json.dumps({"uncited": {}, "chartStatus": {"wrong": 0},
                                    "brokenProperties": [], "modelStates": 1}),
                        encoding="utf-8",
                    )
                    return subprocess.CompletedProcess(command, 0)

                arguments = [name, "--statechart", str(Path(folder) / "fixture-checker.mjs"),
                             "--out", str(output)]
                with mock.patch.object(sys, "argv", arguments), \
                        mock.patch.object(module.tempfile, "mkdtemp", return_value=str(scratch)), \
                        mock.patch.object(module.subprocess, "run", side_effect=run_checker) as checker, \
                        mock.patch.dict(os.environ, {"TLA2TOOLS": ""}), \
                        contextlib.redirect_stdout(io.StringIO()):
                    module.main()
                checker.assert_called_once()

    def test_runners_reject_reports_inside_the_repository(self):
        for name, _, _, _ in RUNNERS:
            with self.subTest(runner=name), tempfile.TemporaryDirectory(prefix="formal-layout-") as folder:
                module = load_runner(name)
                scratch = Path(folder) / "scratch"
                scratch.mkdir()
                arguments = [name, "--statechart", str(Path(folder) / "fixture-checker.mjs"),
                             "--out", str(PROOFS / "fixture-report")]
                with mock.patch.object(sys, "argv", arguments), \
                        mock.patch.object(module.tempfile, "mkdtemp", return_value=str(scratch)), \
                        mock.patch.object(module.subprocess, "run") as checker, \
                        contextlib.redirect_stderr(io.StringIO()):
                    with self.assertRaises(SystemExit) as error:
                        module.main()
                self.assertEqual(error.exception.code, 2)
                checker.assert_not_called()


if __name__ == "__main__":
    unittest.main()
