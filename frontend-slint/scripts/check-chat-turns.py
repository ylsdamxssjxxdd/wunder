"""Fixed-turn regression gate; no backend service or model is started."""
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "temp_dir" / "desktop-turn-review"

def run(*args):
    subprocess.run(args, cwd=ROOT, check=True)

def main():
    OUT.mkdir(parents=True, exist_ok=True)
    run("slint-viewer", "--check", "frontend-slint/ui/main.slint")
    run("cargo", "test", "-p", "wunder-desktop", "--lib", "native::", "--no-default-features", "--features", "sqlite-storage")
    run("cargo", "test", "--manifest-path", "frontend-slint/Cargo.toml", "turn_tests", "--bin", "wunder-frontend-slint")
    empty = {key: "" for key in ["text", "time", "workflow-detail", "state", "stats-status", "stats-duration", "stats-speed", "stats-context", "stats-quota", "stats-tools", "stats-credits", "avatar-glyph"]}
    empty.update(mine=False, workflow=False, blocks=[], **{"avatar-tone": 0})
    turns = []
    for index, status in enumerate(["已停止", "任务完成", "正在排队 · 前方 2 名"]):
        turns.append({"root-id": f"fixture-root-{index}",
            "user": {**empty, "mine": True, "text": ["检查测试内容。", "停止后继续检查。", "/compact"][index]},
            "assistant": {**empty, "text": ["保留的部分回复。", "已完成测试检查。", "等待调度。"][index],
                "workflow": True, "workflow-detail": "读取 fixture.txt\n保留工具结果及压缩摘要", "stats-status": status,
                "stats-duration": "1.2s", "stats-speed": "24.0/s", "avatar-glyph": "✦", "avatar-tone": 1}})
    fixture = OUT / "fixture.json"
    fixture.write_text(json.dumps({"turns": turns, "heading": "测试会话", "selected-agent-name": "测试智能体", "right-open": False}, ensure_ascii=False), encoding="utf-8")
    run("slint-viewer", "--screenshot", str(OUT / "conversation.png"), "--load-data", str(fixture), "--size", "1100x1500", "frontend-slint/ui/main.slint")
    (OUT / "analysis.json").write_text(json.dumps({"mode": "native-projection-and-viewer", "passed": True,
        "coverage": ["fixed-pair", "model-rounds", "continuation-history", "stop-late-output", "queue", "compaction", "background-durable-stream", "snapshot-resume", "terminal-drain"],
        "limitations": ["Viewer data is synthetic; durable follow uses isolated SQLite tests without a real model provider."]}, indent=2), encoding="utf-8")

if __name__ == "__main__":
    main()
