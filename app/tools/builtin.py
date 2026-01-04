import fnmatch
import json
import os
import shlex
import subprocess
import sys
import time
import uuid
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple
from urllib.parse import urlparse, urlunparse

import httpx

from app.core.i18n import t
from app.tools.types import ToolContext, ToolResult

A2A_DEFAULT_TIMEOUT_S = 120  # A2A å§”æ´¾è°ƒç”¨é»˜è®¤è¶…æ—¶ï¼ˆç§’ï¼‰


MAX_READ_BYTES = 1024 * 1024  # 单次读取最大字节数，避免读入过大文件
MAX_READ_LINES = 200  # 单次最多返回行数，避免输出过长
MAX_READ_FILES = 5  # 单次最多读取文件数量
MAX_RANGE_SPAN = 400  # 单个范围最大行数跨度
PTC_DIR_NAME = "ptc_temp"  # 程序化工具脚本临时目录
PTC_TIMEOUT_S = 60  # PTC 脚本默认超时时间（秒）


def _resolve_path_with_base(context: ToolContext, relative_path: str) -> Tuple[Path, Path]:
    """在工作区或白名单目录内解析路径，并返回目标路径与所属根目录。"""
    root = context.workspace.root
    rel = Path(relative_path)
    deny_globs = context.config.get("deny_globs", [])
    allow_paths = context.config.get("allow_paths", [])

    # 组装允许访问的根目录列表：工作区根目录 + 配置白名单
    allowed_roots: List[Path] = [root]
    seen_roots = {str(root)}
    for raw_path in allow_paths:
        path_value = str(raw_path).strip()
        if not path_value:
            continue
        try:
            candidate = Path(path_value).expanduser().resolve()
        except Exception:
            # 忽略无法解析的路径，避免影响其他合法路径
            continue
        key = str(candidate)
        if key in seen_roots:
            continue
        allowed_roots.append(candidate)
        seen_roots.add(key)

    def _match_allowed_root(target: Path) -> Optional[Path]:
        """匹配目标路径所属的允许根目录，未命中则返回 None。"""
        for base in allowed_roots:
            if target == base or base in target.parents:
                return base
        return None

    def _check_deny_globs(target: Path, base: Path) -> None:
        """在对应根目录范围内检查拒绝访问的路径规则。"""
        relative = target.relative_to(base)
        for pattern in deny_globs:
            if fnmatch.fnmatch(str(relative), pattern):
                raise ValueError(t("tool.fs.path_forbidden"))

    if rel.is_absolute():
        target = rel.resolve()
        base = _match_allowed_root(target)
        if not base:
            raise ValueError(t("tool.fs.absolute_forbidden"))
        _check_deny_globs(target, base)
        return target, base

    target = (root / rel).resolve()
    base = _match_allowed_root(target)
    if not base:
        raise ValueError(t("tool.fs.path_out_of_bounds"))
    _check_deny_globs(target, base)
    return target, base


def _resolve_path(context: ToolContext, relative_path: str) -> Path:
    """在工作区或白名单目录内解析路径，确保不越界。"""
    target, _ = _resolve_path_with_base(context, relative_path)
    return target


def _normalize_range(start: int, end: int) -> Tuple[int, int]:
    """标准化行范围，避免越界或跨度过大。"""
    start = max(1, start)
    end = max(start, end)
    if end - start + 1 > MAX_RANGE_SPAN:
        end = start + MAX_RANGE_SPAN - 1
    return start, end


def _parse_file_specs(args: Dict[str, Any]) -> Tuple[List[Dict[str, Any]], str]:
    """解析 read_file 的多文件与行号范围参数。"""
    specs: List[Dict[str, Any]] = []

    def _append_spec(path: str, ranges: List[Tuple[int, int]]) -> None:
        if not path:
            return
        if not ranges:
            ranges = [(1, MAX_READ_LINES)]
        specs.append({"path": path, "ranges": ranges})

    files = args.get("files")
    if isinstance(files, list):
        for item in files[:MAX_READ_FILES]:
            if not isinstance(item, dict):
                continue
            path = str(item.get("path", "")).strip()
            ranges: List[Tuple[int, int]] = []
            for range_item in item.get("line_ranges", []) or []:
                if isinstance(range_item, list) and len(range_item) >= 2:
                    start, end = _normalize_range(int(range_item[0]), int(range_item[1]))
                    ranges.append((start, end))
            start_line = item.get("start_line")
            end_line = item.get("end_line")
            if start_line:
                start, end = _normalize_range(int(start_line), int(end_line or start_line))
                ranges.append((start, end))
            _append_spec(path, ranges)

    if not specs:
        return [], t("tool.read.no_path")
    return specs, ""


