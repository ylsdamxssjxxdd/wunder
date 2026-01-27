from __future__ import annotations

import asyncio
from typing import Any, Callable


async def run_in_thread(func: Callable[..., Any], *args, **kwargs) -> Any:
    return await asyncio.to_thread(func, *args, **kwargs)
