# wunder-cli 真实可用性与自动化测试方案

## 1. 2026-03-08 真实实测摘要

- 实测模型：`qwen3.5-122b`
- 运行方式：使用独立 `temp_root`，将项目内现有 `data/config/wunder.override.yaml` 复制到测试根目录，避免污染日常 CLI 状态。
- 长任务内容：让 `wunder-cli` 在隔离工作区内从零生成一个 Rust CLI `diff-lens`，要求支持 unified diff 统计、`--json` 输出、README 与单元测试，并在任务内自行运行 `cargo test`。
- 本轮结果：模型可以稳定完成多步任务流，实际调用了 `执行命令 / 写入文件 / 读取文件 / 应用补丁 / 列出文件` 等工具，最终产物可独立通过本地复核：
  - `cargo test`
  - `cargo build --release`
  - 产出二进制可正确处理 sample diff 的表格输出与 JSON 输出

## 2. 本轮暴露出的稳定性问题

- `execute_command` 在 Windows PowerShell 下不能直接使用 `&&`，模型首轮命令会直接报错，后续需要显式改为 `;` 或分行执行。
- `cargo test 2>&1` 与 `cargo build --release 2>&1` 会把 stderr 合流为 PowerShell 错误流，导致 transcript 中出现“工具失败 / exit=1”假象，即使实际测试和构建已经成功。
- 长任务最后可能停在最后一次工具调用之后，没有补出自然语言收尾总结；这会降低用户对“任务是否真的结束”的确定感。
- 内置工具结果在 transcript 中仍存在直接显示 JSON 的情况，例如 `写入文件 / 读取文件 / 列出文件`，离 codex 的“标题 + 紧凑摘要 + 少量 preview”还有差距。
- `列出文件` 默认把 `.git/` 目录也带出来，真实编码场景里会放大噪音，影响模型二次检索效率。

## 3. 建议的 CLI 回归测试分层

### 3.1 L0：静态与构建层

每次改动 wunder-cli 之后至少执行：

- `cargo check --bin wunder-cli`
- `cargo test --bin wunder-cli`
- `cargo clippy --bin wunder-cli -- -D warnings`
- `cargo build --release --bin wunder-cli`

### 3.2 L1：渲染与文本格式层

重点覆盖：

- 输入区换行、光标、历史回溯
- transcript 中 patch / command / generic tool 的紧凑展示
- 中英文文案与 UTF-8 安全截断
- approval / question panel / status line 的文本快照

这层优先保持“快”，适合作为开发中的高频回归。

### 3.3 L2：真实模型连通性探针

目标：快速确认“当前 release 二进制 + 当前模型配置 + 当前网络”是通的。

建议命令：

- `python scripts/wunder_cli_e2e_smoke.py --model qwen3.5-122b --skip-long-task`

通过标准：

- probe 成功返回
- 生成 `summary.json`
- 不污染默认 `WUNDER_TEMP`

### 3.4 L3：长任务端到端验证

目标：验证真实多步任务下的稳定性，而不是只看一句问答。

默认长任务选择“生成一个可编译、可测试的小 Rust CLI”，因为它会同时覆盖：

- 工具规划
- 多文件写入
- 补丁修复
- 读取校验
- 命令执行
- 最终产物复核

建议命令：

- `python scripts/wunder_cli_e2e_smoke.py --model qwen3.5-122b --build-release`

通过标准：

- probe 成功
- 长任务执行完成
- 自动定位生成项目
- 自动复核 `cargo test` 与 `cargo build --release`
- 自动执行 sample diff 的 table/json 验证
- 输出 `observations` 供人工快速判断 transcript 质量

## 4. 新增自动化脚本

新增脚本：`scripts/wunder_cli_e2e_smoke.py`

能力：

