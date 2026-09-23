# Qwen/Qwen3.8-27B 提示词预算

本次测量针对 `config/wunder.yaml` 中的 `魔搭` 配置。该模型使用 `function_call`，系统消息不会重复包含 native function schema；schema 会作为请求的 `tools` 字段发送。

精确 Qwen tokenizer 未在本机 Python 环境中加载（`transformers` 不可用）；报告同时记录 UTF-8 字节数、字符数和 `bytes / 4` 近似值。ModelScope 的 `tokenizer_config.json` 可访问，但仅凭配置文件不能宣称精确 token 数。

工具紧凑层位于 `crates/wunder-runtime/src/services/tools/compact.rs`，只作用于模型请求。它保留 `required`、`enum`、类型、边界、默认值和 `additionalProperties`，移除标题、examples 等展示字段，并为高频工具提供短描述。管理端目录和执行逻辑继续使用完整规格。

原始 JSON 数据：`docs/性能基线/assets/2026-09-23-qwen-prompt-budget.json`。

当前模板正文约 1,662 个近似 token；紧凑工具定义约 1,866 个近似 token；两者合计约 3,528，低于 5,000。工作区树、技能详情、记忆索引和 agent 自定义提示属于运行时动态输入，报告未把它们假定为零，生产环境仍应按实际线程快照单独限额。

报告测试：

```text
cargo +1.95.0-x86_64-pc-windows-msvc test -p wunder-runtime --features sqlite-storage,postgres-storage compact --lib --release -j 8 -- --ignored
```

该命令会从仓库根目录配置生成报告；报告中的工具数量取决于当前配置和可用技能/MCP 规格。
