---
id: task_meeting_notice
name: 会议通知撰写
suite: office-workflow
category: writing
grading_type: llm_judge
timeout_seconds: 180
runs_recommended: 2
difficulty: easy
required_tools:
  - read_file
  - write_file
tags:
  - writing
  - notice
  - office
languages:
  - zh-CN
workspace_files:
  - path: input/agenda.md
    content: |
      # Migration Review Agenda

      - Review delayed migration accounts
      - Confirm owner handoff checklist
      - Align external communication timeline
  - path: input/attendees.txt
    content: |
      Product Team
      Operations Team
      Customer Success Team
  - path: input/constraints.md
    content: |
      - Meeting time: Tuesday 15:00-15:30
      - Tone: professional and concise
      - Include clear preparation request before the meeting
      - Mention that delayed migration account updates should be prepared in advance
---

## Prompt

请根据 `{attempt_root}/input` 下的材料，撰写一份会议通知，输出到 `{attempt_root}/output/notice.md`。

要求：

- 语言简洁、正式
- 明确会议时间、参会对象、议题
- 明确会前准备事项
- 不要写成长文，不要遗漏关键约束

## Expected Behavior

智能体应能从约束和议程中提炼要点，生成一份可以直接发送的会议通知。

## Grading Criteria

- [ ] 时间、参会对象、议题完整
- [ ] 会前准备事项明确
- [ ] 语气专业简洁
- [ ] 结构清晰，适合直接发送

## LLM Judge Rubric

- 是否完整覆盖时间、对象、议题与准备要求
- 是否明确提醒准备 delayed migration account updates
- 是否足够简洁，不拖沓
- 是否达到可直接发送的办公通知质量
