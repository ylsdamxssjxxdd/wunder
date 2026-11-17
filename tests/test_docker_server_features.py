import os
import re
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def read_repo_file(relative_path: str) -> str:
    return (ROOT / relative_path).read_text(encoding="utf-8")


def docker_feature_defaults(text: str) -> list[str]:
    return re.findall(r"WUNDER_SERVER_FEATURES=\$\{WUNDER_SERVER_FEATURES:-([^}]+)\}", text)


class DockerServerFeatureTests(unittest.TestCase):
    def test_docker_entry_defaults_enable_runtime_tools(self) -> None:
        entry = read_repo_file("scripts/docker-rust-entry.sh")
        self.assertIn("WUNDER_SERVER_FEATURES:-mcp,host-metrics,web-fetch", entry)
        self.assertIn("normalize_server_features", entry)

    def test_compose_defaults_enable_runtime_tools_for_server_and_sandbox(self) -> None:
        for compose_file in ("docker-compose-x86.yml", "docker-compose-arm.yml"):
            defaults = docker_feature_defaults(read_repo_file(compose_file))
            self.assertGreaterEqual(len(defaults), 2, compose_file)
            for default in defaults:
                features = {part.strip() for part in default.replace(",", " ").split()}
                self.assertIn("mcp", features, compose_file)
                self.assertIn("host-metrics", features, compose_file)
                self.assertIn("web-fetch", features, compose_file)

    def test_browser_seed_fills_new_revision_beside_stale_cache(self) -> None:
        entry = read_repo_file("scripts/docker-rust-entry.sh")
        match = re.search(r"ensure_playwright_browsers\(\) \{.*?\n\}", entry, re.DOTALL)
        self.assertIsNotNone(match)
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            seed = root / "seed"
            target = root / "target"
            old = seed / "chromium-1248"
            new = seed / "chromium_headless_shell-1254"
            aliased = seed / "chromium-1254"
            old.mkdir(parents=True)
            new.mkdir(parents=True)
            aliased.mkdir(parents=True)
            (old / "browser").write_text("seed-old", encoding="utf-8")
            (new / "browser").write_text("new", encoding="utf-8")
            (aliased / "browser").write_text("replacement", encoding="utf-8")
            (target / "chromium-1248").mkdir(parents=True)
            (target / "chromium-1248" / "browser").write_text("cached-old", encoding="utf-8")
            (target / "chromium-1254").symlink_to("chromium-1248", target_is_directory=True)
            environment = dict(os.environ)
            environment.update(
                PLAYWRIGHT_BROWSERS_PATH=str(target),
                WUNDER_PLAYWRIGHT_SEED_PATH=str(seed),
            )
            subprocess.run(
                ["bash", "-c", match.group(0) + "\nensure_playwright_browsers"],
                check=True,
                env=environment,
                capture_output=True,
                text=True,
            )
            self.assertEqual((target / "chromium-1248" / "browser").read_text(), "cached-old")
            self.assertEqual((target / "chromium-1254" / "browser").read_text(), "cached-old")
            self.assertEqual(
                (target / "chromium_headless_shell-1254" / "browser").read_text(), "new"
            )


if __name__ == "__main__":
    unittest.main()
