"""Exit non-zero when this run's commit is no longer the branch head.

Gitea 这个版本没有工作流 `concurrency`（二进制里找不到 cancel-in-progress 的实现），服务端也不会
替新提交取消旧运行，所以由任务自己判：分支头已经不是本次触发的提交，就说明这一轮被更新的提交取代了，
调用方据此立刻结束，不再占用 runner 名额。取分支头用与 checkout.py 相同的短期令牌 + git ls-remote，
不额外假设任务令牌有读 API 的权限；拿不到结论时一律放行（宁可多跑一次，也不要把流水线卡死）。
"""

import base64
import os
import re
import shlex
import subprocess
import sys
from urllib.parse import quote


def main():
    repository = os.environ["CI_REPOSITORY"]
    commit = os.environ["CI_COMMIT"]
    reference = os.environ.get("CI_REF", "")
    token = os.environ.get("CI_TOKEN", "")
    server = os.environ.get("CI_SERVER_URL", "http://gitea:3000").rstrip("/")
    if not reference.startswith("refs/heads/") or not token:
        print("非分支推送或缺少令牌，照常构建")
        return 0
    branch = reference[len("refs/heads/"):]
    url = server + "/" + quote(repository, safe="/") + ".git"
    authorization = base64.b64encode(("x-access-token:" + token).encode()).decode()
    environment = dict(os.environ, GIT_TERMINAL_PROMPT="0",
                       GIT_CONFIG_PARAMETERS=shlex.quote("http.extraHeader=Authorization: Basic " + authorization))
    try:
        output = subprocess.check_output(["git", "ls-remote", url, "refs/heads/" + branch],
                                         env=environment).decode().strip()
    except (OSError, subprocess.CalledProcessError) as error:
        print("查询分支头失败，照常构建: " + str(error))
        return 0
    match = re.match(r"([0-9a-f]{40,64})\s", output + " ")
    if not match:
        print("分支 " + branch + " 没有返回引用，照常构建")
        return 0
    head = match.group(1)
    if head == commit:
        print("分支 " + branch + " 的头就是本次提交 " + commit[:12])
        return 0
    print("分支 " + branch + " 的头已推进到 " + head[:12] + "，本次运行（" + commit[:12] + "）已被取代")
    return 10


if __name__ == "__main__":
    sys.exit(main())
