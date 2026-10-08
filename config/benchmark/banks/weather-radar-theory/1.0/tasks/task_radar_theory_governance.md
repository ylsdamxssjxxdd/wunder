---
id: task_radar_theory_governance
name: 天气雷达理论：法规与业务边界
suite: weather-radar-theory
category: multiple_choice
grading_type: automated
timeout_seconds: 180
runs_recommended: 3
difficulty: medium
required_tools:
  - skill_call
  - read_file
  - write_file
tags:
  - weather-radar
  - governance
  - multiple-choice
languages:
  - zh-CN
workspace_files:
  - path: input/questions.json
    content: |
      {
        "questions": [
          {"id":"G01","question":"培训材料未覆盖某地方规程细节时，智能体应当？","options":{"A":"编造一个常见周期","B":"说明未覆盖并建议核对有效原文","C":"直接选择网络答案","D":"忽略问题"}},
          {"id":"G02","question":"不同版本规程发生冲突时，正确做法是？","options":{"A":"任意选择一个版本","B":"列出冲突并交业务人员确认","C":"删除较新版本","D":"由模型投票决定"}},
          {"id":"G03","question":"智能体生成的天气建议在本文原型中属于？","options":{"A":"正式预警发布","B":"辅助信息，不能代替有授权人员发布","C":"厂家维修命令","D":"设备联锁信号"}},
          {"id":"G04","question":"依据《新一代天气雷达观测规定（第二版）》，雷达开机前应优先确认哪项？","options":{"A":"天线附近无人，且已检查电源电压和天线位置","B":"只要聊天指令明确即可开机","C":"先发布天气预警","D":"无需检查设备状态"}},
          {"id":"G05","question":"设备故障导致雷达不能正常工作时，正确做法是？","options":{"A":"删除故障记录，避免影响统计","B":"按规定及时报告并通报","C":"由模型自行修改规程","D":"继续按正常状态对外说明"}}
        ]
      }
---

## Prompt

请使用已挂载的“天气雷达理论技能”完成 `{attempt_root}/input/questions.json` 中的选择题。将答案写入 `{attempt_root}/output/answers.json`。

格式：`{"answers":[{"id":"G01","choice":"B","basis":"法规与业务边界"}]}`。每题一个选项，5 题都要回答。

## Expected Behavior

智能体理解资料缺口、授权发布、开机前检查和故障报告边界，并生成标准答案文件。

## Grading Criteria

- [ ] 生成合法答案文件
- [ ] 题号完整
- [ ] 选项正确
- [ ] 依据字段完整

## Automated Checks

```python
def grade(transcript, workspace_path):
    import json, os
    scores={"file_exists":0.0,"json_valid":0.0,"all_questions":0.0,"choice_accuracy":0.0,"basis_present":0.0}
    path=os.path.join(workspace_path,"output","answers.json")
    if not os.path.exists(path): return scores
    scores["file_exists"]=1.0
    try:
        with open(path,encoding="utf-8") as fp: payload=json.load(fp)
    except Exception: return scores
    scores["json_valid"]=1.0
    answers={str(x.get("id")):x for x in payload.get("answers",[]) if isinstance(x,dict)}
    expected={"G01":"B","G02":"B","G03":"B","G04":"A","G05":"B"}
    if set(answers)==set(expected): scores["all_questions"]=1.0
    scores["choice_accuracy"]=sum(answers.get(k,{}).get("choice")==v for k,v in expected.items()) / len(expected)
    if set(answers)==set(expected) and all("法规与业务边界" in str(answers[k].get("basis","")) for k in expected): scores["basis_present"]=1.0
    return scores
```