def read_file(context: ToolContext, args: Dict[str, Any]) -> ToolResult:
    """读取工作区内的文本文件，按行号范围返回。"""
    specs, error = _parse_file_specs(args)
    if error:
        return ToolResult(ok=False, data={}, error=error)

    outputs: List[str] = []
    for spec in specs:
        try:
            path = _resolve_path(context, spec["path"])
        except ValueError as exc:
            outputs.append(f">>> {spec['path']}\n{exc}")
            continue
        if not path.exists():
            outputs.append(f">>> {spec['path']}\n{t('tool.read.not_found')}")
            continue
        if path.stat().st_size > MAX_READ_BYTES:
            outputs.append(f">>> {spec['path']}\n{t('tool.read.too_large')}")
            continue

        content = path.read_text(encoding="utf-8", errors="ignore")
        lines = content.splitlines()
        file_output: List[str] = []
        for start, end in spec["ranges"]:
            if not lines:
                file_output.append(t("tool.read.empty_file"))
                continue
            if start > len(lines):
                file_output.append(
                    t(
                        "tool.read.range_out_of_file",
                        start=start,
                        end=end,
                        total=len(lines),
                    )
                )
                continue
            last = min(end, len(lines))
            slice_lines = [
                f"{idx + 1}: {lines[idx]}" for idx in range(start - 1, last)
            ]
            file_output.append("\n".join(slice_lines))
        outputs.append(f">>> {spec['path']}\n" + "\n---\n".join(file_output))

    result = "\n\n".join(outputs) if outputs else t("tool.read.empty_result")
    return ToolResult(ok=True, data={"content": result})


def write_file(context: ToolContext, args: Dict[str, Any]) -> ToolResult:
    """在工作区内写入文本文件。"""
    path = _resolve_path(context, args.get("path", ""))
    content = args.get("content", "")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")
    return ToolResult(ok=True, data={"path": str(path), "bytes": len(content.encode("utf-8"))})


def list_files(context: ToolContext, args: Dict[str, Any]) -> ToolResult:
    """列出工作区内的文件与目录。"""
    base_path, _ = _resolve_path_with_base(context, args.get("path", "."))
    max_depth = int(args.get("max_depth", 2))
    # 始终输出相对路径，避免绝对路径占用过多上下文。
    display_root = base_path

    if not base_path.exists():
        return ToolResult(ok=False, data={}, error=t("tool.list.path_not_found"))

    results: List[str] = []
    base_depth = len(base_path.parts)
    for root, dirs, files in os.walk(base_path):
        depth = len(Path(root).parts) - base_depth
        if depth > max_depth:
            dirs[:] = []
            continue
        for name in dirs:
            rel = Path(root) / name
            display_path = str(rel.relative_to(display_root))
            results.append(display_path + "/")
        for name in files:
            rel = Path(root) / name
            display_path = str(rel.relative_to(display_root))
            results.append(display_path)

    return ToolResult(ok=True, data={"items": results})


