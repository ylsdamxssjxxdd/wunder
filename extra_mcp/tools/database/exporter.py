from __future__ import annotations

import csv
import re
from datetime import datetime
from pathlib import Path
from typing import Any, Sequence
from uuid import uuid4

from .config import DbConfig, DbExportConfig, DbQueryTarget, get_db_export_config
from .db import (
    _has_multiple_statements,
    _is_read_only_sql,
    _normalize_value,
    normalize_sql_punctuation,
    open_connection,
)

SUPPORTED_EXPORT_FORMATS = ("xlsx", "csv")
INVALID_PATH_SEGMENT_CHARS = re.compile(r'[<>:"/\\|?*\x00-\x1f]')
WINDOWS_DRIVE_PATTERN = re.compile(r"^[a-zA-Z]:")
INVALID_SHEET_NAME_CHARS = re.compile(r"[:\\/?*\[\]]")
MAX_SHEET_NAME_LENGTH = 31
LIMIT_OR_OFFSET_PATTERN = re.compile(r"\b(?:limit|offset)\b", re.IGNORECASE)


def _normalize_export_format(format_value: str | None, requested_path: str | None) -> str:
    candidate = (format_value or "").strip().lower()
    if not candidate and requested_path:
        suffix = Path(requested_path.strip()).suffix.lower().lstrip(".")
        if suffix in SUPPORTED_EXPORT_FORMATS:
            candidate = suffix
    if not candidate:
        candidate = "xlsx"
    if candidate not in SUPPORTED_EXPORT_FORMATS:
        supported = ", ".join(SUPPORTED_EXPORT_FORMATS)
        raise ValueError(f"Unsupported export format '{candidate}'. Supported formats: {supported}")
    return candidate


def resolve_sql_request(
    *,
    sql: str | None,
    params: Sequence[Any] | None,
) -> tuple[str, list[Any] | None]:
    sql_text = normalize_sql_punctuation((sql or "").strip())
    if not sql_text:
        raise ValueError("SQL statement is required.")
    return sql_text, list(params) if params else None


def _sanitize_path_segment(value: str) -> str:
    cleaned = INVALID_PATH_SEGMENT_CHARS.sub("_", value.strip())
    cleaned = cleaned.strip(" .")
    return cleaned or "export"


def _sanitize_workspace_id(value: str) -> str:
    cleaned = value.strip()
    if not cleaned:
        return "anonymous"
    output = "".join(
        ch if ch.isascii() and (ch.isalnum() or ch in {"-", "_"}) else "_"
        for ch in cleaned
    )
    return output or "anonymous"


def _sanitize_sheet_name(value: str | None, default_name: str) -> str:
    candidate = INVALID_SHEET_NAME_CHARS.sub("_", (value or "").strip())
    candidate = candidate.strip("'")
    if not candidate:
        candidate = default_name
    return candidate[:MAX_SHEET_NAME_LENGTH] or default_name[:MAX_SHEET_NAME_LENGTH]


def _clean_relative_export_path(requested_path: str | None) -> Path | None:
    raw = (requested_path or "").strip()
    if not raw:
        return None
    normalized = raw.replace("\\", "/")
    if normalized.startswith("/") or WINDOWS_DRIVE_PATTERN.match(normalized):
        raise ValueError("Export path must be relative to the configured export root.")
    parts: list[str] = []
    for part in normalized.split("/"):
        segment = part.strip()
        if not segment or segment == ".":
            continue
        if segment == "..":
            raise ValueError("Export path cannot escape the configured export root.")
        parts.append(_sanitize_path_segment(segment))
    if not parts:
        raise ValueError("Export path cannot be empty.")
    return Path(*parts)


def _clean_workspace_relative_path(raw: str) -> Path:
    normalized = raw.replace("\\", "/")
    parts: list[str] = []
    for part in normalized.split("/"):
        segment = part.strip()
        if not segment or segment == ".":
            continue
        if segment == "..":
            raise ValueError("Workspace path cannot escape the configured workspace root.")
        parts.append(_sanitize_path_segment(segment))
    if not parts:
        raise ValueError("Workspace path cannot be empty.")
    return Path(*parts)


