# 蛹在线更新机制验证

日期：2026-11-01。对象：蛹设置页的在线更新机制——三个更新目标（蛹本体、舵机、补充包）× 两个更新源（云端、GitHub 发布）的手动检查、下载、校验、暂存、应用与启动恢复全链路。范围是 `crates/wunder-desktop/native_update.rs` 的状态机与落盘语义，不覆盖清单服务端实现、网络传输细节与前端控件交互。

## 契约与边界

更新卡片共享一个暂存槽：检查更新按当前选中目标取清单（清单本身按 client/os/arch 参数化，一套服务同时服务三个目标）；下载流式写盘并按清单 SHA256 与大小校验，通过后连同校验结论一起写入暂存元数据；应用时按**暂存包自己的目标**分发，与卡片当前选中无关——蛹本体走"复制到程序目录旁并再次哈希校验 → 旧版改名备份 → 新版就位 → 重启交接"的改名轮换，舵机走同样的复制校验加改名形状、在下次启动时生效，补充包走既有导入管线（校验式流式解压、覆盖合并、热刷新工具环境），全程不触碰任何 exe。启动时的卫生清理会移除轮换残留（`.old`/`.new`），并按恢复闸处理暂存包：舵机与补充包没有编译期基线、只要包文件还在就恢复，蛹本体只在严格新于当前运行版本时恢复，否则清空暂存目录。

模型把真实版本号抽象为整数（`CurrentVersion = 5`、`RemoteVersion = 6`），舵机与补充包的本地检测抽象为"未知、永远可提供"；`Fixed = FALSE` 刻画一个看似合理但省略下载哈希校验结论的捷径，`Fixed = TRUE` 刻画实际代码，两者仅此一个常量不同。舵机轮换抽象为单步（与蛹本体共享"复制校验 + 改名"形状，README 记录该抽象），补充包导入抽象为单步（从不触碰 exe）。崩溃窗口只建模蛹本体轮换中"旧版已改名、新版未就位"这一个最危险时刻。

## 不变量与结果

`models/SelfUpdate.tla` 与 `SelfUpdate.cfg` 是权威源；复验脚本只在仓库外生成仅 `Fixed = TRUE` 不同的对照模型，不删除任何规则。

| 规则 | Fixed = FALSE | Fixed = TRUE | 含义 |
| --- | --- | --- | --- |
| `StagedAlwaysVerified` | 反例（预期，5 步路径） | 通过 | 暂存区里的待应用包一定是下载时通过清单哈希校验的包 |
| `ExeRestorable` | 通过 | 通过 | 程序本体缺失的任何时刻 `.old` 备份必然存在，可手工恢复 |
| `RotationRequiresVerified` | 通过 | 通过 | 改名轮换只有在包落盘并再次校验后才会开始 |
| `OnlyDesktopRotationTouchesExe` | 通过 | 通过 | 只有蛹本体的应用会短暂摘下程序本体；舵机与补充包不动 exe |
| `RestoreNoSameVersion` | 通过 | 通过 | 启动恢复从不把正在运行的同一版本再次端上待应用 |

摘要：141 个模型状态、516 条迁移；总览图（`--cover`）画 70 状态、69 迁移，只裁掉崩溃死端方向的 4 个状态；修复模型下 0 个残留错误状态，`uncited` 为空。12 份 TLC 文本输出全部包含 `Model checking completed`，无异常。TLC 1.7.4 jar SHA256：`936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88`。

这是有界模型检查，不是参数化演绎证明。反例路径的语义是"下载完成但未记录哈希校验结论、未校验的包进入暂存区并被应用"；修复模型把校验结论与下载成功绑定后，同一组规则下该路径不可达。

## 反例

`bug-1`：点击检查更新 → 源上有更新的版本 → 点击下载更新 → 下载完成但跳过清单哈希校验 → 未校验的包进入暂存区。`Fixed = TRUE` 下同一步落在"已校验"状态。真实代码中对应的闸门是下载流的逐块 SHA256 与大小核对，以及轮换复制时的再次校验；两者共用 `copy_and_verify`。蓝色边框为编号路径，绿色框表示修复后不可达。

证据：`chart.png` / `chart.mmd`、`bug-1.png` / `bug-1.mmd`、`stories.md`、`summary.json`；TLC 完整文本输出在 `tlc/` 与 `tlc/fixed/`，其中的机器绝对路径已替换为 `scratch` 标识，未改动状态、反例与结论。

