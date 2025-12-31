from typing import Any

from fastapi.responses import JSONResponse


def json_response(payload: Any) -> JSONResponse:
    """统一返回 JSONResponse，兼容 Pydantic 模型。"""
    if hasattr(payload, "model_dump"):
        return JSONResponse(content=payload.model_dump())
    return JSONResponse(content=payload)
