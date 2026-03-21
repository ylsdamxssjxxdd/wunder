import sys
import types
import unittest
from pathlib import Path
from uuid import uuid4

import pymysql


def _install_mcp_stub() -> None:
    if "mcp.server.fastmcp" in sys.modules:
        return
    mcp_mod = types.ModuleType("mcp")
    server_mod = types.ModuleType("mcp.server")
    fastmcp_mod = types.ModuleType("mcp.server.fastmcp")

    class FastMCP:  # pragma: no cover - import stub only
        pass

    fastmcp_mod.FastMCP = FastMCP
    server_mod.fastmcp = fastmcp_mod
    mcp_mod.server = server_mod
    sys.modules["mcp"] = mcp_mod
    sys.modules["mcp.server"] = server_mod
    sys.modules["mcp.server.fastmcp"] = fastmcp_mod


_install_mcp_stub()

from extra_mcp.tools.database.config import get_db_config, get_db_export_config
from extra_mcp.tools.database.db import (
    describe_table_sync,
    execute_sql_sync,
    get_table_schema_compact_sync,
    validate_sql_against_target_table,
)
from extra_mcp.tools.database.exporter import export_sql_to_file_sync


CHINESE_TABLE_LABEL = "\u4e2d\u6587\u5b57\u6bb5\u56de\u5f52"
CHINESE_COLUMNS = [
    "\u7f16\u53f7",
    "\u59d3\u540d",
    "\u90e8\u95e8",
    "\u72b6\u6001",
    "\u8bf4\u660e",
]
CHINESE_ROWS = [
    (
        1,
        "\u5f20\u4e09",
        "\u7814\u53d1\u4e2d\u5fc3",
        "\u6b63\u5e38",
        "\u5305\u542b\u4e2d\u6587\u5217\u540d",
    ),
    (
        2,
        "\u674e\u56db",
        "\u5e02\u573a\u90e8",
        "\u5f85\u786e\u8ba4",
        "\u7b2c\u4e8c\u884c",
    ),
]


class ExtraMcpDatabaseUnicodeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        try:
            cls.cfg = get_db_config(None, "local_hr_mysql")
            cls.connection = pymysql.connect(
                host=cls.cfg.host,
                port=cls.cfg.port,
                user=cls.cfg.user,
                password=cls.cfg.password,
                database=cls.cfg.database,
                charset="utf8mb4",
                autocommit=True,
            )
        except Exception as exc:  # pragma: no cover - environment dependent
            raise unittest.SkipTest(f"MySQL target unavailable: {exc}") from exc

    @classmethod
    def tearDownClass(cls) -> None:
        connection = getattr(cls, "connection", None)
        if connection is not None:
            connection.close()

    def setUp(self) -> None:
        self.table_name = f"tmp_mcp_{CHINESE_TABLE_LABEL}_{uuid4().hex[:8]}"
        self.quoted_table = f"`{self.table_name}`"
        self.quoted_columns = [f"`{name}`" for name in CHINESE_COLUMNS]
        self._create_table()

    def tearDown(self) -> None:
        self._drop_table()

    def _create_table(self) -> None:
        with self.connection.cursor() as cursor:
            self._drop_table()
            cursor.execute(
                f"CREATE TABLE {self.quoted_table} ("
                f"{self.quoted_columns[0]} INT PRIMARY KEY, "
                f"{self.quoted_columns[1]} VARCHAR(64) NOT NULL, "
                f"{self.quoted_columns[2]} VARCHAR(64) NOT NULL, "
                f"{self.quoted_columns[3]} VARCHAR(32) NOT NULL, "
                f"{self.quoted_columns[4]} VARCHAR(128) NULL"
                f") CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_ai_ci"
            )
            cursor.execute(
                f"INSERT INTO {self.quoted_table} ({', '.join(self.quoted_columns)}) "
                f"VALUES (%s,%s,%s,%s,%s),(%s,%s,%s,%s,%s)",
                CHINESE_ROWS[0] + CHINESE_ROWS[1],
            )

    def _drop_table(self) -> None:
        with self.connection.cursor() as cursor:
            cursor.execute(f"DROP TABLE IF EXISTS {self.quoted_table}")

    def test_execute_sql_sync_preserves_chinese_identifiers(self) -> None:
        sql = (
            f"SELECT {', '.join(self.quoted_columns)} "
            f"FROM {self.quoted_table} ORDER BY {self.quoted_columns[0]}"
        )
        result = execute_sql_sync(self.cfg, sql, None, 10, False)
        self.assertEqual(
            result,
            {
                "ok": True,
                "columns": CHINESE_COLUMNS,
                "rows": [
                    dict(zip(CHINESE_COLUMNS, CHINESE_ROWS[0])),
                    dict(zip(CHINESE_COLUMNS, CHINESE_ROWS[1])),
                ],
                "row_count": 2,
                "truncated": False,
            },
        )

    def test_schema_helpers_preserve_chinese_column_names(self) -> None:
        described = describe_table_sync(self.cfg, self.table_name)
        compact = get_table_schema_compact_sync(self.cfg, self.table_name)
        self.assertTrue(described["ok"])
        self.assertEqual([item["name"] for item in described["columns"]], CHINESE_COLUMNS)
        self.assertTrue(compact["ok"])
        self.assertEqual([item["name"] for item in compact["columns"]], CHINESE_COLUMNS)

    def test_csv_export_preserves_chinese_headers_and_values(self) -> None:
        export_name = f"unicode_regression_{uuid4().hex}.csv"
        sql = (
            f"SELECT {', '.join(self.quoted_columns[:3])} "
            f"FROM {self.quoted_table} ORDER BY {self.quoted_columns[0]}"
        )
        result = export_sql_to_file_sync(
            self.cfg,
            sql,
            None,
            target=None,
            path=export_name,
            export_format="csv",
            sheet_name=None,
            overwrite=True,
        )
        export_path = get_db_export_config().root / export_name
        self.addCleanup(export_path.unlink, missing_ok=True)
        self.assertTrue(result["ok"])
        self.assertEqual(result["columns"], CHINESE_COLUMNS[:3])
        self.assertIn(CHINESE_COLUMNS[0], export_path.read_text(encoding="utf-8-sig"))
        self.assertIn(CHINESE_ROWS[0][1], export_path.read_text(encoding="utf-8-sig"))

    def test_bound_table_validation_accepts_unquoted_chinese_identifier(self) -> None:
        error = validate_sql_against_target_table(
            "SELECT COUNT(*) AS total_count FROM 人员信息",
            self.cfg,
            "人员信息",
        )
        self.assertIsNone(error)

    def test_bound_table_validation_error_includes_mysql_quote_hint(self) -> None:
        error = validate_sql_against_target_table(
            "SELECT COUNT(*) AS total_count",
            self.cfg,
            "人员信息",
        )
        self.assertIsNotNone(error)
        self.assertIn("SQL must include FROM/JOIN on bound table '人员信息'.", error)
        self.assertIn("`人员信息`", error)


if __name__ == "__main__":
    unittest.main()