def _resolve_workspace_path_from_public_root(
    export_cfg: DbExportConfig,
    requested_path: str,
) -> tuple[Path, str, str | None, str]:
    normalized = requested_path.replace("\\", "/").strip()
    public_root = export_cfg.workspace_public_root.rstrip("/") or "/workspaces"
    prefixes = [public_root, public_root.lstrip("/")]
    for prefix in prefixes:
        candidate_prefix = prefix.rstrip("/")
        if not candidate_prefix:
            continue
        if normalized == candidate_prefix:
            raise ValueError("Workspace path must include a target file under the workspace root.")
        if normalized.startswith(candidate_prefix + "/"):
            relative = normalized[len(candidate_prefix) + 1 :]
            clean_relative = _clean_workspace_relative_path(relative)
            if not export_cfg.workspace_single_root and len(clean_relative.parts) < 2:
                raise ValueError(
                    "Workspace path must include the workspace id, for example /workspaces/{user_id}/exports/report.xlsx."
                )
            destination = (export_cfg.workspace_root.resolve() / clean_relative).resolve()
            destination.relative_to(export_cfg.workspace_root.resolve())
            if export_cfg.workspace_single_root:
                workspace_id = None
                workspace_relative_path = clean_relative.as_posix()
            else:
                workspace_id = _sanitize_workspace_id(clean_relative.parts[0])
                workspace_relative_path = Path(*clean_relative.parts[1:]).as_posix()
            public_path = f"{public_root}/{clean_relative.as_posix()}"
            return destination, public_path, workspace_id, workspace_relative_path
    raise ValueError("Not a workspace public path.")


def _resolve_workspace_path_from_absolute_fs(
    export_cfg: DbExportConfig,
    requested_path: str,
) -> tuple[Path, str, str | None, str]:
    raw_path = Path(requested_path).expanduser()
    if not raw_path.is_absolute():
        raise ValueError("Not an absolute workspace filesystem path.")
    workspace_root = export_cfg.workspace_root.resolve()
    destination = raw_path.resolve()
    relative = destination.relative_to(workspace_root)
    if not export_cfg.workspace_single_root and len(relative.parts) < 2:
        raise ValueError(
            "Workspace filesystem path must include the workspace id, for example /workspaces/<workspace_id>/exports/report.xlsx."
        )
    if export_cfg.workspace_single_root:
        workspace_id = None
        workspace_relative_path = relative.as_posix()
    else:
        workspace_id = _sanitize_workspace_id(relative.parts[0])
        workspace_relative_path = Path(*relative.parts[1:]).as_posix()
    public_path = f"{export_cfg.workspace_public_root}/{relative.as_posix()}"
    return destination, public_path, workspace_id, workspace_relative_path


