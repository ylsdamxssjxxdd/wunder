# 形式化证明

本目录存放 wunder 的形式化模型与验证结论。约定：

- 根目录只放总索引与必做清单；
- 每个主题一个子目录，内含 `models/`（TLA+ 模型与配置）、`README.md`（模型↔代码映射、反例、结果摘要、复验说明）与反例图；
- 复验脚本放在 `scripts/`，仓库内不保存 TLC 状态、原始日志与完整报告（用 `--out` 指到仓库外，或默认写临时目录）。

## 必做清单

| 主题 | 模型 | 复验脚本 | 结论 |
| --- | --- | --- | --- |
| 思考流一致性（桌面实时投影） | `思考流一致性验证/models/ThoughtStream.tla`、`ThoughtStreamFixed.tla` | `scripts/check_thought_stream_formal.py` | 缺陷模型复现三条反例；修复模型全状态空间通过 |

复验：

```bash
python3 scripts/check_thought_stream_formal.py --keep
```

要求：缺陷模型必须在指定不变式上报反例，修复模型必须在 `TypeOK` 与实际不变式上枚举完整个状态空间且无错。脚本对任一偏差以非零码退出。
