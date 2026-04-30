import unittest

from extra_mcp.tools.database.db import normalize_sql_punctuation
from extra_mcp.tools.database.exporter import resolve_sql_request


class ExtraMcpExportRequestTests(unittest.TestCase):
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


if __name__ == "__main__":
    unittest.main()
