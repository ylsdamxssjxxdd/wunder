---
名称: 电子表格处理
描述: "全面的电子表格创建、编辑与分析，支持公式、格式、数据分析与可视化。当需要处理电子表格（.xlsx、.xlsm、.csv、.tsv 等）时使用：(1) 新建带公式与格式的表格，(2) 读取或分析数据，(3) 在保留公式的情况下修改现有表格，(4) 在表格内进行数据分析与可视化，或 (5) 重新计算公式"
---

# 目标与适用场景
- 目标：用稳定、可复用的流程生成或修改 Excel，并产出清晰的分析与图表
- 适用：报表生成、业务数据分析、财务模型、运营/销售数据看板
- 优先：快速生成可读的模板结构，保证公式可重算与图表可复用

# 输出要求

## 通用要求
### 公式错误为零
- 所有 Excel 模型必须零公式错误（#REF!, #DIV/0!, #VALUE!, #N/A, #NAME?）

### 保留现有模板（更新模板时）
- 修改文件时必须研究并严格匹配现有格式、样式与约定
- 已有固定样式的文件不要强行套用统一格式
- 现有模板约定始终优先于以下指南

### 图表要求（需要可视化时）
- 至少包含 1 个图表，标题、轴标签完整
- 图表数据范围必须覆盖完整数据区
- 图表与数据区保持一致（同表或摘要表）

# 快速流程（推荐）
1. **环境预检**：检查 Python、pandas、openpyxl、LibreOffice（若需要公式重算）
2. **识别任务类型**：新建 / 编辑 / 分析 / 可视化 / 公式重算
3. **准备数据**：数据清洗、字段校验、日期/数值规范化
4. **写入与公式**：优先用公式表达计算关系，避免硬编码结果
5. **可视化**：插入折线/柱状/组合图
6. **公式重算**：使用 `recalc.py`（需 LibreOffice）
7. **质量检查**：错误扫描、行列范围、格式与单位检查

# 环境预检
## 依赖检查（推荐）
```bash
python3 - <<'PY'
import importlib, shutil
mods = ["pandas", "openpyxl"]
for m in mods:
    try:
        importlib.import_module(m)
        print(f"{m} ok")
    except Exception as e:
        print(f"{m} missing: {e}")
print("soffice", "ok" if shutil.which("soffice") else "missing")
PY
```

## 公式重算（可选）
- 需要 LibreOffice（`soffice` 可执行）才可重算公式
- 命令：
  ```bash
  python recalc.py output.xlsx
  ```

# 财务模型规范（保留原约定）
## 颜色编码规范
除非用户或现有模板另有说明。

### 行业通用颜色约定
- **蓝色文本（RGB: 0,0,255）**：硬编码输入值，以及用户会调整的情景变量
- **黑色文本（RGB: 0,0,0）**：所有公式与计算结果
- **绿色文本（RGB: 0,128,0）**：同一工作簿内其他工作表的引用链接
- **红色文本（RGB: 255,0,0）**：外部文件链接
- **黄色背景（RGB: 255,255,0）**：关键假设或需要更新的单元格

## 数字格式规范
### 必须遵循的格式规则
- **年份**：格式化为文本字符串（如 "2024"，不要 "2,024"）
- **货币**：使用 $#,##0 格式；表头必须标注单位（如 "Revenue ($mm)"）
- **零值**：使用格式将所有 0 显示为 "-"，包含百分比（如 "$#,##0;($#,##0);-"）
- **百分比**：默认 0.0% 格式（1 位小数）
- **倍数**：估值倍数使用 0.0x（EV/EBITDA、P/E）
- **负数**：使用括号 (123)，不要 -123

## 公式构建规则
### 假设项放置
- 所有假设（增长率、毛利率、倍数等）放在独立的假设单元格
- 公式中使用单元格引用而非硬编码数值
- 示例：使用 `=B5*(1+$B$6)`，不要 `=B5*1.05`

### 公式错误预防
- 校验所有单元格引用是否正确
- 检查范围是否存在 off-by-one
- 确保所有预测期公式一致
- 用边界情况测试（零值、负值）
- 确认没有意外的循环引用

