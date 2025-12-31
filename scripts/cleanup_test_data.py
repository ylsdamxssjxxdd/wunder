#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
清理测试残留数据：支持通过 API 删除用户数据，并在本地清理工作区目录。
"""

from __future__ import annotations

import argparse
import os
from pathlib import Path
from typing import Iterable, List

import httpx
import yaml


def parse_args() -> argparse.Namespace:
    """解析命令行参数，控制清理范围与执行方式。"""
    parser = argparse.ArgumentParser(description="清理测试产生的用户数据与工作区残留")
    parser.add_argument(
        "--base-url",
        default="http://127.0.0.1:8000",
        help="服务地址根路径（用于调用 /wunder/admin/users）",
    )
    parser.add_argument(
        "--prefix",
        action="append",
        default=["k6-", "eval-"],
        help="匹配 user_id 的前缀，可重复传入（默认 k6-、eval-）",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="仅输出将要删除的目标，不做实际删除",
    )
    parser.add_argument(
        "--no-api",
        action="store_true",
        help="不调用 API，仅做本地文件清理",
    )
    parser.add_argument(
        "--remove-reports",
        action="store_true",
        help="删除 data/eval_reports 下的评估报告文件",
    )
    return parser.parse_args()


def _load_workspace_root(repo_root: Path) -> Path:
    """读取配置文件中的 workspace.root，若读取失败则回退到 data/workspaces。"""
    candidates: List[Path] = []
    config_env_value = os.environ.get("WUNDER_CONFIG_PATH", "").strip()
    if config_env_value:
        candidates.append(Path(config_env_value))
    candidates.append(repo_root / "config" / "wunder.yaml")
    candidates.append(repo_root / "data" / "config" / "wunder.yaml")

    for path in candidates:
        if not path.exists():
            continue
        try:
            data = yaml.safe_load(path.read_text(encoding="utf-8"))
        except Exception:
            continue
        if not isinstance(data, dict):
            continue
        workspace = data.get("workspace", {}) if isinstance(data.get("workspace"), dict) else {}
        root_value = str(workspace.get("root", "")).strip()
        if not root_value:
            continue
        root_path = Path(root_value)
        if not root_path.is_absolute():
            root_path = (path.parent / root_path).resolve()
        return root_path

    return (repo_root / "data" / "workspaces").resolve()


def _match_prefix(text: str, prefixes: Iterable[str]) -> bool:
    """判断字符串是否匹配任一前缀。"""
    return any(text.startswith(prefix) for prefix in prefixes)


def _delete_user_via_api(
    client: httpx.Client,
    base_url: str,
    prefixes: List[str],
    dry_run: bool,
) -> tuple[int, int]:
    """调用管理接口删除用户，返回成功与失败数量。"""
    users_url = f"{base_url.rstrip('/')}/wunder/admin/users"
    try:
        response = client.get(users_url, timeout=10)
        response.raise_for_status()
    except Exception as exc:  # noqa: BLE001
        print(f"API 获取用户列表失败：{exc}")
        return 0, 0

    payload = response.json()
    users = payload.get("users", []) if isinstance(payload, dict) else []
    target_ids = [
        str(item.get("user_id", ""))
        for item in users
        if isinstance(item, dict) and _match_prefix(str(item.get("user_id", "")), prefixes)
    ]

    if not target_ids:
        print("API 未发现匹配前缀的用户")
        return 0, 0

    success = 0
    failed = 0
    for user_id in target_ids:
        delete_url = f"{base_url.rstrip('/')}/wunder/admin/users/{user_id}"
        if dry_run:
            print(f"[dry-run] 将删除用户：{user_id}")
            success += 1
            continue
        try:
            resp = client.delete(delete_url, timeout=10)
            if resp.status_code == 200:
                success += 1
            else:
                failed += 1
                print(f"删除用户失败：{user_id}（状态码 {resp.status_code}）")
        except Exception as exc:  # noqa: BLE001
            failed += 1
            print(f"删除用户失败：{user_id}（{exc}）")

    return success, failed


def _delete_local_dirs(root: Path, prefixes: List[str], dry_run: bool) -> int:
    """清理本地目录中匹配前缀的文件夹。"""
    if not root.exists() or not root.is_dir():
        return 0
    deleted = 0
    for item in root.iterdir():
        if not item.is_dir():
            continue
        if not _match_prefix(item.name, prefixes):
            continue
        if dry_run:
            print(f"[dry-run] 将删除目录：{item}")
            deleted += 1
            continue
        try:
            for sub in item.rglob("*"):
                if sub.is_file():
                    sub.unlink(missing_ok=True)
            for sub in sorted(item.rglob("*"), reverse=True):
                if sub.is_dir():
                    sub.rmdir()
            item.rmdir()
            deleted += 1
        except Exception as exc:  # noqa: BLE001
            print(f"删除目录失败：{item}（{exc}）")
    return deleted


def _cleanup_eval_reports(repo_root: Path, dry_run: bool) -> int:
    """删除评估报告文件。"""
    reports_root = repo_root / "data" / "eval_reports"
    if not reports_root.exists() or not reports_root.is_dir():
        return 0
    deleted = 0
    for file in reports_root.iterdir():
        if not file.is_file():
            continue
        if dry_run:
            print(f"[dry-run] 将删除评估报告：{file}")
            deleted += 1
            continue
        try:
            file.unlink(missing_ok=True)
            deleted += 1
        except Exception as exc:  # noqa: BLE001
            print(f"删除评估报告失败：{file}（{exc}）")
    return deleted


def main() -> int:
    """主流程：调用 API 清理用户数据，并清理本地目录。"""
    args = parse_args()
    prefixes = [p for p in args.prefix if p]
    if not prefixes:
        print("未提供有效前缀，已退出。")
        return 1

    repo_root = Path(__file__).resolve().parents[1]
    workspace_root = _load_workspace_root(repo_root)
    history_root = (repo_root / "data" / "historys").resolve()

    if args.no_api:
        print("已跳过 API 清理，仅执行本地文件删除。")
    else:
        with httpx.Client() as client:
            success, failed = _delete_user_via_api(
                client, args.base_url, prefixes, args.dry_run
            )
        if not args.dry_run:
            print(f"API 删除完成：成功 {success}，失败 {failed}")

    workspace_deleted = _delete_local_dirs(workspace_root, prefixes, args.dry_run)
    history_deleted = _delete_local_dirs(history_root, prefixes, args.dry_run)
    report_deleted = 0
    if args.remove_reports:
        report_deleted = _cleanup_eval_reports(repo_root, args.dry_run)

    if not args.dry_run:
        print(
            "本地清理完成：workspace {workspace}，historys {historys}，reports {reports}".format(
                workspace=workspace_deleted, historys=history_deleted, reports=report_deleted
            )
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
