---
id: task_radar_theory_basics
name: 天气雷达理论：基础知识
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
  - theory
  - multiple-choice
languages:
  - zh-CN
workspace_files:
  - path: input/questions.json
    content: |
      {
        "questions": [
          {"id":"B01","question":"雷达产品中 dBZ 通常表示什么？","options":{"A":"反射率因子的对数量级","B":"雷达天线高度","C":"扫描转速","D":"降水持续时间"}},
          {"id":"B02","question":"径向速度最直接表示什么？","options":{"A":"完整水平风速","B":"沿雷达波束方向的速度分量","C":"回波顶高","D":"降水粒子直径"}},
          {"id":"B03","question":"PPI 扫描最恰当的描述是？","options":{"A":"固定仰角的平面位置显示","B":"固定方位的垂直剖面","C":"多个仰角体扫","D":"设备自检模式"}},
          {"id":"B04","question":"VOL 扫描的主要用途是？","options":{"A":"只测单一方位","B":"获取多个仰角的体扫结构","C":"关闭发射机","D":"替代设备维护"}},
          {"id":"B05","question":"对谱宽增大的正确理解是？","options":{"A":"必然发生冰雹","B":"可能反映速度离散增大，需结合其他资料判断","C":"等于径向速度","D":"表示雷达已故障"}}
        ]
      }
---

## Prompt

请使用已挂载的“天气雷达理论技能”完成选择题。先读取 `{attempt_root}/input/questions.json`，再按技能中的培训材料核对答案。

只在 `{attempt_root}/output/answers.json` 写入结果，格式必须为：

```json
{"answers":[{"id":"B01","choice":"A","basis":"雷达基础"}]}
```

5 道题都必须作答，`choice` 只能使用 A、B、C、D；`basis` 必须是对应培训要点章节名称。不要在工作区外写文件。

## Expected Behavior

智能体读取题目和理论技能培训材料，完成 5 道雷达基础单选题并生成合法 JSON。题目考查基本物理量及其解释边界，不考查未提供的具体型号参数。

## Grading Criteria

- [ ] 生成合法 answers.json
- [ ] 包含全部 5 道题
- [ ] 5 个选项正确或得到对应比例分
- [ ] 每题包含雷达基础依据

## Automated Checks

```python
def grade(transcript, workspace_path):
    import json, os
    scores = {"file_exists": 0.0, "json_valid": 0.0, "all_questions": 0.0, "choice_accuracy": 0.0, "basis_present": 0.0}
    path = os.path.join(workspace_path, "output", "answers.json")
    if not os.path.exists(path): return scores
    scores["file_exists"] = 1.0
    try:
        with open(path, encoding="utf-8") as fp: payload = json.load(fp)
    except Exception: return scores
    scores["json_valid"] = 1.0
    answers = {str(x.get("id")): x for x in payload.get("answers", []) if isinstance(x, dict)}
    expected = {"B01":"A", "B02":"B", "B03":"A", "B04":"B", "B05":"B"}
    if set(answers) == set(expected): scores["all_questions"] = 1.0
    scores["choice_accuracy"] = sum(answers.get(k, {}).get("choice") == v for k, v in expected.items()) / len(expected)
    if set(answers) == set(expected) and all("雷达基础" in str(answers[k].get("basis", "")) for k in expected): scores["basis_present"] = 1.0
    return scores
```
