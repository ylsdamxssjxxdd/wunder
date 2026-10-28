# 蛹与舵机多进程共存

日期：2026-10-27。蛹是本地 AI 工作台；舵机是手工本地操作入口。蛹窗口仍保持原单实例策略，舵机进程没有全局实例数限制；可同时打开多个舵机，默认各自新建任务线程。实际可用数量受操作系统资源限制，不承诺无限容量。

## 根因与修复

- 旧实现给整个 canonical 数据根持有进程生命周期的排他文件锁；蛹启动后，独立舵机在初始化存储时即被拒绝，与线程是否相同无关。
- 数据根改为内核共享锁；初始化、索引迁移使用独立的短时排他锁，等待上限十秒。保留旧根锁文件，不删除锁标记，旧版排他进程存在时仍安全拒绝。
- 线程级锁保持跨进程排他。不同任务线程可并行，同一任务线程的打开租约和执行锁不放宽，不能通过多开绕过线程所有权。
- 后台 rollout 写入器持有线程锁直到排空结束，避免前台句柄释放后新进程抢到锁、与旧写入器重叠。
- 输入区入口改为“打开舵机”：优先解析安装目录的 `wunder-cli`，传递工作区、用户身份和 `WUNDER_HOME`，不复用蛹当前任务线程。Windows 直接创建独立控制台，Linux 通过系统终端承载 TUI；不拼接 shell 命令，不启动已移除的模拟终端。

实现：`crates/wunder-runtime/src/storage/thread_store.rs`、`crates/wunder-runtime/src/storage/rollout.rs`、`crates/wunder-desktop/native_cli.rs`。

## 状态模型

`LocalEntries.tla` 与 `LocalEntriesFixed.tla` 使用相同配置：一个蛹、两个舵机、两个线程，分别枚举打开、线程领取、句柄释放、后台排空与进程退出。原模型违反 `IndependentEntries`：打开蛹后，再开舵机会被数据根独占拒绝；修复模型同规则通过。`DrainKeepsOwnership` 保持成立。

这是有限人工抽象，不是整个 Rust 程序的定理证明，也没有证明任意实例数、全部 SQLite 交错、历史变更协议或外部工具副作用。原模型已假定线程锁覆盖后台排空；该假设在实现中通过让写入器持有线程锁显式落实，不能据此宣称旧实现已验证排空安全。初始化竞争和实际锁行为由真实多进程回归补充。

编号路径见 `bug-1.png`、`bug-1.mmd`、`stories.md`；总图为 `chart.png`、`chart.mmd`。`summary.json` 为去掉机器路径及图形裁剪长列表的摘要，完整报告由重跑生成。`--cover` 只影响画图，不裁剪 TLC 检查。

重跑时将 `.tla`、`.cfg`、`.mjs` 复制到仓库外 scratch，配置本机已核验的 `JAVA`、`TLA2TOOLS`、`STATECHART_SCRIPT` 后运行：

```powershell
node $env:STATECHART_SCRIPT from-spec LocalEntries.tla --fixed LocalEntriesFixed.tla --map LocalEntries.map.mjs --expect-broken IndependentEntries --out report --cover --png
```

## 真实回归

- `multiple_local_processes_share_root_without_sharing_threads`：主进程持有数据根，同时启动三个真实子进程，共根创建独立线程、写入并读取冻结提示词和历史位置。
- `another_process_cannot_write_active_thread_but_can_take_over_after_shutdown`：另一进程能打开同一数据根，但不能写入已占用线程；原写入器关闭后可接管，历史序号连续、冻结提示词不变。
- `local_process_exit_releases_thread_without_removing_persistent_lock_markers`：强制退出实际子进程后，内核释放线程所有权；新写入继续原序号，锁文件不删除。
- 原有排他根锁拒绝测试保留，确保不会绕过仍运行的旧版进程。
- 原生启动器测试验证安装目录解析、组件缺失、参数及环境保留，不通过 shell 解释工作区名称。
- Windows 控制台探针通过生产 `spawn_cli` 连续拉起两个独立子进程，检查三路标准输入输出均为终端、工作目录和数据根被保留；探针不用真实模型，也不把它当成完整舵机交互验收。
- 探针曾复现 `Command + CREATE_NEW_CONSOLE` 仍继承父进程管道、使舵机判成非 TTY 的问题；Windows 改用不继承句柄且不设置 `STARTF_USESTDHANDLES` 的 `CreateProcessW`，由新控制台分配标准句柄。参数按 Windows 规则转义，环境块显式继承并覆盖数据根与 PATH。
- Slint 1.18.0 viewer 检查真实主窗口，通过仓库外副本仅强制展开菜单渲染，目视确认“打开舵机”及图标；不把静态渲染当点击端到端验证。

```powershell
cargo test -p wunder-runtime --features sqlite-storage --lib storage::thread_store -- --test-threads=1
cargo test -p wunder-desktop --no-default-features --features sqlite-storage --lib native::cli -- --test-threads=1
```

本轮实际结果：线程存储定向 29 项通过；最终同一 runtime 测试二进制的 storage 子树 164 项通过、1 项忽略；原生启动器 5 项通过，含 Windows 真实控制台探针。舵机和 Slint 前端 `cargo check` 通过，主窗口 `slint-viewer --check` 通过。旧模型按预期违反入口独立性，修复模型三条配置不变量通过且无未溯源 guard；统计以本目录摘要为准。

本轮运行平台为 Windows 开发环境；Linux 终端启动与 Win7 32 位、Ubuntu 18.04 仍需目标环境验收。部署时须同时更新蛹与舵机并退出旧进程，旧版长期排他根锁不能被新进程安全绕过。