### 硬编码来源标注要求
- 在单元格内或旁边（若表尾）标注来源。格式："Source: [系统/文档], [日期], [具体引用], [URL 如适用]"
- 示例：
  - "Source: Company 10-K, FY2024, Page 45, Revenue Note, [SEC EDGAR URL]"
  - "Source: Company 10-Q, Q2 2025, Exhibit 99.1, [SEC EDGAR URL]"
  - "Source: Bloomberg Terminal, 8/15/2025, AAPL US Equity"
  - "Source: FactSet, 8/20/2025, Consensus Estimates Screen"

# XLSX 创建、编辑与分析
## 工具选择
1. **pandas**：用于数据分析、统计、摘要输出
2. **openpyxl**：用于公式、格式、Excel 结构与图表

## 关键原则：用公式，不要硬编码结果
必须使用 Excel 公式，而不是在 Python 中计算后写死数值，这样数据变化才能自动重算。

### 错误示例 - 硬编码计算结果
```python
# 错误：在 Python 中计算后写死结果
total = df['Sales'].sum()
sheet['B10'] = total
```

### 正确示例 - 使用 Excel 公式
```python
# 正确：让 Excel 自行计算
sheet['B10'] = '=SUM(B2:B9)'
```

## 编辑现有 Excel 注意事项
- 读取计算值使用 `data_only=True`：`load_workbook('file.xlsx', data_only=True)`
- 警告：以 `data_only=True` 打开并保存会用值覆盖公式且不可恢复
- 大文件读取用 `read_only=True`，写入用 `write_only=True`

# 图表与可视化（新增强化）
## 图表选择建议
- **折线图**：趋势分析（收入、利润、用户）
- **柱状图**：分组对比（部门/区域）
- **组合图**：双指标（收入 + 利润率）

## 图表模板（openpyxl）
```python
from openpyxl.chart import LineChart, Reference

# 图表：收入 vs 利润
chart = LineChart()
chart.title = "收入与利润趋势"
chart.y_axis.title = "金额"
chart.x_axis.title = "月份"

data1 = Reference(ws, min_col=2, max_col=2, min_row=1, max_row=last_row)
data2 = Reference(ws, min_col=3, max_col=3, min_row=1, max_row=last_row)
chart.add_data(data1, titles_from_data=True)
chart.add_data(data2, titles_from_data=True)
chart.set_categories(Reference(ws, min_col=1, min_row=2, max_row=last_row))

ws.add_chart(chart, "E2")
```

# 模板脚本（提高效率）
## 模板 A：新建数据 + 公式 + 图表（推荐）
```python
from datetime import date
from openpyxl import Workbook
from openpyxl.chart import LineChart, Reference
from openpyxl.styles import Alignment, Font

# 1) 准备数据（示例）
months = [date(2024, m, 1) for m in range(1, 13)]
orders = [820, 860, 900, 950, 980, 1020, 1100, 1180, 1120, 1050, 990, 1210]
arpu = [110, 112, 115, 118, 120, 123, 126, 128, 125, 122, 121, 130]
marketing = [42000, 43000, 47000, 44000, 45000, 46000, 48000, 49000, 47000, 45500, 44000, 52000]

wb = Workbook()
ws = wb.active
ws.title = "Data"
ws.append(["Month", "Orders", "ARPU", "Revenue", "Marketing", "Net Profit", "Margin"])

for i in range(12):
    ws.append([months[i], orders[i], arpu[i], None, marketing[i], None, None])

# 2) 公式（禁止硬编码结果）
for r in range(2, 14):
    ws[f"D{r}"] = f"=B{r}*C{r}"
    ws[f"F{r}"] = f"=D{r}-E{r}"
    ws[f"G{r}"] = f"=IFERROR(F{r}/D{r},0)"

# 3) 图表
chart = LineChart()
chart.title = "收入与利润"
chart.y_axis.title = "金额"
chart.x_axis.title = "月份"
chart.add_data(Reference(ws, min_col=4, max_col=4, min_row=1, max_row=13), titles_from_data=True)
chart.add_data(Reference(ws, min_col=6, max_col=6, min_row=1, max_row=13), titles_from_data=True)
chart.set_categories(Reference(ws, min_col=1, min_row=2, max_row=13))
ws.add_chart(chart, "I2")

# 4) 简单格式
ws["A1"].font = Font(bold=True)
ws.freeze_panes = "A2"
for cell in ws["A"]:
    cell.alignment = Alignment(horizontal="center")

wb.save("output.xlsx")
```

