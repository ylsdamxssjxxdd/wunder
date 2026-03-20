---
title: ptc
summary: `ptc` 会把 Python 脚本内容写入工作区临时目录并执行，适合图表、表格、程序化中间产物这类“先生成再运行”的场景。
read_when:
  - 你要理解 `ptc` 和 `执行命令` 的分工
  - 你要生成脚本产物而不是直接跑已有命令
source_docs:
  - src/services/tools/catalog.rs
  - src/services/tools.rs
  - src/sandbox/server.rs
---

# ptc

`ptc` 可以理解成 Wunder 的“程序化工具调用”。

它不是让模型自由拼 shell，而是让模型先给出一段 Python 脚本内容，再由系统把脚本写入临时目录并执行。

## 常用参数

- `filename`
- `workdir`
- `content`

其中：

- `filename` 只接受简单文件名，不允许路径段。
- 没写扩展名时，系统会按 Python 脚本处理。
- 非 Python 扩展会被拒绝。

## 它适合什么

- 生成图表
- 做临时数据清洗
- 输出结构化中间文件
- 先形成脚本再继续执行链路

如果你的目标是“跑一个已经存在的命令”，优先用 [执行命令](/docs/zh-CN/tools/exec/)。

如果你的目标是“先生成一段脚本内容，再把结果产出来”，`ptc` 更合适。

## 它实际怎么落地

系统会把脚本写到工作区下的 `ptc_temp` 一类临时目录中，再调用 Python 执行。

所以它的重点不是编辑仓库源码，而是快速产生一次性的程序化产物。

## 常见误区

### 它不是普通文件编辑

真正要改仓库文件，优先还是：

- [文件与工作区工具](/docs/zh-CN/tools/workspace-files/)
- [应用补丁](/docs/zh-CN/tools/apply-patch/)

### 它也不是普通 shell

`ptc` 的输入核心是脚本内容，不是 shell 命令串。

所以它比 `执行命令` 更适合“生成式脚本”，但不适合把很多系统命令混成一团去跑。

## 实施建议

- `ptc` 面向“生成脚本并执行”，不是面向“执行已有命令”。
- 它只接受受限文件名和 Python 脚本内容。
- 它更适合图表、转换、程序化中间结果，不适合作为仓库编辑主入口。

## 延伸阅读

- [执行命令](/docs/zh-CN/tools/exec/)
- [应用补丁](/docs/zh-CN/tools/apply-patch/)
- [文件与工作区工具](/docs/zh-CN/tools/workspace-files/)
