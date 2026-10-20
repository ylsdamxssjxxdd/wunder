# wunder-egui V-01 / V-06 视觉审计

日期：2026-10-19

## 本次完成

- `crates/wunder-egui/src/theme.rs` 继续作为唯一浅色视觉令牌入口。
- 增加并统一 `focus`、`disabled`、`scrim`、`code_background`、`code_inline`、`process`、`success_tint`、`success_border`、`shadow` 和用户气泡渐变令牌。
- `Visuals` 显式覆盖 `hovered`、`active/focus`、`open`、`selection`、禁用透明度、文本光标、代码背景、警告/错误文字、窗口/弹出层阴影和超链接颜色。
- Markdown 行内代码和围栏代码改用主题令牌，避免展示层重复 RGB。
- 新增 2 个主题单元测试，验证交互状态和内容色角色均存在且映射正确。

## 审计结论

### 已覆盖

| 状态 | 统一来源 | 验收说明 |
| --- | --- | --- |
| hover | `Tokens::hover` | 行、菜单和被动按钮保持固定几何，仅改变底色。 |
| selected/open | `Tokens::selected` / `primary_tint` | 选中行、打开菜单和选择文本均有稳定填充。 |
| focus | `Tokens::focus` | egui active/open 描边和文本光标使用强调色，不依赖布局变化。 |
| disabled | `Tokens::disabled` + `Visuals::disabled_alpha` | 不可用控件同时降低透明度和前景对比度。 |
| code | `Tokens::code_background` / `code_inline` | 围栏代码与行内代码背景不再散写。 |
| overlay | `Tokens::scrim` / `shadow` | 模态遮罩和浮层阴影有集中语义。 |

### 后续迁移点

以下值仍位于组件绘制代码中，暂不在本次并行修改中迁移，以避免覆盖侧栏和设置页正在进行的改动：

- `app.rs`：账户头像蓝色、时间线过程灰色、成功 chip、发送按钮 hover 暗色、若干历史/预览遮罩。
- `settings.rs`：设置页卡片、主题预览、配额和渠道状态的专用色板。
- `markdown.rs`：测试渲染 fixture 使用的黑色文字是测试输入，不属于产品主题。

这些值应在对应组件稳定后，按语义迁移到 `Tokens`，并保留固定截图回归；不要把临时 fixture 色值复用为产品状态色。

## 验证

```text
cargo test -p wunder-egui theme::tests --lib
2 passed; 0 failed
```

全仓 `cargo fmt --all -- --check` 当前受仓库已有临时测试模块缺失阻塞，与本次改动无关；全仓构建还受到其他工作区并行改动影响。本次涉及模块的编译和主题测试已通过。