## 模板 B：数据分析（pandas）并输出结论
```python
import pandas as pd

df = pd.read_excel("output.xlsx", sheet_name="Data")

summary = {
    "总收入": float(df["Revenue"].sum()),
    "总净利润": float(df["Net Profit"].sum()),
    "平均利润率": float(df["Margin"].mean()),
    "最高收入月份": df.loc[df["Revenue"].idxmax(), "Month"],
}
print(summary)
```

# 公式重算与错误检查
```bash
python recalc.py output.xlsx
```
- 脚本返回 JSON，包含错误明细与公式数量
- 若 `status` 为 `errors_found`，需先修复再交付

# 质量检查清单（交付前）
- [ ] 公式错误为 0
- [ ] 图表已生成，标题与坐标轴完整
- [ ] 单位标注清晰（$ / % / x）
- [ ] 数据范围完整，无 NaN/空行
- [ ] 日期与数值格式符合规范

# 公式校验清单（详细版）
## 关键校验
- [ ] **抽查 2-3 个引用**：确认引用范围正确后再批量应用
- [ ] **列映射**：确认 Excel 列号对应正确（如第 64 列是 BL 不是 BK）
- [ ] **行偏移**：Excel 行号从 1 开始（DataFrame 第 5 行对应 Excel 第 6 行）

## 常见陷阱
- [ ] **NaN 处理**：用 `pd.notna()` 检查空值
- [ ] **超范围引用**：检查数据是否超过预期列数
- [ ] **多次匹配**：查找全部出现位置，不只用第一个
- [ ] **除以零**：公式中 `/` 前先检查分母
- [ ] **错误引用**：确认引用指向目标单元格
- [ ] **跨表引用**：使用正确格式（Sheet1!A1）

## 公式测试策略
- [ ] **先小规模测试**：先在 2-3 个单元格验证公式
- [ ] **验证依赖**：确保公式引用的单元格存在
- [ ] **覆盖边界情况**：包含 0、负数、极大值

## recalc.py 输出解读
```json
{
  "status": "success",
  "total_errors": 0,
  "total_formulas": 42,
  "error_summary": {
    "#REF!": {
      "count": 2,
      "locations": ["Sheet1!B5", "Sheet1!C10"]
    }
  }
}
```

# 最佳实践
## 库选择
- **pandas**：适合数据分析、批量操作与简单导出
- **openpyxl**：适合复杂格式、公式与 Excel 专有特性

## openpyxl 注意事项
- 单元格索引从 1 开始（row=1, column=1 即 A1）
- 读取计算值使用 `data_only=True`
- **警告**：`data_only=True` 打开并保存会用值覆盖公式且不可恢复
- 大文件读取用 `read_only=True`，写入用 `write_only=True`
- 公式会被保存但不会计算，需用 `recalc.py` 更新值

## pandas 注意事项
- 指定类型避免自动推断：`pd.read_excel('file.xlsx', dtype={'id': str})`
- 大文件可只读特定列：`pd.read_excel('file.xlsx', usecols=['A', 'C', 'E'])`
- 正确处理日期：`pd.read_excel('file.xlsx', parse_dates=['date_column'])`

# 代码风格指南
## Python（Excel 相关）
- 代码保持简洁，避免冗余变量与重复操作
- 注释使用中文，并在关键步骤给出清晰说明
- 避免不必要的 print 输出

## Excel 文件本身
- 复杂公式或关键假设应在单元格中添加注释
- 硬编码值标注数据来源
- 为关键计算与模型模块添加说明

# 常见问题与排查
- **图表不显示**：检查数据范围、是否有空列、类别轴设置
- **公式结果为 0**：检查引用列偏移、是否被覆盖为数值
- **pandas 读取空值**：检查列名是否一致、是否有隐藏行/列
