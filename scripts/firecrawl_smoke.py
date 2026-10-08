#!/usr/bin/env python3
"""Smoke test the self-hosted Firecrawl API exposed by docker compose."""

from __future__ import annotations

import argparse
import json
import sys
import urllib.error
import urllib.request


DEFAULT_BASE_URL = "http://127.0.0.1:13002"
DEFAULT_TARGET_URL = "https://www.iana.org/domains/example"
EXPECTED_MARKER = "Example Domains"


def post_json(url: str, payload: dict[str, object], timeout: float) -> dict[str, object]:
    data = json.dumps(payload).encode("utf-8")
    request = urllib.request.Request(
        url,
        data=data,
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            body = response.read().decode("utf-8")
    except urllib.error.HTTPError as exc:
        body = exc.read().decode("utf-8", errors="replace")
        raise RuntimeError(f"Firecrawl returned HTTP {exc.code}: {body}") from exc
    except urllib.error.URLError as exc:
        raise RuntimeError(f"Firecrawl request failed: {exc.reason}") from exc

    try:
        parsed = json.loads(body)
    except json.JSONDecodeError as exc:
        raise RuntimeError(f"Firecrawl returned invalid JSON: {body[:500]}") from exc

    if not isinstance(parsed, dict):
        raise RuntimeError("Firecrawl returned a non-object JSON payload")
    return parsed


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base-url", default=DEFAULT_BASE_URL)
    parser.add_argument("--target-url", default=DEFAULT_TARGET_URL)
    parser.add_argument("--timeout", type=float, default=90.0)
    args = parser.parse_args()

    base_url = args.base_url.rstrip("/")
    payload = {
        "url": args.target_url,
        "formats": ["markdown"],
        "onlyMainContent": True,
        "timeout": int(args.timeout * 1000),
    }

    result = post_json(f"{base_url}/v2/scrape", payload, args.timeout)
    data = result.get("data")
    markdown = data.get("markdown") if isinstance(data, dict) else None
    if result.get("success") is not True or not isinstance(markdown, str):
        raise RuntimeError(f"Firecrawl scrape did not succeed: {json.dumps(result, ensure_ascii=False)[:1000]}")
    if EXPECTED_MARKER not in markdown:
        raise RuntimeError(f"Firecrawl markdown missed expected marker {EXPECTED_MARKER!r}")

    metadata = data.get("metadata") if isinstance(data, dict) else {}
    status_code = metadata.get("statusCode") if isinstance(metadata, dict) else None
    print(
        json.dumps(
            {
                "ok": True,
                "base_url": base_url,
                "target_url": args.target_url,
                "status_code": status_code,
                "markdown_chars": len(markdown),
            },
            ensure_ascii=False,
        )
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except RuntimeError as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        raise SystemExit(1)
