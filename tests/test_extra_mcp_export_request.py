import unittest

from extra_mcp.tools.database.config import DbConfig
from extra_mcp.tools.database.db import normalize_sql_punctuation, validate_sql_against_target_table
from extra_mcp.tools.database.exporter import resolve_sql_request
from extra_mcp.tools.database.tools import _error_response


class ExtraMcpExportRequestTests(unittest.TestCase):
    def setUp(self) -> None:
        self.cfg = DbConfig(
            engine="mysql",
            host="127.0.0.1",
            port=3306,
            user="user",
            password="",
            database="db",
            connect_timeout=1,
            description=None,
        )

    def test_resolve_sql_request_returns_sql_and_params(self) -> None:
        sql_text, params = resolve_sql_request(
            sql=" SELECT * FROM `people` WHERE `id` = %s ",
            params=[1],
        )
        self.assertEqual(sql_text, "SELECT * FROM `people` WHERE `id` = %s")
        self.assertEqual(params, [1])

    def test_resolve_sql_request_rejects_missing_sql(self) -> None:
        with self.assertRaisesRegex(ValueError, "SQL statement is required"):
            resolve_sql_request(sql="", params=None)

    def test_resolve_sql_request_normalizes_fullwidth_sql_punctuation(self) -> None:
        sql_text, params = resolve_sql_request(
            sql=" SELECT DATE_FORMAT(`born`，'%Y%m%d') AS `日期，标签` FROM `people` ",
            params=None,
        )
        self.assertEqual(
            sql_text,
            "SELECT DATE_FORMAT(`born`,'%Y%m%d') AS `日期，标签` FROM `people`",
        )
        self.assertIsNone(params)

    def test_normalize_sql_punctuation_preserves_string_literals(self) -> None:
        sql_text = normalize_sql_punctuation(
            "SELECT * FROM `people` WHERE `name` = '甲，乙' AND `born` = DATE_FORMAT(NOW()，'%Y%m%d')"
        )
        self.assertEqual(
            sql_text,
            "SELECT * FROM `people` WHERE `name` = '甲，乙' AND `born` = DATE_FORMAT(NOW(),'%Y%m%d')",
        )

    def test_validate_sql_allows_replace_function(self) -> None:
        error = validate_sql_against_target_table(
            "SELECT REPLACE(`born`, '-', '') AS clean_born FROM `people`",
            self.cfg,
            "people",
        )
        self.assertIsNone(error)

    def test_validate_sql_blocks_replace_into(self) -> None:
        error = validate_sql_against_target_table(
            "REPLACE INTO `people` (`id`) VALUES (1)",
            self.cfg,
            "people",
        )
        self.assertEqual(error, "Only SELECT/EXPLAIN/WITH read-only SQL is allowed.")

    def test_database_error_response_adds_identifier_hint_for_sql_syntax(self) -> None:
        result = _error_response(
            RuntimeError(
                "(1064, \"You have an error in your SQL syntax; check the manual near '2026 年 07 月任职月数 FROM `people`'\")"
            )
        )
        self.assertFalse(result["ok"])
        self.assertIn("backticks", result["error"])
        self.assertIn("SELECT *", result["error"])


if __name__ == "__main__":
    unittest.main()
