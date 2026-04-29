import json
import os
import tempfile
import unittest
from pathlib import Path

from extra_mcp.tools.database.config import DbQueryTarget
from extra_mcp.tools.database.exporter import (
    QUERY_HANDLE_PREFIX,
    build_query_handle,
    resolve_query_request,
)


class ExtraMcpQueryHandleTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp_dir = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp_dir.cleanup)
        self.original_export_root = os.environ.get("EXTRA_MCP_EXPORT_ROOT")
        os.environ["EXTRA_MCP_EXPORT_ROOT"] = self.temp_dir.name
        self.addCleanup(self._restore_export_root)
        self.target = DbQueryTarget(
            key="personnel",
            name="人员信息",
            table="人员信息",
            description=None,
            db_key="hr",
        )

    def _restore_export_root(self) -> None:
        if self.original_export_root is None:
            os.environ.pop("EXTRA_MCP_EXPORT_ROOT", None)
        else:
            os.environ["EXTRA_MCP_EXPORT_ROOT"] = self.original_export_root

    def test_build_query_handle_uses_short_token_and_persists_payload(self) -> None:
        handle = build_query_handle(
            "SELECT * FROM `人员信息` WHERE `状态` = %s",
            ["正常"],
            self.target,
        )
        self.assertTrue(handle.startswith(QUERY_HANDLE_PREFIX))
        self.assertLess(len(handle), 32)

        sql_text, params = resolve_query_request(
            query_handle=handle,
            sql=None,
            params=None,
            expected_target=self.target,
        )
        self.assertEqual(sql_text, "SELECT * FROM `人员信息` WHERE `状态` = %s")
        self.assertEqual(params, ["正常"])

        store_path = Path(self.temp_dir.name) / ".query_handles" / f"{handle}.json"
        self.assertTrue(store_path.exists())
        payload = json.loads(store_path.read_text(encoding="utf-8"))
        self.assertEqual(payload["table"], "人员信息")
        self.assertEqual(payload["db_key"], "hr")

    def test_resolve_query_request_keeps_legacy_query_handle_compatible(self) -> None:
        legacy_handle = (
            "eyJ2ZXJzaW9uIjoxLCJraW5kIjoiZGJfcXVlcnkiLCJzcWwiOiJTRUxFQ1QgKiBGUk9NIGBwZW9wbGVgIFdIRVJF"
            "IGBpZGAgPSAlcyIsInBhcmFtcyI6WzFdLCJ0YWJsZSI6InBlb3BsZSIsImRiX2tleSI6ImhyIiwiY3JlYXRlZF9h"
            "dCI6IjIwMjYtMDQtMjlUMTI6MDE6NTUifQ"
        )
        target = DbQueryTarget(
            key="people",
            name="people",
            table="people",
            description=None,
            db_key="hr",
        )

        sql_text, params = resolve_query_request(
            query_handle=legacy_handle,
            sql=None,
            params=None,
            expected_target=target,
        )
        self.assertEqual(sql_text, "SELECT * FROM `people` WHERE `id` = %s")
        self.assertEqual(params, [1])


if __name__ == "__main__":
    unittest.main()
