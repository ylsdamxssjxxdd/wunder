---
name: 招聘JD与岗位画像
description: "生成招聘 JD 与岗位画像模板，输出结构化 Markdown。"
---

# 招聘 JD 生成与岗位画像模板

## 概要
根据岗位信息生成标准化 JD，并同步输出岗位画像（候选人画像）模板，便于招聘统一口径与评估。

## 快速流程
1. 准备岗位信息 JSON。
2. 运行脚本生成 `jd_output.md`。
3. 按实际情况补充薪资区间与团队介绍。

## 输入 JSON 结构
```json
{
  "role_title": "资深后端工程师",
  "department": "平台研发部",
  "location": "上海",
  "employment_type": "全职",
  "reporting_to": "技术负责人",
  "responsibilities": ["设计与实现核心服务", "优化性能与可用性"],
  "requirements": ["5 年以上后端经验", "熟悉 Rust/Go"],
  "nice_to_have": ["云原生经验", "SRE 实践"],
  "skills": ["Rust", "PostgreSQL", "分布式系统"],
  "benefits": ["年度体检", "带薪年假"],
  "persona": {
    "background": "有大型系统建设经验",
    "strengths": ["工程化", "稳定性优化"],
    "motivators": ["技术挑战", "成长空间"],
    "screening_questions": ["分享一次你主导的稳定性提升项目"]
  }
}
```

## 脚本用法
```bash
python scripts/jd_persona_generator.py \
  --input examples/jd_input.json \
  --output examples/jd_output.md
```

## 输出说明
- JD：包含岗位信息、职责、任职要求、加分项、福利。
- 岗位画像：候选人背景、能力标签、动机与筛选问题。
