# 内嵌字体

桌面版的界面文字不依赖目标机器装了什么字体。这两款字体在构建时以 LZ4 压缩进可执行
文件，启动时先解压再注册一次，之后 Slint 的所有排版都走它们：

| 文件 | 用途 | 未压缩 | 许可 |
| --- | --- | --- | --- |
| `MiSans-Regular.ttf` | 正文字体（中英文、数字、标点） | 8.12 MB | MiSans 免费商用，见 `LICENSE-MiSans.txt` |
| `DejaVuSans.ttf` | 仅作符号回退：MiSans 缺 `⚠ ⌘ ☑ ☐ ☁ ⊞ ⊟ ✦ ◉ ❖` | 0.76 MB | Bitstream Vera / DejaVu，见 `LICENSE-DejaVu.txt` |

注册顺序即优先级：`MiSans` 是泛族的第一位，DejaVu Sans 排在它后面，所以**两款都覆盖的
字符一律由 MiSans 出字形**，DejaVu 只在 MiSans 无字形时补位，不会抢走拉丁字母或中文。

## 为什么是这两款

- **行高比**：MiSans 的自然行高约 1.33em，与原先内嵌的微软雅黑 1.36em 只差 2%，所以
  `design_tokens.slint` 的 `prose-line: 1.25`、`markdown-rich.slint` 的 `lead-half`
  都不用重调；换成思源黑体（1.45em）就得连带调整正文行距。
- **体积**：原先内嵌的 `msyh.ttc` 未压缩 19.65 MB，现在 8.88 MB，且雅黑本身不可随软件
  再分发，换成免费商用字体同时解决合规问题。
- **观感一致**：MiSans 就是小米手机上的默认正文字体，桌面版与安卓版在同一台小米设备上
  的中文观感因此统一。

## 维护约束

- 两款字体的许可都**禁止修改字体本体**，所以不要对这两个文件做子集化、改名或转格式；
  需要更小体积时，正确做法是换一款允许裁剪的字体（如 OFL 的思源黑体/Noto Sans SC）。
- MiSans 许可要求「在软件中特别注明使用了 MiSans 字体」，该声明写在
  `docs/使用说明书/zh-CN/forms/desktop-ui.md`（会生成进内置说明书）。
- 升级字体时同步更新本表里的体积与许可文件，并回归 `cargo test`（字体注册有测试覆盖）。
