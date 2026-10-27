"""Fetch exactly the triggering commit without external checkout actions."""

import base64
import os
from pathlib import Path
import re
import shlex
import subprocess
from urllib.parse import quote


def main():
    repository = os.environ["CI_REPOSITORY"]
    commit = os.environ["CI_COMMIT"]
    reference = os.environ["CI_REF"]
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
        raise ValueError("invalid repository identity")
    if not re.fullmatch(r"[0-9a-fA-F]{40,64}", commit):
        raise ValueError("invalid commit identity")
    server = os.environ.get("CI_SERVER_URL", "http://gitea:3000").rstrip("/")
    token = os.environ["CI_TOKEN"]
    authorization = base64.b64encode(("x-access-token:" + token).encode()).decode()
    environment = dict(os.environ, GIT_TERMINAL_PROMPT="0",
                       GIT_CONFIG_PARAMETERS=shlex.quote("http.extraHeader=Authorization: Basic " + authorization))
    environment.pop("CI_TOKEN", None)
    workspace = Path(os.environ.get("GITHUB_WORKSPACE", os.getcwd()))
    workspace.mkdir(parents=True, exist_ok=True)

    def git(*arguments):
        return subprocess.check_output(["git", *arguments], cwd=workspace,
                                       env=environment).decode().strip()

    git("init", ".")
    remote = server + "/" + quote(repository, safe="/") + ".git"
    remotes = git("remote").splitlines()
    git("remote", "set-url" if "origin" in remotes else "add", "origin", remote)
    git("fetch", "--no-tags", "--depth=1", "origin", reference)
    if git("rev-parse", "FETCH_HEAD") != commit:
        git("fetch", "--no-tags", "--depth=1", "origin", commit)
    git("checkout", "--detach", commit)
    if git("rev-parse", "HEAD") != commit:
        raise RuntimeError("checked-out commit does not match the workflow trigger")
    print("Checked out triggering commit " + commit)


if __name__ == "__main__":
    main()
