---
名称: xlsx
描述: "全面的电子表格创建、编辑与分析，支持公式、格式、数据分析与可视化。当需要处理电子表格（.xlsx、.xlsm、.csv、.tsv 等）时使用：(1) 新建带公式与格式的表格，(2) 读取或分析数据，(3) 在保留公式的情况下修改现有表格，(4) 在表格内进行数据分析与可视化，或 (5) 重新计算公式"
---

# 输出要求

## 所有 Excel 文件

### 公式错误为零
- 所有 Excel 模型必须零公式错误（#REF!, #DIV/0!, #VALUE!, #N/A, #NAME?）

### 保留现有模板（更新模板时）
- 修改文件时必须研究并严格匹配现有格式、样式与约定
- 已有固定样式的文件不要强行套用统一格式
- 现有模板约定始终优先于以下指南

## 财务模型

### 颜色编码规范
除非用户或现有模板另有说明。

#### 行业通用颜色约定
- **蓝色文本（RGB: 0,0,255）**：硬编码输入值，以及用户会调整的情景变量
- **黑色文本（RGB: 0,0,0）**：所有公式与计算结果
- **绿色文本（RGB: 0,128,0）**：同一工作簿内其他工作表的引用链接
- **红色文本（RGB: 255,0,0）**：外部文件链接
- **黄色背景（RGB: 255,255,0）**：关键假设或需要更新的单元格

### 数字格式规范

#### 必须遵循的格式规则
- **年份**：格式化为文本字符串（如 "2024"，不要 "2,024"）
- **货币**：使用 $#,##0 格式；表头必须标注单位（如 "Revenue ($mm)"）
- **零值**：使用格式将所有 0 显示为 "-"，包含百分比（如 "$#,##0;($#,##0);-"）
- **百分比**：默认 0.0% 格式（1 位小数）
- **倍数**：估值倍数使用 0.0x（EV/EBITDA、P/E）
- **负数**：使用括号 (123)，不要 -123

### 公式构建规则

#### 假设项放置
- 所有假设（增长率、毛利率、倍数等）放在独立的假设单元格
- 公式中使用单元格引用而非硬编码数值
- 示例：使用 `=B5*(1+$B$6)`，不要 `=B5*1.05`

#### 公式错误预防
- 校验所有单元格引用是否正确
- 检查范围是否存在 off-by-one
- 确保所有预测期公式一致
- 用边界情况测试（零值、负值）
- 确认没有意外的循环引用

#### 硬编码来源标注要求
- 在单元格内或旁边（若表尾）标注来源。格式："Source: [系统/文档], [日期], [具体引用], [URL 如适用]"
- 示例：
  - "Source: Company 10-K, FY2024, Page 45, Revenue Note, [SEC EDGAR URL]"
  - "Source: Company 10-Q, Q2 2025, Exhibit 99.1, [SEC EDGAR URL]"
  - "Source: Bloomberg Terminal, 8/15/2025, AAPL US Equity"
  - "Source: FactSet, 8/20/2025, Consensus Estimates Screen"

# XLSX 创建、编辑与分析

## 概述

用户可能会要求创建、编辑或分析 .xlsx 文件内容。针对不同任务有不同工具与流程。

## 重要要求

**公式重新计算需要 LibreOffice**：可假设 LibreOffice 已安装，用 `recalc.py` 脚本重新计算公式值。脚本首次运行会自动配置 LibreOffice。

## 读取与分析数据

### 使用 pandas 进行数据分析
用于数据分析、可视化与基础操作时优先使用 **pandas**：

```python
import pandas as pd

# 读取 Excel
df = pd.read_excel('file.xlsx')  # 默认：第一个工作表
all_sheets = pd.read_excel('file.xlsx', sheet_name=None)  # 读取全部工作表为字典

# 分析
df.head()      # 预览前几行
df.info()      # 查看字段信息与类型
df.describe()  # 统计概览

# 写回 Excel
df.to_excel('output.xlsx', index=False)
```

## Excel 文件工作流

## 关键：使用公式，不要硬编码数值

**必须使用 Excel 公式，而不是在 Python 中计算后写死数值。** 这样才能保证表格可随数据变化自动重算。

### 错误示例 - 硬编码计算值
```python
# 错误：在 Python 中计算后把结果写死
total = df['Sales'].sum()
sheet['B10'] = total  # 这会把 5000 固定在单元格中

# 错误：在 Python 中计算增长率
growth = (df.iloc[-1]['Revenue'] - df.iloc[0]['Revenue']) / df.iloc[0]['Revenue']
sheet['C5'] = growth  # 这会把 0.15 固定在单元格中

# 错误：在 Python 中计算平均值
avg = sum(values) / len(values)
sheet['D20'] = avg  # 这会把 42.5 固定在单元格中
```

### 正确示例 - 使用 Excel 公式
```python
# 正确：让 Excel 自己求和
sheet['B10'] = '=SUM(B2:B9)'

# 正确：增长率使用 Excel 公式
sheet['C5'] = '=(C4-C2)/C2'

# 正确：平均值使用 Excel 函数
sheet['D20'] = '=AVERAGE(D2:D19)'
```

这适用于所有计算：合计、百分比、比率、差值等。表格应能在源数据变化时自动重算。

