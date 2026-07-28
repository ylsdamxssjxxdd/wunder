---
id: task_radar_theory_application
name: 天气雷达理论：产品应用
suite: weather-radar-theory
category: multiple_choice
grading_type: automated
timeout_seconds: 180
runs_recommended: 3
difficulty: easy
required_tools:
  - skill_call
  - read_file
  - write_file
tags:
  - weather-radar
  - product
  - multiple-choice
languages:
  - zh-CN
workspace_files:
  - path: input/questions.json
    content: |
      {
        "questions": [
          {"id":"A01","question":"搜索强对流目标时，合理的做法是？","options":{"A":"只看一个反射率数值","B":"同时关注强度、距离、方位、类型和趋势","C":"只看雷达型号","D":"不需要扫描"}},
          {"id":"A02","question":"当前阈值和量程内没有返回目标时，正确表述是？","options":{"A":"全区域没有天气","B":"当前阈值和量程内未检出目标","C":"必定设备故障","D":"可以发布晴天预报"}},
          {"id":"A03","question":"形成模拟天气建议时，应当如何组织？","options":{"A":"直接发布预警","B":"区分终端事实、建议关注点和资料限制","C":"删除资料限制","D":"只写结论不写依据"}},
          {"id":"A04","question":"连续扫描在目标研判中的主要作用是？","options":{"A":"观察目标随时间的变化","B":"代替设备维修","C":"取消量程设置","D":"保证正式预警正确"}},
          {"id":"A05","question":"模拟终端的目标搜索结果代表什么？","options":{"A":"正式临近预报","B":"当前模拟时刻和条件下的终端目标","C":"真实台站观测","D":"所有区域未来天气"}}
        ]
      }
---

## Prompt

请使用已挂载的“天气雷达理论技能”作答。读取 `{attempt_root}/input/questions.json`，再在 `{attempt_root}/output/answers.json` 中写入 5 道题答案。

每项格式为 `{"id":"A01","choice":"B","basis":"产品应用与研判"}`；不要输出多个选项。

## Expected Behavior

智能体依据培训材料完成雷达产品应用选择题，并给出便于自动评分的结果。题目只评价模拟目标解释和建议边界，不评价真实预报或预警发布能力。

## Grading Criteria

- [ ] 生成合法 JSON
- [ ] 5 道题均有答案
- [ ] 正确选择所有答案
- [ ] 标记产品应用依据

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
    expected={"A01":"B","A02":"B","A03":"B","A04":"A","A05":"B"}
    if set(answers)==set(expected): scores["all_questions"]=1.0
    scores["choice_accuracy"]=sum(answers.get(k,{}).get("choice")==v for k,v in expected.items()) / len(expected)
    if set(answers)==set(expected) and all("产品应用与研判" in str(answers[k].get("basis","")) for k in expected): scores["basis_present"]=1.0
    return scores
```
