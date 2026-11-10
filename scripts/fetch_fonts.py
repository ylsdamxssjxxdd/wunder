#!/usr/bin/env python3
"""下载技能所需的字体到 config/fonts（云端部署、本地开发与补充包构建共用）。

字体本体不进 git：本脚本按 packaging/fonts/*.json 里固定的 URL 与 SHA-256 下载，
已存在且哈希正确的文件会直接复用。

  python scripts/fetch_fonts.py                 # 通用字体 + 视频字体
  python scripts/fetch_fonts.py --set common    # 只下载公文/办公通用字体
  python scripts/fetch_fonts.py --set video     # 只下载知识短片字体
  python scripts/fetch_fonts.py --check         # 只校验，不联网
  python scripts/fetch_fonts.py --dest DIR      # 换一个字体根目录（默认 config/fonts）
"""
import argparse
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FETCHER = ROOT / "packaging/fonts/fetch_video_fonts.py"

SETS = {
    "common": {
        "manifest": ROOT / "packaging/fonts/common-fonts.json",
        "licenses": None,
    },
    "video": {
        "manifest": ROOT / "packaging/fonts/video-fonts.json",
        "licenses": ROOT / "config/skills/知识短片/engine/fonts",
    },
}


def run(set_name, root, check):
    spec = SETS[set_name]
    cmd = [
        sys.executable,
        str(FETCHER),
        "--manifest", str(spec["manifest"]),
        "--dest", str(root / set_name),
    ]
    if spec["licenses"]:
        cmd += ["--licenses", str(spec["licenses"])]
    if check:
        cmd.append("--check")
    print(f"\n== {set_name}: {root / set_name}", flush=True)
    return subprocess.call(cmd)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--set", choices=["all", "common", "video"], default="all")
    ap.add_argument("--dest", default=str(ROOT / "config/fonts"), help="字体根目录（默认 config/fonts）")
    ap.add_argument("--check", action="store_true", help="只校验已有字体")
    args = ap.parse_args()

    if not FETCHER.is_file():
        sys.exit(f"font fetcher not found: {FETCHER}")

    root = Path(args.dest).resolve()
    names = ["common", "video"] if args.set == "all" else [args.set]
    failed = [name for name in names if run(name, root, args.check) != 0]
    if failed:
        sys.exit("font sets failed: " + ", ".join(failed))
    print("\n字体齐全：" + "、".join(names), flush=True)


if __name__ == "__main__":
    main()
