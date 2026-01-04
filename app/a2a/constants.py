"""A2A 协议常量与错误码定义。"""

from __future__ import annotations


# A2A 协议版本号，参考规范固定为 1.0。
A2A_PROTOCOL_VERSION = "1.0"

# JSON-RPC 2.0 版本号，所有请求与响应必须保持一致。
JSONRPC_VERSION = "2.0"

# A2A 规范内置错误码映射，便于统一构造 JSON-RPC 错误响应。
A2A_ERROR_CODES = {
    "TaskNotFoundError": -32001,
    "TaskNotCancelableError": -32002,
    "PushNotificationNotSupportedError": -32003,
    "UnsupportedOperationError": -32004,
    "ContentTypeNotSupportedError": -32005,
    "InvalidAgentResponseError": -32006,
    "ExtendedAgentCardNotConfiguredError": -32007,
    "ExtensionSupportRequiredError": -32008,
    "VersionNotSupportedError": -32009,
}

# JSON-RPC 标准错误码，和协议错误码分开管理，避免混用。
JSONRPC_ERROR_CODES = {
    "ParseError": -32700,
    "InvalidRequest": -32600,
    "MethodNotFound": -32601,
    "InvalidParams": -32602,
    "InternalError": -32603,
}