def search_content(context: ToolContext, args: Dict[str, Any]) -> ToolResult:
    """在工作区内进行文本搜索，按行返回结果。"""
    query = str(args.get("query", "")).strip()
    base_path, base_root = _resolve_path_with_base(context, args.get("path", "."))
    file_pattern = str(args.get("file_pattern", "")).strip()
    # 非工作区根目录时返回绝对路径，避免相对路径歧义
    show_absolute = base_root != context.workspace.root

    if not query:
        return ToolResult(ok=False, data={}, error=t("tool.search.empty"))
    if not base_path.exists():
        return ToolResult(ok=False, data={}, error=t("tool.search.path_not_found"))

    matches: List[str] = []
    lower_query = query.lower()
    for file_path in base_path.rglob("*"):
        if file_path.is_dir():
            continue
        relative_path = file_path.relative_to(base_root)
        if file_pattern and not fnmatch.fnmatch(str(relative_path), file_pattern):
            continue
        try:
            content = file_path.read_text(encoding="utf-8", errors="ignore")
        except Exception:
            continue
        for idx, line in enumerate(content.splitlines(), start=1):
            if lower_query in line.lower():
                display_path = str(file_path) if show_absolute else str(relative_path)
                matches.append(f"{display_path}:{idx}:{line.strip()}")

    return ToolResult(ok=True, data={"matches": matches})


def replace_in_file(context: ToolContext, args: Dict[str, Any]) -> ToolResult:
    """在文件中替换指定字符串。"""
    path = _resolve_path(context, args.get("path", ""))
    old = args.get("old_string", "")
    new = args.get("new_string", "")
    expected = int(args.get("expected_replacements", 1))

    if not path.exists():
        return ToolResult(ok=False, data={}, error=t("tool.replace.file_not_found"))
    content = path.read_text(encoding="utf-8", errors="ignore")
    occurrences = content.count(old)
    if occurrences == 0:
        return ToolResult(ok=False, data={}, error=t("tool.replace.not_found"))
    if expected and occurrences != expected:
        return ToolResult(
            ok=False,
            data={"occurrences": occurrences},
            error=t("tool.replace.count_mismatch"),
        )
    replaced = content.replace(old, new)
    path.write_text(replaced, encoding="utf-8")
    return ToolResult(ok=True, data={"path": str(path), "replaced": occurrences})


def edit_in_file(context: ToolContext, args: Dict[str, Any]) -> ToolResult:
    """按行号进行结构化编辑。"""
    path = _resolve_path(context, args.get("path", ""))
    edits = args.get("edits", [])
    ensure_newline = bool(args.get("ensure_newline_at_eof", True))

    if not path.exists():
        return ToolResult(ok=False, data={}, error=t("tool.edit.file_not_found"))
    lines = path.read_text(encoding="utf-8", errors="ignore").splitlines()

    def _insert_lines(index: int, content: str) -> None:
        insert_lines = content.splitlines()
        for offset, line in enumerate(insert_lines):
            lines.insert(index + offset, line)

    # 按起始行倒序应用，避免前面操作影响行号
    sorted_edits = sorted(edits, key=lambda e: int(e.get("start_line", 1)), reverse=True)
    for edit in sorted_edits:
        action = edit.get("action")
        start_line = int(edit.get("start_line", 1))
        end_line = int(edit.get("end_line", start_line))
        new_content = edit.get("new_content", "")

        if start_line < 1:
            return ToolResult(ok=False, data={}, error=t("tool.edit.invalid_start"))
        if start_line > len(lines) + 1:
            return ToolResult(ok=False, data={}, error=t("tool.edit.out_of_range"))

        start_index = start_line - 1
        end_index = max(start_index, end_line - 1)

        if action == "replace":
            if end_index >= len(lines):
                return ToolResult(
                    ok=False, data={}, error=t("tool.edit.replace_out_of_range")
                )
            del lines[start_index : end_index + 1]
            _insert_lines(start_index, new_content)
        elif action == "delete":
            if end_index >= len(lines):
                return ToolResult(
                    ok=False, data={}, error=t("tool.edit.delete_out_of_range")
                )
            del lines[start_index : end_index + 1]
        elif action == "insert_before":
            _insert_lines(start_index, new_content)
        elif action == "insert_after":
            _insert_lines(end_index + 1, new_content)
        else:
            return ToolResult(
                ok=False, data={}, error=t("tool.edit.action_unsupported")
            )

    output = "\n".join(lines)
    if ensure_newline:
        output += "\n"
    path.write_text(output, encoding="utf-8")
    return ToolResult(ok=True, data={"path": str(path), "lines": len(lines)})


