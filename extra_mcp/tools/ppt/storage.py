from __future__ import annotations

import json
import re
from datetime import datetime
from pathlib import Path
from typing import Any

from .config import PptConfig
from .model import PresentationManifest, make_presentation_id

INVALID_PATH_SEGMENT_CHARS = re.compile(r'[<>:"/\\|?*\x00-\x1f]')
WINDOWS_DRIVE_PATTERN = re.compile(r"^[a-zA-Z]:")


def create_presentation_dir(config: PptConfig, presentation_id: str | None = None) -> Path:
    pid = sanitize_presentation_id(presentation_id or make_presentation_id())
    path = (config.root / pid).resolve()
    path.relative_to(config.root.resolve())
    path.mkdir(parents=True, exist_ok=True)
    return path


def load_manifest(config: PptConfig, presentation_id: str) -> PresentationManifest:
    manifest_path = manifest_file(config, presentation_id)
    if not manifest_path.exists():
        raise ValueError(f"presentation_id not found: {presentation_id}")
    data = json.loads(manifest_path.read_text(encoding="utf-8"))
    if not isinstance(data, dict):
        raise ValueError("manifest must be a JSON object.")
    return PresentationManifest.from_dict(data)


def save_manifest(config: PptConfig, manifest: PresentationManifest) -> Path:
    directory = presentation_dir(config, manifest.presentation_id)
    directory.mkdir(parents=True, exist_ok=True)
    path = directory / "manifest.json"
    path.write_text(
        json.dumps(manifest.to_dict(), ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    return path


def manifest_file(config: PptConfig, presentation_id: str) -> Path:
    return presentation_dir(config, presentation_id) / "manifest.json"


def presentation_dir(config: PptConfig, presentation_id: str) -> Path:
    pid = sanitize_presentation_id(presentation_id)
    path = (config.root / pid).resolve()
    path.relative_to(config.root.resolve())
    return path


def default_output_path(config: PptConfig, presentation_id: str, name: str) -> Path:
    stem = sanitize_filename(name) or presentation_id
    directory = presentation_dir(config, presentation_id)
    directory.mkdir(parents=True, exist_ok=True)
    return unique_path(directory / f"{stem}.pptx")


def resolve_output_path(
    config: PptConfig,
    requested_path: str,
    presentation_id: str,
    presentation_name: str,
    overwrite: bool,
) -> tuple[Path, dict[str, Any]]:
    raw = (requested_path or "").strip()
    if not raw:
        output = default_output_path(config, presentation_id, presentation_name)
        return output, {"output_scope": "ppt_root", "relative_path": output.relative_to(config.root).as_posix()}

    try:
        destination, public_path, workspace_id, workspace_relative_path = _resolve_workspace_public_path(
            config,
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
            destination, public_path, workspace_id, workspace_relative_path = _resolve_workspace_fs_path(
                config,
                raw,
            )
            metadata = {
                "output_scope": "workspace",
                "public_path": public_path,
                "workspace_id": workspace_id,
                "workspace_relative_path": workspace_relative_path,
            }
        except (ValueError, OSError):
            relative = _clean_relative_path(raw)
            destination = (presentation_dir(config, presentation_id) / relative).resolve()
            destination.relative_to(presentation_dir(config, presentation_id).resolve())
            metadata = {
                "output_scope": "ppt_root",
                "relative_path": destination.relative_to(config.root.resolve()).as_posix(),
            }

    if destination.suffix.lower() != ".pptx":
        destination = destination.with_suffix(".pptx")
        if metadata.get("output_scope") == "workspace":
            public_path = str(metadata.get("public_path") or "").rstrip("/")
            metadata["public_path"] = f"{public_path}.pptx"
            workspace_relative_path = str(metadata.get("workspace_relative_path") or "").rstrip("/")
            if workspace_relative_path:
                metadata["workspace_relative_path"] = f"{workspace_relative_path}.pptx"

    destination.parent.mkdir(parents=True, exist_ok=True)
    if destination.exists() and not overwrite:
        destination = unique_path(destination)
        if metadata.get("output_scope") == "workspace":
            metadata = _workspace_metadata_for_destination(config, destination)
        else:
            metadata["relative_path"] = destination.relative_to(config.root.resolve()).as_posix()
    return destination, metadata


def unique_path(path: Path) -> Path:
    if not path.exists():
        return path
    stamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    candidate = path.with_name(f"{path.stem}_{stamp}{path.suffix}")
    counter = 2
    while candidate.exists():
        candidate = path.with_name(f"{path.stem}_{stamp}_{counter}{path.suffix}")
        counter += 1
    return candidate


def output_metadata_to_manifest(manifest: PresentationManifest, output: Path, metadata: dict[str, Any]) -> None:
    manifest.output_path = str(output)
    manifest.public_path = str(metadata.get("public_path") or str(output))
    manifest.workspace_relative_path = str(metadata.get("workspace_relative_path") or "")


def resolve_readable_pptx_path(config: PptConfig, path_text: str) -> Path:
    raw = (path_text or "").strip()
    if not raw:
        raise ValueError("path is required.")
    try:
        destination, _, _, _ = _resolve_workspace_public_path(config, raw)
    except ValueError:
        candidate = Path(raw).expanduser()
        if candidate.is_absolute():
            destination = candidate.resolve()
        else:
            destination = (config.root / _clean_relative_path(raw)).resolve()

    allowed_roots = (config.root.resolve(), config.workspace_root.resolve())
    if not any(_is_relative_to(destination, root) for root in allowed_roots):
        raise ValueError("path must be under the PPT output root or workspace root.")
    if destination.suffix.lower() != ".pptx":
        raise ValueError("path must point to a .pptx file.")
    if not destination.exists() or not destination.is_file():
        raise ValueError(f"pptx file not found: {destination}")
    return destination


def sanitize_presentation_id(value: str) -> str:
    cleaned = re.sub(r"[^A-Za-z0-9_.-]+", "_", value.strip())
    cleaned = cleaned.strip("._-")
    if not cleaned:
        raise ValueError("presentation_id is required.")
    return cleaned[:80]


def sanitize_filename(value: str) -> str:
    cleaned = INVALID_PATH_SEGMENT_CHARS.sub("_", value.strip())
    cleaned = cleaned.strip(" .")
    return cleaned[:80] or "presentation"


def _clean_relative_path(raw: str) -> Path:
    normalized = raw.replace("\\", "/")
    if normalized.startswith("/") or WINDOWS_DRIVE_PATTERN.match(normalized):
        raise ValueError("path must be relative or under the workspace root.")
    parts: list[str] = []
    for part in normalized.split("/"):
        raw_segment = part.strip()
        if not raw_segment or raw_segment == ".":
            continue
        if raw_segment == "..":
            raise ValueError("path cannot escape the output root.")
        segment = _sanitize_path_segment(raw_segment)
        if not segment:
            continue
        parts.append(segment)
    if not parts:
        raise ValueError("path cannot be empty.")
    return Path(*parts)


def _clean_workspace_relative_path(raw: str) -> Path:
    normalized = raw.replace("\\", "/")
    parts: list[str] = []
    for part in normalized.split("/"):
        raw_segment = part.strip()
        if not raw_segment or raw_segment == ".":
            continue
        if raw_segment == "..":
            raise ValueError("workspace path cannot escape the workspace root.")
        segment = _sanitize_path_segment(raw_segment)
        if not segment:
            continue
        parts.append(segment)
    if not parts:
        raise ValueError("workspace path cannot be empty.")
    return Path(*parts)


def _resolve_workspace_public_path(config: PptConfig, requested_path: str) -> tuple[Path, str, str | None, str]:
    normalized = requested_path.replace("\\", "/").strip()
    public_root = config.workspace_public_root.rstrip("/") or "/workspaces"
    for prefix in (public_root, public_root.lstrip("/")):
        candidate_prefix = prefix.rstrip("/")
        if not candidate_prefix:
            continue
        if normalized == candidate_prefix:
            raise ValueError("workspace path must include a target file.")
        if normalized.startswith(candidate_prefix + "/"):
            relative = normalized[len(candidate_prefix) + 1 :]
            clean_relative = _clean_workspace_relative_path(relative)
            if not config.workspace_single_root and len(clean_relative.parts) < 2:
                raise ValueError("workspace path must include workspace id.")
            destination = (config.workspace_root / clean_relative).resolve()
            destination.relative_to(config.workspace_root.resolve())
            if config.workspace_single_root:
                workspace_id = None
                workspace_relative_path = clean_relative.as_posix()
            else:
                workspace_id = _sanitize_workspace_id(clean_relative.parts[0])
                workspace_relative_path = Path(*clean_relative.parts[1:]).as_posix()
            return (
                destination,
                f"{public_root}/{clean_relative.as_posix()}",
                workspace_id,
                workspace_relative_path,
            )
    raise ValueError("not a workspace public path.")


def _resolve_workspace_fs_path(config: PptConfig, requested_path: str) -> tuple[Path, str, str | None, str]:
    raw = requested_path.strip()
    path = Path(raw).expanduser()
    if not path.is_absolute():
        raise ValueError("not an absolute workspace path.")
    destination = path.resolve()
    relative = destination.relative_to(config.workspace_root.resolve())
    if not config.workspace_single_root and len(relative.parts) < 2:
        raise ValueError("workspace filesystem path must include workspace id.")
    if config.workspace_single_root:
        workspace_id = None
        workspace_relative_path = relative.as_posix()
    else:
        workspace_id = _sanitize_workspace_id(relative.parts[0])
        workspace_relative_path = Path(*relative.parts[1:]).as_posix()
    public_path = f"{config.workspace_public_root}/{relative.as_posix()}"
    return destination, public_path, workspace_id, workspace_relative_path


def _workspace_metadata_for_destination(config: PptConfig, destination: Path) -> dict[str, Any]:
    relative = destination.resolve().relative_to(config.workspace_root.resolve())
    public_path = f"{config.workspace_public_root}/{relative.as_posix()}"
    if config.workspace_single_root:
        return {
            "output_scope": "workspace",
            "public_path": public_path,
            "workspace_id": None,
            "workspace_relative_path": relative.as_posix(),
        }
    return {
        "output_scope": "workspace",
        "public_path": public_path,
        "workspace_id": _sanitize_workspace_id(relative.parts[0]),
        "workspace_relative_path": Path(*relative.parts[1:]).as_posix(),
    }


def _sanitize_path_segment(value: str) -> str:
    cleaned = INVALID_PATH_SEGMENT_CHARS.sub("_", value.strip())
    cleaned = cleaned.strip(" .")
    return cleaned or ""


def _sanitize_workspace_id(value: str) -> str:
    cleaned = value.strip()
    if not cleaned:
        return "anonymous"
    output = "".join(ch if ch.isascii() and (ch.isalnum() or ch in {"-", "_"}) else "_" for ch in cleaned)
    return output or "anonymous"


def _is_relative_to(path: Path, root: Path) -> bool:
    try:
        path.relative_to(root)
        return True
    except ValueError:
        return False
