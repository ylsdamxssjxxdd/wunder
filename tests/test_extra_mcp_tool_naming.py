import sys
import types
import unittest


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

from extra_mcp.tools.database.config import DbQueryTarget
from extra_mcp.tools.database.tools import _build_tool_names as build_db_tool_names
from extra_mcp.tools.knowledge.config import KnowledgeTargetConfig
from extra_mcp.tools.knowledge.tools import _build_tool_names as build_kb_tool_names


class ExtraMcpToolNamingTests(unittest.TestCase):
    def test_database_tool_name_prefers_name_and_preserves_unicode(self) -> None:
        targets = [
            DbQueryTarget(
                key="company_all_personnel",
                name="人员信息",
                table="人员信息",
                description=None,
                db_key=None,
            )
        ]
        self.assertEqual(build_db_tool_names("db_query", targets), ["db_query_人员信息"])
        self.assertEqual(build_db_tool_names("db_export", targets), ["db_export_人员信息"])

    def test_database_duplicate_display_names_get_stable_suffix(self) -> None:
        targets = [
            DbQueryTarget("a", "人员信息", "table_a", None, None),
            DbQueryTarget("b", "人员信息", "table_b", None, None),
        ]
        self.assertEqual(
            build_db_tool_names("db_query", targets),
            ["db_query_人员信息", "db_query_人员信息_2"],
        )

    def test_knowledge_tool_name_prefers_name_and_preserves_unicode(self) -> None:
        targets = [
            KnowledgeTargetConfig(
                key="product_docs",
                name="产品文档",
                base_url="http://127.0.0.1:9380",
                api_key="token",
                dataset_ids=["dataset_a"],
                description=None,
                timeout_s=10,
                request={},
            )
        ]
        self.assertEqual(build_kb_tool_names(targets), ["kb_query_产品文档"])


if __name__ == "__main__":
    unittest.main()
