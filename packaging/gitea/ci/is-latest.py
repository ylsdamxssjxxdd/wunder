"""Exit non-zero when this run's commit is no longer the branch head.

Gitea 这个版本没有工作流 `concurrency`（二进制里找不到 cancel-in-progress 的实现），服务端也不会
替新提交取消旧运行，所以由任务自己判：分支头已经不是本次触发的提交，就说明这一轮被更新的提交取代了，
调用方据此立刻结束，不再占用 runner 名额。取分支头用与 checkout.py 相同的短期令牌 + git ls-remote，
不额外假设任务令牌有读 API 的权限；拿不到结论时一律放行（宁可多跑一次，也不要把流水线卡死）。
"""

import base64
import io
import os
import re
import shlex
import subprocess
import sys
from urllib.parse import quote


def force_utf8(stream):
    """任务容器里没有 LANG/LC_ALL 时 Python 3.6 会把 stdout 当 ASCII，中文直接抛异常。"""
    try:
        return io.TextIOWrapper(stream.buffer, encoding="utf-8", errors="replace")
    except (AttributeError, ValueError):
        return stream


def main():
    sys.stdout = force_utf8(sys.stdout)
    sys.stderr = force_utf8(sys.stderr)
    repository = os.environ.get("CI_REPOSITORY", "")
    commit = os.environ.get("CI_COMMIT", "")
    reference = os.environ.get("CI_REF", "")
    token = os.environ.get("CI_TOKEN", "")
    server = os.environ.get("CI_SERVER_URL", "http://gitea:3000").rstrip("/")
    # 缺上下文时绝不能判成「已被取代」：本地复跑与调试运行常常没有 CI_COMMIT/CI_TOKEN，
    # 一旦误判就整条流水线什么都不构建，而且看起来像成功。
    if not repository or not commit or not reference.startswith("refs/heads/") or not token:
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
    try:
        code = main()
    except Exception as error:
        # 这个判据只用来让路，任何时候都不能因为自己出错而把整条构建判成「已被取代」。
        print("判定失败，照常构建: " + repr(error), file=sys.stderr)
        code = 0
    sys.exit(code)