def _resolve_destination_path(
    export_cfg: DbExportConfig,
    requested_path: str | None,
    export_format: str,
    default_stem: str,
    overwrite: bool,
) -> tuple[Path, str, dict[str, Any]]:
    raw = (requested_path or "").strip()
    if raw:
        try:
            destination, public_path, workspace_id, workspace_relative_path = _resolve_workspace_path_from_public_root(
                export_cfg,
                raw,
            )
            metadata = {
                "output_scope": "workspace",
                "public_path": public_path,
                "workspace_id": workspace_id,
                "workspace_relative_path": workspace_relative_path,
            }
        except ValueError:
            try:
                destination, public_path, workspace_id, workspace_relative_path = _resolve_workspace_path_from_absolute_fs(
                    export_cfg,
                    raw,
                )
                metadata = {
                    "output_scope": "workspace",
                    "public_path": public_path,
                    "workspace_id": workspace_id,
                    "workspace_relative_path": workspace_relative_path,
                }
            except (ValueError, OSError):
                destination, relative_path = _choose_destination_path(
                    export_cfg,
                    raw,
                    export_format,
                    default_stem,
                    overwrite,
                )
                metadata = {
                    "output_scope": "export_root",
                    "relative_path": relative_path,
                }
                return destination, relative_path, metadata

        if destination.suffix:
            current_format = destination.suffix.lower().lstrip(".")
            if current_format != export_format:
                raise ValueError(
                    f"Export path suffix '.{current_format}' does not match format '{export_format}'."
                )
        else:
            destination = destination.with_suffix(f".{export_format}")
            if metadata.get("output_scope") == "workspace":
                public_path = str(metadata.get("public_path") or "").rstrip("/") + f".{export_format}"
                metadata["public_path"] = public_path
                workspace_relative_path = str(metadata.get("workspace_relative_path") or "").rstrip("/")
                if workspace_relative_path:
                    metadata["workspace_relative_path"] = workspace_relative_path + f".{export_format}"

        destination.parent.mkdir(parents=True, exist_ok=True)
        if overwrite:
            key_path = str(metadata.get("public_path") or destination)
            return destination, key_path, metadata

        candidate = destination
        counter = 2
        while candidate.exists():
            candidate = destination.with_name(f"{destination.stem}_{counter}{destination.suffix}")
            counter += 1
        if metadata.get("output_scope") == "workspace":
            workspace_root = export_cfg.workspace_root.resolve()
            relative = candidate.relative_to(workspace_root)
            metadata["public_path"] = f"{export_cfg.workspace_public_root}/{relative.as_posix()}"
            if export_cfg.workspace_single_root:
                metadata["workspace_relative_path"] = relative.as_posix()
            else:
                metadata["workspace_id"] = _sanitize_workspace_id(relative.parts[0])
                metadata["workspace_relative_path"] = Path(*relative.parts[1:]).as_posix()
            return candidate, str(metadata["public_path"]), metadata
        relative_path = candidate.relative_to(export_cfg.root.resolve()).as_posix()
        metadata["relative_path"] = relative_path
        return candidate, relative_path, metadata

    destination, relative_path = _choose_destination_path(
        export_cfg,
        None,
        export_format,
        default_stem,
        overwrite,
    )
    metadata = {
        "output_scope": "export_root",
        "relative_path": relative_path,
    }
    return destination, relative_path, metadata


def _choose_destination_path(
    export_cfg: DbExportConfig,
    requested_path: str | None,
    export_format: str,
    default_stem: str,
    overwrite: bool,
) -> tuple[Path, str]:
    export_root = export_cfg.root.resolve()
    export_root.mkdir(parents=True, exist_ok=True)

    relative_path = _clean_relative_export_path(requested_path)
    if relative_path is None:
        timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
        relative_path = Path(f"{_sanitize_path_segment(default_stem)}_{timestamp}.{export_format}")

    if relative_path.suffix:
        current_format = relative_path.suffix.lower().lstrip(".")
        if current_format != export_format:
            raise ValueError(
                f"Export path suffix '.{current_format}' does not match format '{export_format}'."
            )
    else:
        relative_path = relative_path.with_suffix(f".{export_format}")

    destination = (export_root / relative_path).resolve()
    destination.relative_to(export_root)
    destination.parent.mkdir(parents=True, exist_ok=True)

    if overwrite:
        return destination, destination.relative_to(export_root).as_posix()

    candidate = destination
    counter = 2
    while candidate.exists():
        candidate = destination.with_name(f"{destination.stem}_{counter}{destination.suffix}")
        counter += 1
    return candidate, candidate.relative_to(export_root).as_posix()


def _open_export_cursor(connection: Any, cfg: DbConfig):
    if cfg.engine == "mysql":
        try:
            import pymysql
        except ImportError as exc:  # pragma: no cover
            raise RuntimeError("Missing dependency: pip install pymysql") from exc
        return connection.cursor(pymysql.cursors.SSCursor)

    return connection.cursor()


