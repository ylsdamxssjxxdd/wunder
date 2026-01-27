from __future__ import annotations

import re
from typing import Any

from pydantic import BaseModel, ConfigDict, Field, field_validator

MAX_TABLE_FILTERS = 50
MAX_COLUMN_FILTERS = 200
IDENTIFIER_RE = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")


def _validate_identifier(value: str) -> str:
    if not value or not IDENTIFIER_RE.fullmatch(value):
        raise ValueError("Identifier must be alphanumeric with underscores and not start with a digit.")
    return value


class BaseInput(BaseModel):
    model_config = ConfigDict(extra="forbid", str_strip_whitespace=True)

    db_key: str | None = Field(
        default=None,
        description="Optional database target key (for PERSONNEL_DB_TARGETS).",
        max_length=64,
    )

    @field_validator("db_key")
    @classmethod
    def validate_db_key(cls, value: str | None) -> str | None:
        if value is None:
            return value
        return _validate_identifier(value)


class SchemaInput(BaseInput):
    """Input for personnel_get_schema."""

    database: str | None = Field(
        default=None,
        description="Database name. If omitted, uses configured default.",
        min_length=1,
        max_length=128,
    )
    tables: list[str] | None = Field(
        default=None,
        description="Optional table whitelist to reduce schema size.",
        max_length=MAX_TABLE_FILTERS,
    )

    @field_validator("tables")
    @classmethod
    def validate_tables(cls, value: list[str] | None) -> list[str] | None:
        if value is None:
            return value
        return [_validate_identifier(item) for item in value if item]


class QueryInput(BaseInput):
    """Input for personnel_query."""

    sql: str = Field(
        ...,
        description="SQL statement to execute.",
        min_length=1,
        max_length=10000,
    )
    params: list[str | int | float | bool | None] | None = Field(
        default=None,
        description="Optional parameters for the SQL statement.",
    )
    database: str | None = Field(
        default=None,
        description="Database name. If omitted, uses configured default.",
        min_length=1,
        max_length=128,
    )
    max_rows: int = Field(
        default=200,
        description="Max rows to return for result sets.",
        ge=1,
        le=5000,
    )
    allow_write: bool = Field(
        default=False,
        description="Allow non read-only SQL when true.",
    )


class DatabaseListInput(BaseInput):
    """Input for personnel_list_databases."""


class TableListInput(BaseInput):
    """Input for personnel_list_tables."""

    database: str | None = Field(
        default=None,
        description="Database name. If omitted, uses configured default.",
        min_length=1,
        max_length=128,
    )
    pattern: str | None = Field(
        default=None,
        description="Optional SQL LIKE pattern to filter table names.",
        max_length=128,
    )
    limit: int = Field(
        default=200,
        description="Max number of tables to return.",
        ge=1,
        le=2000,
    )


class TableInput(BaseInput):
    """Base input for table-specific operations."""

    table: str = Field(
        ...,
        description="Table name.",
        min_length=1,
        max_length=128,
    )
    database: str | None = Field(
        default=None,
        description="Database name. If omitted, uses configured default.",
        min_length=1,
        max_length=128,
    )

    @field_validator("table")
    @classmethod
    def validate_table(cls, value: str) -> str:
        return _validate_identifier(value)


class TableDescribeInput(TableInput):
    """Input for personnel_describe_table."""


class TableCountInput(TableInput):
    """Input for personnel_count_rows."""


class TablePreviewInput(TableInput):
    """Input for personnel_preview_rows."""

    columns: list[str] | None = Field(
        default=None,
        description="Optional column whitelist; defaults to all columns.",
        max_length=MAX_COLUMN_FILTERS,
    )
    limit: int = Field(
        default=50,
        description="Max number of rows to return.",
        ge=1,
        le=500,
    )
    order_by: str | None = Field(
        default=None,
        description="Optional column name to order by.",
        max_length=128,
    )
    order_desc: bool = Field(
        default=False,
        description="Order by descending when true.",
    )

    @field_validator("columns")
    @classmethod
    def validate_columns(cls, value: list[str] | None) -> list[str] | None:
        if value is None:
            return value
        cleaned = [_validate_identifier(item) for item in value if item]
        return cleaned or None

    @field_validator("order_by")
    @classmethod
    def validate_order_by(cls, value: str | None) -> str | None:
        if value is None:
            return value
        return _validate_identifier(value)


class HealthInput(BaseInput):
    """Input for personnel_ping."""

    database: str | None = Field(
        default=None,
        description="Database name. If omitted, uses configured default.",
        min_length=1,
        max_length=128,
    )


StructuredResult = dict[str, Any]
