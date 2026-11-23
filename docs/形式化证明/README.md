# 形式化证明

本目录存放 wunder 的形式化模型与验证结论。约定：

- 根目录只放总索引与必做清单；
- 每个主题一个子目录，内含 `models/`（TLA+ 模型与配置）、`README.md`（模型↔代码映射、反例、结果摘要、复验说明），以及可选的反例图；
- 复验脚本放在 `scripts/`，仓库内不保存 TLC 状态、原始日志与完整报告（用 `--out` 指到仓库外，或默认写临时目录）。

## 主题

- [思考流一致性验证](./思考流一致性验证/README.md)：桌面实时投影的思考条目与工具条目结算
- [上下文缓存前缀](./context-cache-prefix/README.md)：压缩 / 图片预算 / 预算裁剪改写已发送前缀导致提供方缓存整段失效

## 必做清单

| 主题 | 模型 | 复验脚本 | 结论 |
| --- | --- | --- | --- |
| 思考流一致性（桌面实时投影） | `思考流一致性验证/models/ThoughtStream.tla`、`ThoughtStreamFixed.tla` | `scripts/check_thought_stream_formal.py` | 缺陷模型复现三条反例；修复模型全状态空间通过 |
| 上下文缓存前缀（提示词缓存冻结） | `context-cache-prefix/models/PrefixStability.tla`、`PrefixStabilityFixed.tla`（脚本生成） | `scripts/check_context_cache_prefix_formal.py` | 缺陷模型在 `AppendOnlyPrefix` 上报反例；修复模型六条不变式全状态空间通过 |

复验：

```bash
python3 scripts/check_thought_stream_formal.py --keep
python3 scripts/check_context_cache_prefix_formal.py \
  --statechart skills/statechart/scripts/statechart.mjs \
  --out /tmp/prefix-stability-report
```

要求：缺陷模型必须在指定不变式上报反例，修复模型必须在 `TypeOK` 与实际不变式上枚举完整个状态空间且无错。脚本对任一偏差以非零码退出。