def execute_command(context: ToolContext, args: Dict[str, Any]) -> ToolResult:
    """在工作区内执行允许的系统命令。"""
    content = str(args.get("content", "")).strip()
    timeout_s = int(args.get("timeout_s", 30))
    workdir = str(args.get("workdir", "")).strip()
    raw_allow_commands = context.config.get("allow_commands", [])
    allow_commands = {
        str(item).strip().lower()
        for item in raw_allow_commands
        if str(item).strip()
    }
    # allow_commands 包含 "*" 时视为放开所有命令限制
    allow_all = "*" in allow_commands
    raw_shell = args.get("shell")
    # shell 默认随 allow_commands 为 * 自动开启，显式传参时按布尔解析
    if raw_shell is None:
        use_shell = allow_all
    elif isinstance(raw_shell, str):
        use_shell = raw_shell.strip().lower() in {"1", "true", "yes", "y"}
    else:
        use_shell = bool(raw_shell)

    if not content:
        return ToolResult(ok=False, data={}, error=t("tool.exec.command_required"))

    # 允许指定工作目录，但仅限工作区或白名单目录
    try:
        cwd = _resolve_path(context, workdir or ".")
    except ValueError as exc:
        return ToolResult(ok=False, data={}, error=str(exc))
    if not cwd.exists():
        return ToolResult(ok=False, data={}, error=t("tool.exec.workdir_not_found"))
    if not cwd.is_dir():
        return ToolResult(ok=False, data={}, error=t("tool.exec.workdir_not_dir"))

    def _strip_wrapped_quotes(token: str) -> str:
        """去除命令参数首尾成对的引号，避免传入被当作字面量。"""
        if len(token) >= 2 and token[0] == token[-1] and token[0] in {"\"", "'"}:
            return token[1:-1]
        return token

    command_results: List[Dict[str, Any]] = []
    for raw_line in content.splitlines():
        command = raw_line.strip()
        if not command:
            continue
        if use_shell:
            if not allow_all:
                return ToolResult(
                    ok=False,
                    data={},
                    error=t("tool.exec.shell_not_allowed"),
                )
            try:
                completed = subprocess.run(
                    command,
                    cwd=cwd,
                    capture_output=True,
                    text=True,
                    timeout=timeout_s,
                    check=False,
                    shell=True,
                )
            except Exception as exc:
                return ToolResult(
                    ok=False,
                    data={},
                    error=t("tool.exec.command_failed", detail=str(exc)),
                )
        else:
            tokens = shlex.split(command, posix=False)
            tokens = [_strip_wrapped_quotes(token) for token in tokens]
            if not tokens:
                return ToolResult(ok=False, data={}, error=t("tool.exec.parse_failed"))
            if not allow_all and tokens[0].lower() not in allow_commands:
                return ToolResult(ok=False, data={}, error=t("tool.exec.not_allowed"))

            try:
                completed = subprocess.run(
                    tokens,
                    cwd=cwd,
                    capture_output=True,
                    text=True,
                    timeout=timeout_s,
                    check=False,
                )
            except Exception as exc:
                return ToolResult(
                    ok=False,
                    data={},
                    error=t("tool.exec.command_failed", detail=str(exc)),
                )
        command_results.append(
            {
                "command": command,
                "returncode": completed.returncode,
                "stdout": completed.stdout,
                "stderr": completed.stderr,
            }
        )
        if completed.returncode != 0:
            return ToolResult(
                ok=False,
                data={
                    "results": command_results,
                },
                error=t("tool.exec.failed"),
            )

    return ToolResult(ok=True, data={"results": command_results})


