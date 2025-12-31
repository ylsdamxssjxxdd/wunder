from dataclasses import dataclass
from typing import Any, Dict, Optional


@dataclass
class WunderError(Exception):
    """统一异常结构，便于对外返回标准化错误信息。"""

    code: str
    message: str
    detail: Optional[Dict[str, Any]] = None

    def to_dict(self) -> Dict[str, Any]:
        """转换为 JSON 可序列化字典。"""
        payload: Dict[str, Any] = {"code": self.code, "message": self.message}
        if self.detail:
            payload["detail"] = self.detail
        return payload


class ErrorCodes:
    """预定义错误码。"""

    INVALID_REQUEST = "INVALID_REQUEST"
    TOOL_NOT_FOUND = "TOOL_NOT_FOUND"
    TOOL_EXECUTION_ERROR = "TOOL_EXECUTION_ERROR"
    LLM_UNAVAILABLE = "LLM_UNAVAILABLE"
    INTERNAL_ERROR = "INTERNAL_ERROR"
    CANCELLED = "CANCELLED"
    USER_BUSY = "USER_BUSY"
