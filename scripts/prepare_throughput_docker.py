#!/usr/bin/env python3
"""Prepare an isolated local configuration for the throughput Docker acceptance stack."""
import json
import secrets
from pathlib import Path


def main():
    root = Path(__file__).resolve().parents[1] / "temp_dir" / "throughput-docker"
    runtime = root / "runtime" / "config"
    runtime.mkdir(parents=True, exist_ok=True)
    path = root / "runtime.json"
    credentials = root / "credentials.json"
    key = json.loads(credentials.read_text(encoding="utf-8"))["api_key"] if credentials.exists() else secrets.token_hex(24)
    models = {}
    for name, provider, model in [("virtual", "virtual_replay", ""), ("api", "vllm", "model"),
                                  ("short", "openai_compatible", "short"), ("missing", "openai_compatible", "missing"),
                                  ("disconnect", "openai_compatible", "disconnect"), ("reject", "vllm", "reject")]:
        models[name] = {"enable": True, "provider": provider, "model": model, "base_url": "http://model:18090/v1",
                        "max_context": 1100000, "max_output": 8192, "timeout_s": 900, "stream": True, "stream_include_usage": True}
    # Omit unrelated sections so their complete Rust defaults apply instead of partial structs.
    for speed in ("medium", "slow"):
        models[f"virtual_{speed}"] = {**models["virtual"], "simulation_speed": speed}
    config = {"security": {"api_key": key}, "llm": {"default": "virtual", "models": models}}
    path.write_text(json.dumps(config, indent=2), encoding="utf-8")
    credentials.write_text(json.dumps({"api_key": key}), encoding="utf-8")
    (runtime / "org_units.json").write_text("[]", encoding="utf-8")
    print("Prepared isolated runtime configuration; credentials are only stored locally.")


if __name__ == "__main__":
    main()
