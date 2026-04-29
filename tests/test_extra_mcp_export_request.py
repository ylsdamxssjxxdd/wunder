import unittest

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


if __name__ == "__main__":
    unittest.main()
