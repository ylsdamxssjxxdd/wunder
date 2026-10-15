#!/usr/bin/env python3
"""Run one packaged skill entry point with the engine's JSON stdin contract.

The runtime invokes this small standard-library wrapper so both local clients
use the same execution boundary. The entry point receives the original stdin
and ``sys.argv[0]`` points at the skill script; its stdout is returned as-is.
"""

from __future__ import annotations

import runpy
import sys
from pathlib import Path


def main() -> int:
    if len(sys.argv) != 2:
        print("skill runner requires one entrypoint path", file=sys.stderr)
        return 2
    entrypoint = Path(sys.argv[1]).expanduser().resolve()
    if not entrypoint.is_file():
        print("skill entrypoint not found", file=sys.stderr)
        return 2
    sys.argv = [str(entrypoint)]
    runpy.run_path(str(entrypoint), run_name="__main__")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