- 可选自动构建 `target/release/wunder-cli`
- 自动复制现有模型配置到独立 `temp_root`
- 先跑 probe，再跑长任务
- 自动保存完整日志到 `temp_dir/cli-e2e/runs/<timestamp>/logs/`
- 自动发现长任务生成的项目根目录
- 自动复核 `cargo test` / `cargo build --release`
- 自动跑 sample diff table/json 校验
- 输出统一 `summary.json`

默认长任务提示词文件：`scripts/prompts/wunder_cli_long_task_diff_lens.txt`

## 5. 推荐日常使用方式

### 5.1 快速冒烟

```bash
python scripts/wunder_cli_e2e_smoke.py --model qwen3.5-122b --skip-long-task
```

### 5.2 完整回归

```bash
python scripts/wunder_cli_e2e_smoke.py --model qwen3.5-122b --build-release
```

### 5.3 重新开始一轮干净测试

```bash
python scripts/wunder_cli_e2e_smoke.py --model qwen3.5-122b --build-release --clean
```

## 6. 下一步最值得继续优化的点

- 把 generic tool transcript 从 JSON 摘要改成结构化卡片摘要。
- 在 `execute_command` 的 Windows 包装层中，把 `&&` 自动转换为 PowerShell 友好的链式执行策略，或直接给模型更强约束。
- 为“命令 stderr 但 exit=0”场景建立更可靠的成功判定，减少模型误修复。
- 对长任务增加“最后收尾答复”保障，避免停在最后一次工具调用后直接结束。
- 给 `list_files` / `search_content` 增加默认 ignore 规则，优先屏蔽 `.git/target/node_modules` 等噪音目录。

## 7. 2026-03-08 第二轮优化落地

- 已落地三项高优先级优化：
  - Windows 下 `execute_command` 遇到未加引号的 `&&` / `||` / `2>&1` 时自动改走 `cmd.exe`，避免 PowerShell 5 语法不兼容与 stderr 合流误判。
  - `list_files / search_content / read_files / write_file / read_image / skill_call / lsp_query / ptc` 的 transcript 改为结构化摘要与 preview，不再优先直出 raw JSON。
  - 当一轮任务做完工具调用但 `final.answer` 为空、最后可见事件仍是工具输出时，CLI/TUI 会补一条自然语言收尾提示，降低“是否真的结束”的不确定感。
- 工具层新增默认噪音过滤：`list_files` 与 `search_content` 会默认跳过 `.git/target/node_modules/.next/.nuxt/.turbo/.cache` 等目录，更接近 codex 的真实检索体验。

## 8. 本轮验证与阻塞

- 代码侧验证已经通过：
  - `cargo check --bin wunder-cli`
  - `cargo test --bin wunder-cli`
  - `cargo clippy --bin wunder-cli -- -D warnings`
  - `cargo build --release --bin wunder-cli`
- 自动化 E2E 在本轮重试时被上游模型计费状态阻断，不属于 CLI 代码回归：
  - `temp_dir/cli-e2e/runs/20260308-201846/summary.json`
  - `temp_dir/cli-e2e/runs/20260308-201846/logs/probe.log`
- 当前日志显示的错误是阿里云百炼返回 `Arrearage / Access denied`；待模型账户恢复后，可直接复跑：
  - `python scripts/wunder_cli_e2e_smoke.py --model qwen3.5-122b`

## 9. 鼠标交互专项回归建议

这类体验很难通过纯命令行 E2E 全覆盖，建议补一组人工专项回归：

- `auto` 模式：
  - 鼠标左键拖选 transcript 文本，确认可直接复制；
  - 滚轮在当前终端中是否可自然滚动；若不可滚动，确认 `/mouse scroll` 可作为兜底；
- `select` 模式：
  - 鼠标拖选复制仍可用；
  - 右键不应再由应用接管粘贴；
- `scroll` 模式：
  - transcript 区域滚轮滚动恢复由应用接管；
  - 原生拖选应被抑制，这是预期行为；
- 模式切换：
  - `F2` 与 `/mouse [auto|scroll|select]` 需实时生效，无需重启会话；
  - status line 中的 `mouse=...` 提示需要同步变化。
