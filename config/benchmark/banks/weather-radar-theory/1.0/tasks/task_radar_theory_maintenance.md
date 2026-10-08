---
id: task_radar_theory_maintenance
name: 天气雷达理论：设备与维修安全
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
  - maintenance
  - multiple-choice
languages:
  - zh-CN
workspace_files:
  - path: input/questions.json
    content: |
      {
        "questions": [
          {"id":"M01","question":"本原型终端中，从关机到高压状态的正确顺序是？","options":{"A":"高压-低压-冷却","B":"冷却-低压-高压","C":"低压-高压-冷却","D":"直接高压"}},
          {"id":"M02","question":"QX/T 724-2024 对除日维护外的维护人员数量要求是？","options":{"A":"至少 1 人","B":"至少 2 人同时在场","C":"至少 3 人","D":"无需现场人员"}},
          {"id":"M03","question":"发射机开高压运行时，下列做法正确的是？","options":{"A":"打开高压区域防护面板检查","B":"带电拆装相关部件","C":"不得拆除或打开高压区域防护面板，也不得带电拆装","D":"由智能体远程拆机"}},
          {"id":"M04","question":"天线在体扫或俯仰工作时出现异常响声，应如何处置？","options":{"A":"继续运行至扫描结束","B":"立即停机","C":"提高发射机电压","D":"忽略并只记录日志"}},
          {"id":"M05","question":"工具返回非法状态转换错误后，正确动作是？","options":{"A":"重复盲试同一动作","B":"停止试错并报告状态和错误原因","C":"忽略错误继续扫描","D":"修改工具返回值"}}
        ]
      }
---

## Prompt

请使用已挂载的“天气雷达理论技能”完成选择题。读取 `{attempt_root}/input/questions.json` 后，在 `{attempt_root}/output/answers.json` 输出 5 道题的单选答案。

输出格式：

```json
{"answers":[{"id":"M01","choice":"B","basis":"设备与维修安全"}]}
```

每题只能选一个选项。不要编造设备厂家要求；只依据本技能培训要点作答。

## Expected Behavior

智能体按照培训材料完成设备与维修安全选择题，结果可由脚本自动评分。涉及实际高压、拆检或故障处理时，题目只考查安全原则，不授权智能体实施真实操作。

## Grading Criteria

- [ ] 文件和 JSON 合法
- [ ] 全部题号存在
- [ ] 选项正确
- [ ] 依据字段完整

## Automated Checks

```python
def grade(transcript, workspace_path):
    import json, os
    scores = {"file_exists":0.0,"json_valid":0.0,"all_questions":0.0,"choice_accuracy":0.0,"basis_present":0.0}
    path=os.path.join(workspace_path,"output","answers.json")
    if not os.path.exists(path): return scores
    scores["file_exists"]=1.0
    try:
        with open(path,encoding="utf-8") as fp: payload=json.load(fp)
    except Exception: return scores
    scores["json_valid"]=1.0
    answers={str(x.get("id")):x for x in payload.get("answers",[]) if isinstance(x,dict)}
    expected={"M01":"B","M02":"B","M03":"C","M04":"B","M05":"B"}
    if set(answers)==set(expected): scores["all_questions"]=1.0
    scores["choice_accuracy"]=sum(answers.get(k,{}).get("choice")==v for k,v in expected.items()) / len(expected)
    if set(answers)==set(expected) and all("设备与维修安全" in str(answers[k].get("basis","")) for k in expected): scores["basis_present"]=1.0
    return scores
```
