#!/usr/bin/env python3
"""
Quick validation script for skills - minimal version.

This validator intentionally uses only the Python standard library so it can
run in bare desktop environments without optional dependencies such as PyYAML.
"""

import re
import sys
from pathlib import Path

ALLOWED_PROPERTIES = {"name", "description", "license", "allowed-tools", "metadata"}


def configure_stdio():
    """Best-effort UTF-8 stdio configuration for Windows and legacy terminals."""
    for stream_name in ("stdout", "stderr"):
        stream = getattr(sys, stream_name, None)
        if stream is None:
            continue
        reconfigure = getattr(stream, "reconfigure", None)
        if callable(reconfigure):
            try:
                reconfigure(encoding="utf-8", errors="replace")
            except Exception:
                pass


def _strip_matching_quotes(value):
    value = value.strip()
    if len(value) >= 2 and value[0] == value[-1] and value[0] in {'"', "'"}:
        return value[1:-1]
    return value


def _parse_scalar(raw_value):
    lowered = raw_value.lower()
    if lowered == "true":
        return True
    if lowered == "false":
        return False
    return _strip_matching_quotes(raw_value)


def _parse_frontmatter(frontmatter_text):
    """
    Parse the limited YAML subset used by mounted skills.

    Supported forms:
    - key: value
    - key: "quoted value"
    - key: |
        multiline
    - key: >
        multiline
    """
    result = {}
    current_key = None
    current_indent = None
    block_lines = []

    def flush_block():
        nonlocal current_key, current_indent, block_lines
        if current_key is None:
            return
        result[current_key] = "\n".join(block_lines).rstrip("\n")
        current_key = None
        current_indent = None
        block_lines = []

    for index, raw_line in enumerate(frontmatter_text.splitlines(), 1):
        line = raw_line.rstrip("\r")
        stripped = line.strip()

        if not stripped:
            if current_key is not None:
                block_lines.append("")
            continue

        if stripped.startswith("#"):
            continue

        indent = len(line) - len(line.lstrip(" "))
        if current_key is not None:
            if indent <= current_indent:
                flush_block()
            else:
                block_lines.append(line[current_indent + 1 :])
                continue

        if ":" not in line:
            raise ValueError(f"Invalid frontmatter line {index}: {line}")

        key, raw_value = line.split(":", 1)
        key = key.strip()
        raw_value = raw_value.strip()

        if not key:
            raise ValueError(f"Missing key on line {index}")
        if key in result:
            raise ValueError(f"Duplicate key '{key}' in frontmatter")

        if raw_value in {"|", ">"} or raw_value == "":
            current_key = key
            current_indent = indent
            block_lines = []
            continue

        result[key] = _parse_scalar(raw_value)

    flush_block()
    return result


def validate_skill(skill_path):
    """Basic validation of a skill."""
    skill_path = Path(skill_path).resolve()

    skill_md = skill_path / "SKILL.md"
    if not skill_md.exists():
        return False, "SKILL.md not found"

    content = skill_md.read_text(encoding="utf-8")
    if not content.startswith("---"):
        return False, "No YAML frontmatter found"

    match = re.match(r"^---\r?\n(.*?)\r?\n---", content, re.DOTALL)
    if not match:
        return False, "Invalid frontmatter format"

    frontmatter_text = match.group(1)
    try:
        frontmatter = _parse_frontmatter(frontmatter_text)
    except ValueError as err:
        return False, f"Invalid YAML in frontmatter: {err}"

    unexpected_keys = set(frontmatter.keys()) - ALLOWED_PROPERTIES
    if unexpected_keys:
        return False, (
            "Unexpected key(s) in SKILL.md frontmatter: "
            f"{', '.join(sorted(unexpected_keys))}. "
            f"Allowed properties are: {', '.join(sorted(ALLOWED_PROPERTIES))}"
        )

    if "name" not in frontmatter:
        return False, "Missing 'name' in frontmatter"
    if "description" not in frontmatter:
        return False, "Missing 'description' in frontmatter"

    name = frontmatter.get("name", "")
    if not isinstance(name, str):
        return False, f"Name must be a string, got {type(name).__name__}"
    name = name.strip()
    if name:
        if not re.match(r"^[a-z0-9-]+$", name):
            return (
                False,
                f"Name '{name}' should be hyphen-case (lowercase letters, digits, and hyphens only)",
            )
        if name.startswith("-") or name.endswith("-") or "--" in name:
            return (
                False,
                f"Name '{name}' cannot start/end with hyphen or contain consecutive hyphens",
            )
        if len(name) > 64:
            return False, f"Name is too long ({len(name)} characters). Maximum is 64 characters."

    description = frontmatter.get("description", "")
    if not isinstance(description, str):
        return False, f"Description must be a string, got {type(description).__name__}"
    description = description.strip()
    if description:
        if "<" in description or ">" in description:
            return False, "Description cannot contain angle brackets (< or >)"
        if len(description) > 1024:
            return (
                False,
                f"Description is too long ({len(description)} characters). Maximum is 1024 characters.",
            )

    return True, "Skill is valid!"


if __name__ == "__main__":
    configure_stdio()
    if len(sys.argv) != 2:
        print("Usage: python quick_validate.py <skill_directory>")
        sys.exit(1)

    valid, message = validate_skill(sys.argv[1])
    print(message)
    sys.exit(0 if valid else 1)