def ptc(context: ToolContext, args: Dict[str, Any]) -> ToolResult:
    """程序化工具调用：保存并执行 Python 脚本。"""
    filename = str(args.get("filename", "")).strip()
    workdir = str(args.get("workdir", ".")).strip() or "."
    content = args.get("content", "")

    if not filename:
        return ToolResult(ok=False, data={}, error=t("tool.ptc.filename_required"))
    if not isinstance(content, str) or not content.strip():
        return ToolResult(ok=False, data={}, error=t("tool.ptc.content_required"))

    # 仅允许文件名，避免路径穿透
    path = Path(filename)
    if path.name != filename:
        return ToolResult(ok=False, data={}, error=t("tool.ptc.filename_invalid"))
    if not path.suffix:
        path = path.with_suffix(".py")
    if path.suffix.lower() != ".py":
        return ToolResult(ok=False, data={}, error=t("tool.ptc.ext_invalid"))

    # 解析工作目录与 PTC 临时目录
    try:
        workdir_path = _resolve_path(context, workdir)
        ptc_root = _resolve_path(context, PTC_DIR_NAME)
    except ValueError as exc:
        return ToolResult(ok=False, data={}, error=str(exc))

    workdir_path.mkdir(parents=True, exist_ok=True)
    ptc_root.mkdir(parents=True, exist_ok=True)
    script_path = ptc_root / path.name
    script_path.write_text(content, encoding="utf-8")

    # 执行脚本并采集输出
    env = os.environ.copy()
    env["PYTHONIOENCODING"] = "utf-8"
    try:
        completed = subprocess.run(
            [sys.executable, str(script_path)],
            cwd=workdir_path,
            env=env,
            capture_output=True,
            text=True,
            timeout=PTC_TIMEOUT_S,
            check=False,
        )
    except Exception as exc:
        return ToolResult(
            ok=False, data={}, error=t("tool.ptc.exec_error", detail=str(exc))
        )

    data = {
        "path": str(script_path),
        "workdir": str(workdir_path),
        "returncode": completed.returncode,
        "stdout": completed.stdout,
        "stderr": completed.stderr,
    }
    if completed.returncode != 0:
        return ToolResult(ok=False, data=data, error=t("tool.ptc.exec_failed"))
    return ToolResult(ok=True, data=data)


def _normalize_a2a_endpoint(raw_endpoint: Any) -> str:
    """规范化 A2A 端点地址，避免漏写协议或路径。"""
    endpoint = str(raw_endpoint or "").strip()
    if not endpoint:
        return ""
    if not endpoint.startswith(("http://", "https://")):
        endpoint = f"http://{endpoint}"
    parsed = urlparse(endpoint)
    path = parsed.path or ""
    # 当路径为空或仅为 "/" 时自动补齐 /a2a，降低配置成本
    if not path or path == "/":
        path = "/a2a"
    path = path.rstrip("/") or "/a2a"
    return urlunparse(parsed._replace(path=path))


def _normalize_a2a_tool_names(raw: Any) -> Optional[List[str]]:
    """统一清洗 A2A toolNames 输入。"""
    if isinstance(raw, str):
        parts = [item.strip() for item in raw.split(",")]
        cleaned = [item for item in parts if item]
        return cleaned or None
    if isinstance(raw, list):
        cleaned = [str(item).strip() for item in raw if str(item).strip()]
        return cleaned or None
    return None


def _normalize_a2a_message(raw_message: Any) -> Optional[Dict[str, Any]]:
    """将输入统一转换为 A2A Message 结构。"""
    if raw_message is None:
        return None
    if isinstance(raw_message, dict):
        message = dict(raw_message)
    elif isinstance(raw_message, str):
        message = {"role": "user", "parts": [{"text": raw_message}]}
    else:
        return None

    # 兼容 content/text 直传的简写形式
    if "parts" not in message:
        text_value = message.pop("text", None) or message.pop("content", None)
        if text_value:
            message["parts"] = [{"text": str(text_value)}]

    parts = message.get("parts")
    if isinstance(parts, list):
        normalized_parts: List[Dict[str, Any]] = []
        for part in parts:
            if isinstance(part, dict):
                normalized_parts.append(part)
            elif isinstance(part, str):
                normalized_parts.append({"text": part})
        message["parts"] = normalized_parts
    else:
        return None

    if not message.get("role"):
        message["role"] = "user"
    if not message.get("parts"):
        return None
    return message


