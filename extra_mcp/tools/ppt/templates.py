from __future__ import annotations

from copy import deepcopy
from typing import Any

DEFAULT_TEMPLATE_ID = "amber_clear"

COLOR_KEYS = {
    "bg",
    "surface",
    "surface_alt",
    "primary",
    "secondary",
    "muted",
    "line",
    "accent",
    "accent2",
    "accent3",
    "success",
    "danger",
    "section_bg",
    "data_panel",
    "cover_panel",
}

BASE_THEME: dict[str, str] = {
    "bg": "F8FAFC",
    "surface": "FFFFFF",
    "surface_alt": "F1F5F9",
    "primary": "1F2937",
    "secondary": "475569",
    "muted": "64748B",
    "line": "E2E8F0",
    "accent": "D97706",
    "accent2": "0F766E",
    "accent3": "7C3AED",
    "success": "16A34A",
    "danger": "B91C1C",
    "section_bg": "1F2937",
    "data_panel": "FFF7ED",
    "cover_panel": "1F2937",
}

BUILTIN_TEMPLATES: dict[str, dict[str, Any]] = {
    "amber_clear": {
        "name": "Amber Clear",
        "description": "Warm general-purpose report style with amber and teal accents.",
        "suitable_for": ["general report", "project update", "training"],
        "theme": {
            **BASE_THEME,
            "accent": "D97706",
            "accent2": "0F766E",
            "accent3": "7C3AED",
            "data_panel": "FFF7ED",
        },
    },
    "executive_green": {
        "name": "Executive Green",
        "description": "Quiet executive deck with deep green, graphite, and restrained gold.",
        "suitable_for": ["business review", "strategy", "management briefing"],
        "theme": {
            **BASE_THEME,
            "bg": "F7F8F4",
            "surface": "FFFFFF",
            "surface_alt": "EEF4EC",
            "primary": "17211B",
            "secondary": "3F5147",
            "muted": "6A766F",
            "line": "DCE5DC",
            "accent": "1F7A4D",
            "accent2": "B88A2E",
            "accent3": "506C85",
            "section_bg": "173B2B",
            "cover_panel": "173B2B",
            "data_panel": "F3F8EF",
        },
    },
    "research_blue": {
        "name": "Research Blue",
        "description": "Academic and technical style using blue, slate, and clear figure blocks.",
        "suitable_for": ["research", "technical report", "paper presentation"],
        "theme": {
            **BASE_THEME,
            "bg": "F6F9FC",
            "surface": "FFFFFF",
            "surface_alt": "EAF1F8",
            "primary": "163047",
            "secondary": "425B73",
            "muted": "6A7D8F",
            "line": "D7E2EA",
            "accent": "2563A8",
            "accent2": "0E8F92",
            "accent3": "8B5E34",
            "section_bg": "12314C",
            "cover_panel": "12314C",
            "data_panel": "EEF6FF",
        },
    },
    "finance_ink": {
        "name": "Finance Ink",
        "description": "High-contrast boardroom style with ink, ivory, and numeric emphasis.",
        "suitable_for": ["finance", "market analysis", "board presentation"],
        "theme": {
            **BASE_THEME,
            "bg": "FBFAF6",
            "surface": "FFFFFF",
            "surface_alt": "F0ECE2",
            "primary": "111827",
            "secondary": "374151",
            "muted": "6B7280",
            "line": "DED8C8",
            "accent": "A16207",
            "accent2": "334155",
            "accent3": "0F766E",
            "section_bg": "111827",
            "cover_panel": "111827",
            "data_panel": "F6F0DF",
        },
    },
    "creative_coral": {
        "name": "Creative Coral",
        "description": "Modern product narrative style with coral, indigo, and mint accents.",
        "suitable_for": ["product plan", "creative proposal", "workshop"],
        "theme": {
            **BASE_THEME,
            "bg": "FFFBFA",
            "surface": "FFFFFF",
            "surface_alt": "F5F0FF",
            "primary": "231F33",
            "secondary": "51465F",
            "muted": "7A7185",
            "line": "E8DDEB",
            "accent": "E75A4A",
            "accent2": "5A67D8",
            "accent3": "2AA889",
            "section_bg": "2C2443",
            "cover_panel": "2C2443",
            "data_panel": "FFF0EB",
        },
    },
    "minimal_gray": {
        "name": "Minimal Gray",
        "description": "Clean neutral style with small red and blue accents for concise decks.",
        "suitable_for": ["briefing", "internal memo", "simple summary"],
        "theme": {
            **BASE_THEME,
            "bg": "F7F7F5",
            "surface": "FFFFFF",
            "surface_alt": "EFEFED",
            "primary": "242424",
            "secondary": "4D4D4D",
            "muted": "777777",
            "line": "DADAD6",
            "accent": "C2413A",
            "accent2": "2F6F8F",
            "accent3": "6B7280",
            "section_bg": "242424",
            "cover_panel": "242424",
            "data_panel": "F2F2EF",
        },
    },
}


def normalize_template_id(value: str | None) -> str:
    raw = (value or "").strip().lower().replace("-", "_").replace(" ", "_")
    aliases = {
        "default": DEFAULT_TEMPLATE_ID,
        "business": "executive_green",
        "executive": "executive_green",
        "academic": "research_blue",
        "research": "research_blue",
        "technical": "research_blue",
        "finance": "finance_ink",
        "financial": "finance_ink",
        "creative": "creative_coral",
        "product": "creative_coral",
        "minimal": "minimal_gray",
        "simple": "minimal_gray",
    }
    candidate = aliases.get(raw, raw)
    if candidate in BUILTIN_TEMPLATES:
        return candidate
    return DEFAULT_TEMPLATE_ID


def is_builtin_template(value: str | None) -> bool:
    raw = (value or "").strip().lower().replace("-", "_").replace(" ", "_")
    return raw in BUILTIN_TEMPLATES or raw in {
        "default",
        "business",
        "executive",
        "academic",
        "research",
        "technical",
        "finance",
        "financial",
        "creative",
        "product",
        "minimal",
        "simple",
    }


def theme_for_template(template_id: str | None) -> dict[str, str]:
    normalized = normalize_template_id(template_id)
    return deepcopy(BUILTIN_TEMPLATES[normalized]["theme"])


def list_builtin_templates() -> list[dict[str, Any]]:
    output: list[dict[str, Any]] = []
    for template_id, data in BUILTIN_TEMPLATES.items():
        output.append(
            {
                "template_id": template_id,
                "name": data["name"],
                "description": data["description"],
                "suitable_for": list(data["suitable_for"]),
                "theme": deepcopy(data["theme"]),
            }
        )
    return output


def builtin_template_summary(template_id: str) -> dict[str, Any]:
    normalized = normalize_template_id(template_id)
    data = BUILTIN_TEMPLATES[normalized]
    return {
        "template_id": normalized,
        "name": data["name"],
        "description": data["description"],
        "suitable_for": list(data["suitable_for"]),
        "theme": deepcopy(data["theme"]),
    }
