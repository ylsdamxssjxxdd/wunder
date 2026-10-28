# 蛹 `/goal` 控制链验证

日期：2026-10-26。对象是蛹的原生 Slint 前端、`NativeDesktop` façade 与共享引擎，不是舰桥、蜂巢或兼容 bridge 的全量验收。

## 结论

`statechart` 在本例中确实有帮助：先把“创建必须入账、编辑必须生效、暂停返回前旧执行必须结束”变成明确规则，再对照实际入口复现，最后分别验证修复后的模型、真实执行与界面渲染。

这不是“整个系统没有 BUG”的证明。模型是人工构造、与源码对应的有限抽象；只有回归测试和渲染证据覆盖到的边界可以据此验收。

## 已确认的问题与修复

| 编号 | 原始路径与违反的规则 | 实际证据 | 修复 |
| --- | --- | --- | --- |
| 1 | 目标条保存 → `GoalSet` → `Create` → 已存在目标拒绝，但 façade 丢弃拒绝文本；新目标没有保存 | 修复前回归读到原目标，而不是修订目标 | 目标条使用强类型 `GoalEdit`；slash 解析复用引擎语法；保留领域反馈 |
| 2 | `/goal <objective>` → 仅执行领域命令 → 不经过用户轮次准入与完成 | 修复前存储投影为 0 条，预期为 1 条；接通持久化后，完整原生投影又因旧过滤器为 0 条 | 原生入口复用统一控制流程，并删除完整聊天投影对目标轮次的特殊丢弃 |
| 3 | 正在执行目标 → 暂停领域状态 → banner 可变成 paused，但模型调用仍在运行 | 本地受控模型请求进入挂起状态；旧领域路径返回 paused 后，实际执行仍是 running；统一入口暂停后同一执行变为 cancelled，线程可再次提交 | 复用共享控制锁、解除激活、取消并结清旧执行、持久化命令、再变更目标 |
| 4 | blocked + disarmed → 目标条同时显示“已阻塞”和“已暂停” | 真实 `GoalBanner` 渲染前后截图 | 暂停标签只用于 paused，或 active + disarmed |

另修正共享解析器用字节下标切割 `edit` 前缀的 UTF-8 边界问题，覆盖中文、混合文本和 emoji 目标；`/goal pause fixture` 作为完整目标内容，不再被原生解析器误当暂停。

## 模型与编号路径

- `GoalControl.tla` / `GoalControl.cfg`：旧入口行为，`Unified = FALSE`。
- `GoalControlFixed.tla` / `GoalControlFixed.cfg`：统一控制流程，`Unified = TRUE`。
- 源码对应：旧入口行号参照基线 `4df8150`，修复入口和共享流程行号参照本次实现；后续代码移动需更新引用。
- `model/summary.json` 是统计来源：旧模型 9 个状态、6 条迁移；合并对照图 24 个状态、24 条迁移；3 条旧路径违反规则，修复模型中相同 3 条规则全部满足，未标注来源的 guard 列表为空。
- `images/chart.png` 与 `model/chart.mmd` 是总图；`images/bug-1.png` 至 `images/bug-3.png` 和同名 Mermaid 是蓝边编号路径；详细故事见 `model/stories.md`。

每条旧路径均为 **0 初始 → 1 提交命令 → 2 返回错误状态**。修复增加了必要内部步骤，故事中“原第 2 步不能发生”表示不能跳过这些步骤直接返回，不表示修复后的系统无法完成：总图包含解除激活、执行取消、命令准入、命令完成及最终目标变更的完整路径。

模型刻意只描述同一线程内一个有效命令的成功控制路径，不包含模型供应方故障、数据库失败/崩溃恢复、队列容量、多个命令交错、跨进程租约、UI 事件延迟与存活性。`armed` 仅表示续跑准入状态，不把它与持久 phase 或正在运行的 executor 混为一谈。

## 真实测试

回归入口是 `crates/wunder-desktop/tests/goal_facade.rs`，直接启动原生 façade，使用临时 `WUNDER_HOME`、独立 SQLite 和通用 fixture 模型。环境修改由进程内锁串行化；不需要真实上游服务或真实凭据。

- 目标命令持久化、控制轮次结束原因、完整聊天投影重载。
- 暂停后编辑目标，目标内容更新但保持 paused/disarmed。
- 空编辑不创建目标，重复创建如实反馈拒绝，显式编辑和清除生效。
- 真实本地 HTTP 模型请求挂起期间，对比旧领域路径和统一控制入口的暂停行为；检查旧执行 cancelled、线程可提交、没有额外模型请求。

共享引擎的 `goal` 测试过滤集实跑 64 项通过，包括真实目标续跑、直接/排队执行的编辑与暂停交接、恢复、提示词冻结、目标 CAS、旧轮次与清除投影等。原生 idle observer 的独立测试验证没有新聊天帧时目标变化也能到达观察者。

