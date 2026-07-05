# wunder-cli 输入热路径与 Win7 体积记录

## 目标

- 修复 TUI 普通输入明显卡顿的问题，优先保证输入体验接近 Codex。
- 收拢 CLI 默认运行时临时目录，避免在工作区生成 `WUNDER_TEMP`。
- 复查 Win7 GNU 32 位 CLI 体积来源，并记录本次低风险减重结果。

## 环境

- OS：Windows，本地 PowerShell。
- 工作区：`C:\Users\sjxx\Desktop\wunder`。
- Rust：按仓库 toolchain 和 Win7 GNU 构建脚本执行。
- Win7 目标：`i686-win7-windows-gnu`。
- 构建脚本：`npm run build:cli:win7:gnu:fast`。

## 基线

输入热路径基线为代码审计结果：

- 普通 `KeyCode::Char` 输入会调用粘贴识别逻辑。
- Windows 下粘贴识别会通过 `read_system_clipboard_text()` 启动 PowerShell 执行 `Get-Clipboard -Raw`。
- 快速字符输入还可能进入“疑似非 bracketed paste”缓存路径，导致字符先被缓存而不是立即插入输入框。
- redraw 请求和 draw 通知均使用无界队列，快速输入时可能积压旧帧请求。

Win7 GNU 体积基线：

| 阶段 | 字节 | MiB | SHA256 |
| --- | ---: | ---: | --- |
| strip 后历史产物 | 88,166,912 | 84.08 | 14BE3EFD67E76626DE270B016487D2C736CFBDF4C9E32B8FB5D503ADCC61C4C2 |

## 优化后

输入热路径：

- 普通字符输入只做内存态插入，不再同步读系统剪贴板，不再启动 PowerShell/pbpaste/xclip。
- 粘贴只走 terminal bracketed paste 事件或 Ctrl+V/Shift+Insert 显式路径。
- 关闭普通字符的“疑似粘贴”延迟缓存路径。
- redraw 请求队列和 draw 通知队列改为有界合并，避免输入时 UI 追帧。

运行时目录：

- 默认 `temp_root` 改为 `~/.wunder/cli/WUNDER_TEMP`。
- CLI 初始化时同步设置 `WUNDER_TEMP_DIR_ROOT` 到同一目录。
- `--temp-root` 仍可覆盖默认目录。

Win7 GNU 体积：

| 阶段 | 字节 | MiB | SHA256 |
| --- | ---: | ---: | --- |
| 去 aws-lc / ring provider 后 | 86,410,240 | 82.41 | 5D221C7C9C3E6108303C35972FA4F3D962342231AB213B47235894F8D9B656AE |
| 再移除 Win7 CLI syntect 默认高亮后 | 84,570,112 | 80.65 | D001EF6400425AF9F961336745D00B8E383A1BEE41BEA74DEA9D3B93ABB5A2D3 |

最终 PE section 摘要：

| section | size hex | 约 MiB |
| --- | ---: | ---: |
| `.text` | `042d11e0` | 66.82 |
| `.rdata` | `0068692c` | 6.53 |
| `.eh_fram` | `0073cfe0` | 7.24 |

## 验证

- `cargo check -p wunder-cli --release -j 8`：通过。
- `python scripts/build_docs_site.py`：通过，生成 180 页。
- `npm run build:cli:win7:gnu:fast`：通过，`--help` smoke test 通过。
- `cargo tree -p wunder-cli --target i686-win7-windows-gnu --edges features --invert aws-lc-rs`：无输出，确认未再拉入 `aws-lc-rs`。
- `cargo tree -p wunder-cli --target i686-win7-windows-gnu --edges features --invert syntect`：无输出，确认 Win7 CLI 不再拉入 `syntect`。

## 结论

- 输入卡顿的高概率阻塞点已移除：普通按键路径不再访问系统剪贴板或启动外部进程，redraw 也不再无界排队。
- 本次没有接入自动化交互延迟探针，输入体验提升属于热路径结构性验证，仍建议后续补 TUI key-to-render 延迟脚本。
- Win7 CLI 体积从 88,166,912 bytes 降到 84,570,112 bytes，下降约 3.60 MiB。
- 现有 80 MiB 级体积主要来自完整 runtime 静态链接代码，`.text` 约 66.82 MiB；若目标是 30 MiB 内，需要拆 `cli-lite`/runtime feature 边界，避免把 server API、渠道、管理面、多媒体和完整工具链一起链接进 CLI。

## 回退结论

- 当前改动对普通平台保留 `syntect` 高亮；仅 Win7 CLI 代码块高亮回退为普通 Markdown 代码块渲染。
- 若需要恢复非 bracketed paste 猜测功能，应实现异步剪贴板观察或显式配置开关，不能回到普通按键同步读剪贴板。
