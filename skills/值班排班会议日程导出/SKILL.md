---
name: 值班排班会议日程导出
description: "生成值班/排班/会议日程表，导出 CSV + Markdown + ICS 日历文件。"
---

# 值班/排班/会议日程表生成与导出

## 概要
将排班与会议安排整理为统一事件清单，支持导出 CSV、Markdown 和可导入日历的 ICS 文件。

## 快速流程
1. 按规范准备 `schedule_input.json`。
2. 运行脚本生成 `schedule.csv`、`schedule_summary.md`、`schedule.ics`。
3. 导入日历或继续二次加工。

## 输入 JSON 结构
```json
{
  "title": "值班与会议日程",
  "timezone": "Asia/Shanghai",
  "range": {"start": "2026-02-01", "end": "2026-02-07"},
  "shifts": [
    {"date": "2026-02-01", "start": "09:00", "end": "18:00", "role": "日班", "person": "王一", "location": "A机房", "notes": "主班"}
  ],
  "meetings": [
    {"date": "2026-02-03", "start": "10:00", "end": "11:00", "title": "周会", "organizer": "张三", "attendees": ["王一", "李四"], "location": "会议室 1"}
  ]
}
```

## 输出说明
- `schedule.csv`：统一事件表（值班与会议合并）。
- `schedule_summary.md`：人类可读的排班与会议列表。
- `schedule.ics`：可导入日历的标准文件。

## 脚本用法
```bash
python scripts/schedule_builder.py \
  --input examples/schedule_input.json \
  --output-dir examples
```

## 注意事项
- 时间格式统一为 `HH:MM`，日期为 `YYYY-MM-DD`。
- ICS 使用 `TZID=Asia/Shanghai`，如需其他时区请在输入中替换。
