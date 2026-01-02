from typing import Any

from fastapi.responses import JSONResponse

from app.core.i18n import get_language


def json_response(payload: Any) -> JSONResponse:
    """统一返回 JSONResponse，兼容 Pydantic 模型。"""
    if hasattr(payload, "model_dump"):
        content = payload.model_dump()
    else:
        content = payload
    # 标注响应语言，方便前端调试与缓存
    return JSONResponse(content=content, headers={"Content-Language": get_language()})