扩展原生 `--lib goal` 测试时有一个已有失败：`native::stream::follow::tests::completed_history_is_drained_before_terminal_and_goal_updates_wake_settlement` 的“active goal retains the completed follower”断言失败。在改动前已构建的旧测试可执行文件中复跑同样失败；本次没有修改该 follower，也不宣称这组扩展测试全绿。

扩展 `native::commands::tests` 过滤集还有一个已有失败：`native_commands_enforce_owner_and_persist_command_turns` 的 current-thread Tokio 测试环境触发同步 workspace 的多线程运行时要求，旧测试可执行文件中同样失败。临时用多线程测试环境排查时，目标段断言已通过，后续 `/compact` 投影断言仍失败；没有为本次目标控制任务修改压缩链路或保留临时测试环境改动。单独的命令解析、可见性、激活判定测试均通过。

## 渲染验收

使用项目版本 Slint 1.18.0 的 `slint-viewer`，不构建完整桌面程序；与项目 `build.rs` 一样开启 `SLINT_ENABLE_EXPERIMENTAL_FEATURES=1`，否则设置页的 `index-of` 会在独立 viewer 中报错。

- `images/goal-preview.png`：修复前真实组件的状态矩阵。
- `images/goal-preview-fixed.png`：修复后 active、paused、blocked、active/disarmed 与编辑态；已目视检查标签、按钮、输入、布局及文本边界。
- `images/main-goal-active.png`：真实主窗口、目标条显示、生成按钮为停止态。
- `images/main-goal-paused.png` / `images/main-goal-blocked.png`：显示相应标签与继续按钮；生成按钮不处于目标运行态。
- `images/main-goal-complete.png` / `images/main-goal-cleared.png`：目标条隐藏。

这些是**真实 Slint 页面 + 注入状态**的静态渲染，不是从键盘输入 `/goal` 到 Rust 回调、观察器、页面刷新的 GUI 自动化；不据此证明所有交互或异步刷新都正确。

## 复现命令

在仓库根目录的 PowerShell 执行测试：

```powershell
$env:RUST_MIN_STACK = '16777216'
cargo test -p wunder-desktop --test goal_facade -- --nocapture --test-threads=1
cargo test -p wunder-desktop --lib native::commands::tests::commands_match_web_and_preserve_objective -- --exact
cargo test -p wunder-desktop --lib native::commands::tests::goal_visibility_distinguishes_complete_and_cleared_from_inactive -- --exact
cargo test -p wunder-desktop --lib native::commands::tests::active_goal_requires_the_process_local_armed_activation -- --exact
cargo test -p wunder-desktop --lib native::observer::tests::goal_service_updates_reach_an_idle_native_observer_without_a_durable_frame -- --exact
cargo test -p wunder-desktop --lib native::chat_turns::tests::goal_command_round_survives_full_chat_projection -- --exact
cargo test -p wunder-runtime --features sqlite-storage --lib goal -- --test-threads=1
```

模型需已安装 statechart skill；也可以用已有 Java 与经验证的 TLC 1.7.4 设置 `JAVA` 和 `TLA2TOOLS`，无需在仓库中保存 Java/JAR。运行输出继续放仓库外：

```powershell
$proof = 'docs/形式化证明/goal-control'
$skill = Join-Path $env:USERPROFILE '.codex/skills/statechart/scripts/statechart.mjs'
$out = Join-Path $env:TEMP ('goal-control-' + [guid]::NewGuid().ToString('N'))
node $skill from-spec "$proof/GoalControl.tla" --fixed "$proof/GoalControlFixed.tla" --expect-broken CreateHasDurableCommand,BannerSaveUpdatesGoal,PauseFencesExecutor --out $out --png
```

重新渲染：

```powershell
$env:SLINT_ENABLE_EXPERIMENTAL_FEATURES = '1'
$proof = 'docs/形式化证明/goal-control'
slint-viewer --check frontend-slint/ui/main.slint
slint-viewer --screenshot "$env:TEMP/goal-preview.png" "$proof/goal_preview.slint"
foreach ($phase in @('active', 'paused', 'blocked', 'complete', 'cleared')) {
    slint-viewer --screenshot "$env:TEMP/main-goal-$phase.png" --size 1100x800 --load-data "$proof/main-goal-$phase.json" frontend-slint/ui/main.slint
}
```

## 尚未证明的部分

会话列表后台快照与实时目标事件之间可能出现覆盖旧投影的竞态；当前仅完成源码定位，没有可执行反例，不列为已确认问题，也没有增加一次性前端补丁。需要后续以可控时序测试覆盖“延迟列表结果、切换线程、清除/重建目标、观察器重连”。本次证据也不替代 Win7 32 位或 Ubuntu 18.04 的运行验证。