## 常规流程
1. **选择工具**：数据处理用 pandas，公式/格式用 openpyxl
2. **创建/加载**：新建工作簿或加载现有文件
3. **修改**：添加/编辑数据、公式与格式
4. **保存**：写入文件
5. **重新计算公式（使用了公式则必须）**：使用 recalc.py
   ```bash
   python recalc.py output.xlsx
   ```
6. **验证并修复错误**：
   - 脚本返回 JSON，包含错误详情
   - 若 `status` 为 `errors_found`，查看 `error_summary` 定位问题
   - 修复后重新计算
   - 常见错误：
     - `#REF!`：单元格引用无效
     - `#DIV/0!`：除以零
     - `#VALUE!`：公式数据类型错误
     - `#NAME?`：公式名称无法识别

### 创建新的 Excel 文件

```python
# 使用 openpyxl 处理公式与格式
from openpyxl import Workbook
from openpyxl.styles import Font, PatternFill, Alignment

wb = Workbook()
sheet = wb.active

# 添加数据
sheet['A1'] = 'Hello'
sheet['B1'] = 'World'
sheet.append(['Row', 'of', 'data'])

# 添加公式
sheet['B2'] = '=SUM(A1:A10)'

# 设置格式
sheet['A1'].font = Font(bold=True, color='FF0000')
sheet['A1'].fill = PatternFill('solid', start_color='FFFF00')
sheet['A1'].alignment = Alignment(horizontal='center')

# 设置列宽
sheet.column_dimensions['A'].width = 20

wb.save('output.xlsx')
```

### 编辑现有 Excel 文件

```python
# 使用 openpyxl 保留公式与格式
from openpyxl import load_workbook

# 加载现有文件
wb = load_workbook('existing.xlsx')
sheet = wb.active  # 或 wb['SheetName'] 选择特定工作表

# 遍历多个工作表
for sheet_name in wb.sheetnames:
    sheet = wb[sheet_name]
    print(f"Sheet: {sheet_name}")

# 修改单元格
sheet['A1'] = 'New Value'
sheet.insert_rows(2)  # 在第 2 行插入新行
sheet.delete_cols(3)  # 删除第 3 列

# 新增工作表
new_sheet = wb.create_sheet('NewSheet')
new_sheet['A1'] = 'Data'

wb.save('modified.xlsx')
```

## 重新计算公式

使用 openpyxl 创建或修改的 Excel 文件中，公式以字符串形式保存但不会计算。请使用 `recalc.py` 重新计算：

```bash
python recalc.py <excel_file> [timeout_seconds]
```

示例：
```bash
python recalc.py output.xlsx 30
```

脚本将：
- 首次运行时自动配置 LibreOffice 宏
- 重新计算所有工作表中的公式
- 扫描全部单元格的 Excel 错误（#REF!, #DIV/0! 等）
- 返回包含错误位置与数量的 JSON
- 支持 Linux 与 macOS

## 公式校验清单

快速检查确保公式正确：

### 关键校验
- [ ] **抽查 2-3 个引用**：确认能拉到正确数值后再批量应用
- [ ] **列映射**：确认 Excel 列号对应正确（如第 64 列是 BL 不是 BK）
- [ ] **行偏移**：Excel 行号从 1 开始（DataFrame 第 5 行对应 Excel 第 6 行）

### 常见陷阱
- [ ] **NaN 处理**：用 `pd.notna()` 检查空值
- [ ] **超右列**：财务年度数据常在第 50 列之后
- [ ] **多次匹配**：查找所有出现位置，不要只用第一个
- [ ] **除以零**：公式中 `/` 前先检查分母
- [ ] **错误引用**：确认引用指向目标单元格（#REF!）
- [ ] **跨表引用**：使用正确格式（Sheet1!A1）

### 公式测试策略
- [ ] **先小规模测试**：先在 2-3 个单元格验证公式
- [ ] **验证依赖**：确认公式引用的单元格存在
- [ ] **覆盖边界情况**：包含 0、负数、极大值

### 解读 recalc.py 输出
脚本返回 JSON 示例：
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

## 最佳实践

### 库选择
- **pandas**：适合数据分析、批量操作与简单导出
- **openpyxl**：适合复杂格式、公式与 Excel 专有特性

### 使用 openpyxl 注意事项
- 单元格索引从 1 开始（row=1, column=1 即 A1）
- 读取计算值使用 `data_only=True`：`load_workbook('file.xlsx', data_only=True)`
- **警告**：以 `data_only=True` 打开并保存会用值覆盖公式，且不可恢复
- 大文件读取用 `read_only=True`，写入用 `write_only=True`
- 公式会被保留但不会计算，需要用 recalc.py 更新值

### 使用 pandas 注意事项
- 指定类型避免自动推断：`pd.read_excel('file.xlsx', dtype={'id': str})`
- 大文件可只读特定列：`pd.read_excel('file.xlsx', usecols=['A', 'C', 'E'])`
- 正确处理日期：`pd.read_excel('file.xlsx', parse_dates=['date_column'])`

## 代码风格指南
**重要**：生成 Excel 操作相关 Python 代码时：
- 代码保持简洁，避免冗余变量与重复操作
- 注释使用中文，并在关键步骤给出清晰说明，避免无意义赘述
- 避免不必要的 print 输出

**对 Excel 文件本身**：
- 复杂公式或关键假设应在单元格中添加注释
- 对硬编码值标注数据来源
- 为关键计算与模型区块添加说明