def _validate_export_sql(sql: str, allow_write: bool, allow_limited_export: bool) -> None:
    if _has_multiple_statements(sql):
        raise ValueError("Only a single SQL statement is allowed.")
    if not allow_write and not _is_read_only_sql(sql):
        raise ValueError("Only read-only SQL is allowed (SELECT/SHOW/DESCRIBE/EXPLAIN/WITH).")
    if not allow_limited_export and LIMIT_OR_OFFSET_PATTERN.search(sql):
        raise ValueError(
            "db_export rejects SQL with LIMIT/OFFSET by default to avoid accidental partial exports. Remove LIMIT/OFFSET for formal full exports, or set allow_limited_export=true if you intentionally want a partial export."
        )


def export_sql_to_file_sync(
    cfg: DbConfig,
    sql: str,
    params: list[Any] | None,
    *,
    target: DbQueryTarget | None,
    path: str | None,
    export_format: str,
    sheet_name: str | None,
    overwrite: bool,
    allow_limited_export: bool = False,
    allow_write: bool = False,
) -> dict[str, Any]:
    sql_text = normalize_sql_punctuation(sql.strip())
    if not sql_text:
        raise ValueError("SQL statement cannot be empty.")
    _validate_export_sql(sql_text, allow_write, allow_limited_export)
    export_format = _normalize_export_format(export_format, path)

    export_cfg = get_db_export_config()
    default_stem = target.table if target is not None else "query_export"
    destination, key_path, output_metadata = _resolve_destination_path(
        export_cfg,
        path,
        export_format,
        default_stem,
        overwrite,
    )
    tmp_path = destination.with_name(f".{destination.name}.{uuid4().hex}.part")

    connection = open_connection(cfg)
    row_count = 0
    columns: list[str] = []
    actual_sheet_name = None
    try:
        with _open_export_cursor(connection, cfg) as cursor:
            if params:
                cursor.execute(sql_text, params)
            else:
                # Avoid driver-side `%` interpolation when SQL includes
                # patterns like DATE_FORMAT(..., '%Y-%m') without bind params.
                cursor.execute(sql_text)
            if cursor.description:
                columns = [str(col[0]) for col in cursor.description]

            if export_format == "xlsx":
                try:
                    from openpyxl import Workbook
                except ImportError as exc:  # pragma: no cover
                    raise RuntimeError("Missing dependency: pip install openpyxl") from exc

                workbook = Workbook(write_only=True)
                actual_sheet_name = _sanitize_sheet_name(sheet_name, default_stem or "Sheet1")
                worksheet = workbook.create_sheet(title=actual_sheet_name)
                if columns:
                    worksheet.append(columns)
                while True:
                    batch = cursor.fetchmany(export_cfg.batch_size)
                    if not batch:
                        break
                    for row in batch:
                        worksheet.append([_normalize_value(value) for value in row])
                        row_count += 1
                workbook.save(tmp_path)
                workbook.close()
            else:
                with tmp_path.open("w", encoding=export_cfg.csv_encoding, newline="") as handle:
                    writer = csv.writer(handle)
                    if columns:
                        writer.writerow(columns)
                    while True:
                        batch = cursor.fetchmany(export_cfg.batch_size)
                        if not batch:
                            break
                        for row in batch:
                            writer.writerow([_normalize_value(value) for value in row])
                            row_count += 1

        tmp_path.replace(destination)
        result = {
            "ok": True,
            "format": export_format,
            "path": str(output_metadata.get("public_path") or key_path),
            "row_count": row_count,
            "columns": columns,
            "bytes": destination.stat().st_size,
        }
        if actual_sheet_name:
            result["sheet_name"] = actual_sheet_name
        workspace_relative_path = output_metadata.get("workspace_relative_path")
        if workspace_relative_path:
            result["workspace_relative_path"] = workspace_relative_path
        return result
    except Exception:
        if tmp_path.exists():
            tmp_path.unlink(missing_ok=True)
        raise
    finally:
        connection.close()