## 模型与代码的对应

| 状态/动作 | 实现 |
| --- | --- |
| `Check` | `native_update.rs:340` 的 `update_check`：进行中拒绝重入（346-347），`normalize_target` 携带选中目标（341、125）。 |
| `OfferNewer` / `OfferNone` / `OfferStaged` | `finish_check`（198）：暂存有效性与"暂存包不被新供给击败"在 205-213；候选闸 214-219（蛹本体按编译期版本，舵机/补充包本地未知则总是候选）；三个去向分别在 220-221、222-231、232-234。 |
| `Download` / `DownloadVerify` / `DownloadFail` | `update_download`（406）：仅 `available` 可下载（409-410）；流式下载的哈希与大小核对在 455-470；暂存元数据与状态落位在 487-506。 |
| `ApplyDispatch` | `update_apply`（514）：`staged`/`failed` 且包在盘上才可应用（517-526，失败态不锁死暂存包），按包自身目标分发到 `apply_staged_by_target`（564-581）。 |
| `RotateStage` / `RotateBackup` / `RotateActivate` | `apply_staged_package`（634）：复制并再次哈希（645），旧版改名备份（647-649），新版就位与回滚（653-656），重启交接由 `RELAUNCH_ENV` 承担。 |
| `ConsumeInPlace` | `apply_staged_cli`（674）：同一复制校验（678）加改名形状的就地替换，运行中的 TUI 持旧 inode。 |
| `ApplyImport` | `apply_staged_supplement`（702）：走既有补充包导入管线，立即生效。 |
| `RestartRestore` / `RestartClear` | `update_cleanup`（585）：清除 `.old`/`.new` 残留（588-595），恢复闸在 603（谓词 `staging_restores` 在 746-750），恢复分支 605-616，清空分支 618-627。 |
| `KillAfterBackup` | 649-656 之间"旧版已改名、新版未就位"的窗口。 |

## 真实回归

```powershell
cargo test -p wunder-desktop --lib update::tests
```

`staged_packages_restore_only_when_the_startup_gate_accepts_them` 锚定恢复闸的三个分支（舵机/补充包总恢复、旧版蛹包被消费、包文件丢失不恢复），对应 `RestartRestore`/`RestartClear` 的分界与 `RestoreNoSameVersion`；`copy_and_verify_accepts_matching_and_rejects_tampered_content` 锚定哈希闸，对应 `StagedAlwaysVerified` 与 `RotationRequiresVerified` 的代码侧语义；`staging_meta_defaults_legacy_packages_to_the_desktop_target` 锚定旧格式暂存元数据的目标归属；`cli_version_probe_parses_clap_output_and_rejects_noise` 与 `supplement_version_marker_reads_only_valid_documents` 锚定两个目标的本地版本检测。

## 已知失败与未覆盖

- 没有未解决的规则违反；预修复反例是预期结果（防回归哨兵）。
- 蛹本体轮换中"旧版已改名、新版未就位"窗口内进程被杀，机器无法自行启动，只能用 `.old` 手工恢复——模型如实保留该死端（总览图裁掉的 4 个状态即此方向），这是改名轮换的固有边界，不是可修复缺陷。
- 舵机轮换被抽象为单步；其复制校验与改名形状与蛹本体一致，但未单独展开崩溃窗口。
- 未建模真实网络故障时序（超时、断流、DNS 失败——抽象为单步失败动作）、并发更新操作（UI 以进行中拒绝重入）、清单服务的鉴权与无仓库快速失败、真实版本号比较的边界（模型用整数）。
- 未覆盖 32 位 Win7 与 Ubuntu 18.04 真机上的改名轮换行为（运行中 exe 的 rename 语义是平台相关的）：上线前需在真机各完成一次三目标的实际更新验证。

## 重跑

Java 与 TLC 准备好后，在仓库外输出；参数为占位符：

```powershell
python scripts/check_self_update_formal.py --statechart <statechart-skill>/scripts/statechart.mjs --out <external-scratch>/self-update
```

脚本复制 `models/` 的三份权威文件，生成仅 `Fixed = TRUE` 不同的修复模型，执行 `from-spec --fixed --expect-broken StagedAlwaysVerified --cover --png`，然后校验 12 份 TLC 输出、`uncited` 与残留错误状态。

```powershell
python -m unittest discover -s tests -p test_formal_proof_layout.py -v
```
