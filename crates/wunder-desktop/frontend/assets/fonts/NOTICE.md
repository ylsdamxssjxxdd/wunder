# 内嵌字体

桌面版的界面文字不依赖目标机器装了什么字体。这三款字体在构建时以 LZ4 压缩进可执行
文件，启动时先解压再注册一次，之后 Slint 的所有排版都走它们：

| 文件 | 用途 | 未压缩 | 许可 |
| --- | --- | --- | --- |
| `MiSans-Regular.ttf` | 正文字体（中英文、数字、标点） | 8.12 MB | MiSans 免费商用，见 `LICENSE-MiSans.txt` |
| `DejaVuSans.ttf` | 符号回退：MiSans 缺 `⚠ ⌘ ☑ ☐ ☁ ⊞ ⊟ ✦ ◉ ❖` | 0.76 MB | Bitstream Vera / DejaVu，见 `LICENSE-DejaVu.txt` |
| `NotoEmoji-Regular.ttf` | emoji 回退（单色轮廓） | 0.87 MB | SIL OFL 1.1，见 `LICENSE-NotoEmoji.txt` |

注册顺序即优先级：`MiSans` 是泛族的第一位，DejaVu Sans 与 Noto Emoji 依次排后，所以
**前面那款覆盖的字符一律由它出字形**，后面的只在缺字形时补位，不会抢走拉丁字母或中文。
三款都挂在 `SansSerif` / `SystemUi` / `UiSansSerif` 泛族上，Noto Emoji 另外单独挂到
`Emoji` 泛族。

真正让符号和 emoji 不空白的，是**按脚本登记的回落表**：界面文字的 `font-family` 写的是
`MiSans`，Slint 在缺字时不会去走泛族列表，而是按字符所属脚本向字体集合要回落族——符号和
emoji 都属于 Common（`Zyyy`）脚本，所以 DejaVu Sans 与 Noto Emoji 必须登记在
`Zyyy` 的回落表里；中文（`Hani`）仍回落 MiSans。桌面版的字体集合**不枚举宿主字体**
（`create_collection(system_fonts: false)`），漏了这一步就会在目标机上直接空白。

## emoji 为什么是单色

Slint 的 software renderer 只缓存轮廓字形，取不到 CBDT 彩色位图（Noto Color Emoji 就是
后者），所以内嵌彩色 emoji 字体只会得到空白。单色 Noto Emoji 是轮廓字体，能正常栅格化，
代价是没有颜色。想上彩色 emoji 得等渲染后端支持 COLR/CBDT，不是换字体能解决的。

## 为什么是这两款

- **行高比**：MiSans 的自然行高约 1.33em，与原先内嵌的微软雅黑 1.36em 只差 2%，所以
  `design_tokens.slint` 的 `prose-line: 1.25`、`markdown-rich.slint` 的 `lead-half`
  都不用重调；换成思源黑体（1.45em）就得连带调整正文行距。
- **体积**：原先内嵌的 `msyh.ttc` 未压缩 19.65 MB，现在 9.75 MB（MiSans + DejaVu +
  Noto Emoji），且雅黑本身不可随软件再分发，换成免费商用字体同时解决合规问题。
- **观感一致**：MiSans 就是小米手机上的默认正文字体，桌面版与安卓版在同一台小米设备上
  的中文观感因此统一。

## 维护约束

- MiSans 与 DejaVu 的许可都**禁止修改字体本体**（DejaVu 允许改名后的修改，MiSans 完全禁止），
  所以不要对这两个文件做子集化或转格式；需要更小体积时，正确做法是换一款允许裁剪的字体
  （如 OFL 的思源黑体/Noto Sans SC）。Noto Emoji 是 OFL，可以自由裁剪，但当前体积已经够小。
- MiSans 许可要求「在软件中特别注明使用了 MiSans 字体」，该声明写在
  `docs/使用说明书/zh-CN/forms/desktop-ui.md`（会生成进内置说明书）。
- 升级字体时同步更新本表里的体积与许可文件，并回归 `cargo test`（字体注册有测试覆盖）。