def _extract_a2a_text_parts(parts: Any) -> List[str]:
    """从 A2A Part 列表中提取文本内容。"""
    if not isinstance(parts, list):
        return []
    texts: List[str] = []
    for part in parts:
        if not isinstance(part, dict):
            continue
        if "text" in part:
            text_value = str(part.get("text") or "")
            if text_value:
                texts.append(text_value)
    return texts


def _iter_a2a_sse_payloads(response: httpx.Response):
    """迭代解析 A2A SSE 返回的 data 行。"""
    for line in response.iter_lines():
        if not line:
            continue
        if isinstance(line, bytes):
            line = line.decode("utf-8", errors="ignore")
        cleaned = line.strip()
        if not cleaned.startswith("data:"):
            continue
        raw = cleaned[len("data:") :].strip()
        if not raw:
            continue
        try:
            payload = json.loads(raw)
        except json.JSONDecodeError:
            continue
        if isinstance(payload, dict):
            yield payload


def a2a_delegate(context: ToolContext, args: Dict[str, Any]) -> ToolResult:
    """通过 A2A JSON-RPC 委派任务给外部智能体并返回结果。"""
    start_ts = time.perf_counter()
    endpoint = _normalize_a2a_endpoint(args.get("endpoint") or args.get("url"))
    if not endpoint:
        return ToolResult(ok=False, data={}, error=t("tool.a2a.endpoint_required"))

    method = str(args.get("method") or "").strip() or "SendMessage"
    if method not in {"SendMessage", "SendStreamingMessage"}:
        return ToolResult(
            ok=False,
            data={"method": method},
            error=t("tool.a2a.method_unsupported", method=method),
        )

    # 统一读取会话标识，默认跟随当前会话，便于多轮对话衔接
    session_id = str(
        args.get("session_id")
        or args.get("context_id")
        or args.get("task_id")
        or context.workspace.session_id
        or ""
    ).strip()
    request_id = str(
        args.get("jsonrpc_id") or args.get("request_id") or session_id or uuid.uuid4().hex
    ).strip()

    raw_message = args.get("message")
    if raw_message is None:
        raw_message = args.get("content")
    if raw_message is None:
        raw_message = args.get("text")
    if raw_message is None:
        raw_message = args.get("request")
    message = _normalize_a2a_message(raw_message)
    if message is None:
        return ToolResult(ok=False, data={}, error=t("tool.a2a.message_required"))

    if session_id:
        message.setdefault("taskId", session_id)
        message.setdefault("contextId", session_id)

    # 组装委派链路，避免循环调用
    caller = str(args.get("caller") or args.get("caller_id") or "Wunder").strip() or "Wunder"
    target = str(args.get("target") or endpoint).strip() or endpoint
    chain = _normalize_a2a_tool_names(
        args.get("delegation_chain") or args.get("delegationChain")
    ) or []
    metadata = message.get("metadata") if isinstance(message.get("metadata"), dict) else {}
    if not chain:
        delegation_meta = metadata.get("delegation") if isinstance(metadata, dict) else {}
        if isinstance(delegation_meta, dict):
            chain = _normalize_a2a_tool_names(delegation_meta.get("chain")) or []
    if caller and (not chain or (chain[0] != caller and caller not in chain)):
        chain.insert(0, caller)

    allow_self = bool(args.get("allow_self", False))
    max_depth = int(args.get("max_depth") or 0)
    local_endpoint = ""
    local_port = context.config.get("server_port")
    if local_port:
        local_endpoint = _normalize_a2a_endpoint(f"http://127.0.0.1:{local_port}/a2a")
    if local_endpoint and endpoint == local_endpoint and not allow_self:
        return ToolResult(ok=False, data={}, error=t("tool.a2a.loop_detected"))

    if target in chain and not allow_self:
        return ToolResult(ok=False, data={}, error=t("tool.a2a.loop_detected"))
    if target not in chain:
        chain.append(target)
    if max_depth > 0 and len(chain) > max_depth:
        return ToolResult(
            ok=False,
            data={"max_depth": max_depth, "delegation_chain": chain},
            error=t("tool.a2a.depth_exceeded", max_depth=max_depth),
        )

    # 将委派链路写入 metadata，供对端或后续调用沿用
    if not isinstance(metadata, dict):
        metadata = {}
    metadata["delegation"] = {
        "caller": caller,
        "target": target,
        "chain": chain,
        "depth": len(chain),
        "parentTaskId": context.workspace.session_id,
    }
    metadata.setdefault("delegationChain", chain)
    message["metadata"] = metadata

    params: Dict[str, Any] = {"message": message}
    user_id = str(args.get("user_id") or args.get("userId") or context.workspace.user_id or "").strip()
    if user_id:
        params["userId"] = user_id

    tool_names = _normalize_a2a_tool_names(args.get("tool_names") or args.get("toolNames"))
    if tool_names:
        params["toolNames"] = tool_names
    model_name = str(args.get("model_name") or args.get("modelName") or "").strip()
    if model_name:
        params["modelName"] = model_name

    configuration = args.get("configuration")
    config_payload = dict(configuration) if isinstance(configuration, dict) else {}
    blocking = args.get("blocking")
    if blocking is None:
        blocking = method == "SendMessage"
    config_payload["blocking"] = bool(blocking)
    history_length = args.get("history_length") or args.get("historyLength")
    if history_length is not None:
        try:
            config_payload["historyLength"] = int(history_length)
        except (TypeError, ValueError):
            pass
    if method == "SendMessage" and config_payload:
        params["configuration"] = config_payload

    headers: Dict[str, str] = {}
    raw_headers = args.get("headers")
    if isinstance(raw_headers, dict):
        for key, value in raw_headers.items():
            header_key = str(key or "").strip()
            header_value = str(value or "").strip()
            if header_key and header_value:
                headers[header_key] = header_value

    api_key = str(args.get("api_key") or args.get("apiKey") or context.config.get("api_key") or "").strip()
    if api_key and not any(k.lower() in {"x-api-key", "authorization"} for k in headers):
        headers["X-API-Key"] = api_key
    authorization = str(args.get("authorization") or args.get("auth") or "").strip()
    if authorization and not any(k.lower() == "authorization" for k in headers):
        if authorization.lower().startswith("bearer "):
            headers["Authorization"] = authorization
        else:
            headers["Authorization"] = f"Bearer {authorization}"
    if method == "SendStreamingMessage":
        headers.setdefault("Accept", "text/event-stream")

    timeout_s = int(args.get("timeout_s") or A2A_DEFAULT_TIMEOUT_S)
    payload = {"jsonrpc": "2.0", "id": request_id, "method": method, "params": params}

    if context.emit_event:
        context.emit_event(
            "a2a_request",
            {
                "endpoint": endpoint,
                "method": method,
                "request_id": request_id,
                "session_id": session_id,
                "blocking": bool(blocking),
            },
        )

    include_raw = bool(args.get("include_raw", False))
    include_events = bool(args.get("include_events", False))
    max_events = int(args.get("max_events") or 200)
    events: List[Dict[str, Any]] = []
    raw_response: Any = None
    final_text_parts: List[str] = []
    task_id = ""
    context_id = ""
    state = ""
    token_usage: Optional[Dict[str, Any]] = None

    try:
        with httpx.Client(timeout=timeout_s) as client:
            if method == "SendStreamingMessage":
                with client.stream("POST", endpoint, json=payload, headers=headers) as resp:
                    if resp.status_code >= 400:
                        return ToolResult(
                            ok=False,
                            data={"status": resp.status_code},
                            error=t("tool.a2a.http_error", status=resp.status_code),
                        )
                    for item in _iter_a2a_sse_payloads(resp):
                        if (include_events or include_raw) and len(events) < max_events:
                            events.append(item)
                        if "task" in item and isinstance(item.get("task"), dict):
                            task = item.get("task", {})
                            task_id = str(task.get("id") or "").strip()
                            context_id = str(task.get("contextId") or "").strip()
                        status_update = item.get("statusUpdate")
                        if isinstance(status_update, dict):
                            status = status_update.get("status") if isinstance(status_update.get("status"), dict) else {}
                            state = str(status.get("state") or state)
                            meta = status_update.get("metadata") if isinstance(status_update.get("metadata"), dict) else {}
                            if isinstance(meta.get("tokenUsage"), dict):
                                token_usage = meta.get("tokenUsage")
                            if context.emit_event:
                                context.emit_event(
                                    "a2a_status",
                                    {
                                        "task_id": task_id,
                                        "context_id": context_id,
                                        "state": state,
                                        "final": bool(status_update.get("final")),
                                    },
                                )
                            if status_update.get("final"):
                                break
                        artifact_update = item.get("artifactUpdate")
                        if isinstance(artifact_update, dict):
                            artifact = artifact_update.get("artifact") if isinstance(artifact_update.get("artifact"), dict) else {}
                            parts = artifact.get("parts")
                            final_text_parts.extend(_extract_a2a_text_parts(parts))
                            if context.emit_event and artifact.get("name"):
                                context.emit_event(
                                    "a2a_artifact",
                                    {
                                        "task_id": task_id,
                                        "context_id": context_id,
                                        "name": artifact.get("name"),
                                    },
                                )
            else:
                resp = client.post(endpoint, json=payload, headers=headers)
                if resp.status_code >= 400:
                    return ToolResult(
                        ok=False,
                        data={"status": resp.status_code},
                        error=t("tool.a2a.http_error", status=resp.status_code),
                    )
                try:
                    body = resp.json()
                except ValueError:
                    return ToolResult(ok=False, data={}, error=t("tool.a2a.response_invalid"))
                if include_events and len(events) < max_events:
                    events.append(body if isinstance(body, dict) else {"result": body})
                if include_raw:
                    raw_response = body
                if isinstance(body, dict) and body.get("error"):
                    error_payload = body.get("error") or {}
                    message = str(error_payload.get("message") or t("tool.a2a.response_invalid"))
                    return ToolResult(ok=False, data={"error": error_payload}, error=message)
                result = body.get("result") if isinstance(body, dict) else None
                task = result.get("task") if isinstance(result, dict) else None
                if not task and isinstance(result, dict):
                    task = result
                if not isinstance(task, dict):
                    return ToolResult(ok=False, data={}, error=t("tool.a2a.response_invalid"))
                task_id = str(task.get("id") or "").strip()
                context_id = str(task.get("contextId") or "").strip()
                status = task.get("status") if isinstance(task.get("status"), dict) else {}
                state = str(status.get("state") or "")
                artifacts = task.get("artifacts") if isinstance(task.get("artifacts"), list) else []
                for artifact in artifacts:
                    if not isinstance(artifact, dict):
                        continue
                    final_text_parts.extend(_extract_a2a_text_parts(artifact.get("parts")))
                metadata_value = task.get("metadata") if isinstance(task.get("metadata"), dict) else {}
                if isinstance(metadata_value.get("tokenUsage"), dict):
                    token_usage = metadata_value.get("tokenUsage")
                if include_raw:
                    raw_response = body
    except httpx.RequestError as exc:
        return ToolResult(
            ok=False,
            data={},
            error=t("tool.a2a.request_failed", detail=str(exc)),
        )

    if include_raw and raw_response is None and events:
        raw_response = events
    answer = "\n".join([part for part in final_text_parts if part]).strip()
    elapsed_ms = int((time.perf_counter() - start_ts) * 1000)
    ok = state not in {"failed", "cancelled", "rejected"} if state else True
    data: Dict[str, Any] = {
        "endpoint": endpoint,
        "method": method,
        "task_id": task_id,
        "context_id": context_id,
        "status": state,
        "answer": answer,
        "elapsed_ms": elapsed_ms,
        "delegation": metadata.get("delegation"),
    }
    if token_usage is not None:
        data["token_usage"] = token_usage
    if include_events and events:
        data["events"] = events
    if include_raw and raw_response is not None:
        data["raw"] = raw_response

    if context.emit_event:
        context.emit_event(
            "a2a_result",
            {
                "task_id": task_id,
                "context_id": context_id,
                "status": state,
                "elapsed_ms": elapsed_ms,
                "ok": ok,
            },
        )

    return ToolResult(ok=ok, data=data)
