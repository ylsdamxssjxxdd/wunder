from __future__ import annotations

from typing import Any, Dict, List, Optional

from pydantic import BaseModel, Field


class SandboxResources(BaseModel):
    """沙盒资源限额请求模型。"""

    cpu: float = 1.0
    memory_mb: int = 2048
    pids: int = 256


class SandboxToolRequest(BaseModel):
    """内置工具沙盒执行请求。"""

    user_id: str
    session_id: Optional[str] = ""
    tool: str
    args: Dict[str, Any] = Field(default_factory=dict)
    workspace_root: str
    allow_paths: List[str] = Field(default_factory=list)
    deny_globs: List[str] = Field(default_factory=list)
    allow_commands: List[str] = Field(default_factory=list)
    container_root: str = "/workspaces"
    image: str
    network: str = "bridge"
    readonly_rootfs: bool = True
    idle_ttl_s: int = 1800
    resources: SandboxResources = Field(default_factory=SandboxResources)


class SandboxToolResponse(BaseModel):
    """内置工具沙盒执行响应。"""

    ok: bool
    data: Dict[str, Any] = Field(default_factory=dict)
    error: str = ""
    debug_events: List[Dict[str, Any]] = Field(default_factory=list)


class SandboxReleaseRequest(BaseModel):
    """释放沙盒请求。"""

    user_id: str
    session_id: str = ""


class SandboxReleaseResponse(BaseModel):
    """释放沙盒响应。"""

    ok: bool
    message: str = ""
