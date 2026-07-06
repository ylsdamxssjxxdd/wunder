import re
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


if __name__ == "__main__":
    unittest.main()
