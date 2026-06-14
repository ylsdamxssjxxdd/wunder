use super::spec::{BenchmarkGradingType, BenchmarkTaskSpec};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};

const PROFILE_QUICK: &str = "quick";
const PROFILE_CORE: &str = "core";
const PROFILE_FULL: &str = "full";

#[derive(Debug, Clone)]
pub struct BenchmarkProfileSelection {
    pub profile: String,
    pub tasks: Vec<BenchmarkTaskSpec>,
    pub suite_ids: Vec<String>,
}

pub fn available_profiles(tasks: &[BenchmarkTaskSpec]) -> Vec<Value> {
    let quick_count = select_quick_tasks(tasks).len();
    let core_count = select_core_tasks(tasks).len();
    let full_count = tasks.len();
    vec![
        json!({
            "id": PROFILE_QUICK,
            "name": "Quick Smoke",
            "description": "Fast automated pass for model/tool readiness.",
            "task_count": quick_count,
            "recommended_runs": 1,
            "default": true,
        }),
        json!({
            "id": PROFILE_CORE,
            "name": "Core Capability",
            "description": "Balanced Wunder task coverage with automated and judge scoring.",
            "task_count": core_count,
            "recommended_runs": 2,
            "default": false,
        }),
        json!({
            "id": PROFILE_FULL,
            "name": "Full Suite",
            "description": "All available WunderBench tasks.",
            "task_count": full_count,
            "recommended_runs": 2,
            "default": false,
        }),
    ]
}

pub fn resolve_profile_tasks(
    tasks: Vec<BenchmarkTaskSpec>,
    profile: Option<&str>,
    suite_ids: &[String],
    task_ids: &[String],
) -> BenchmarkProfileSelection {
    let profile = normalize_profile(profile);
    let manual_filter = has_non_empty(suite_ids) || has_non_empty(task_ids);
    let selected = if manual_filter {
        filter_tasks(tasks, suite_ids, task_ids)
    } else {
        match profile.as_str() {
            PROFILE_QUICK => select_quick_tasks(&tasks),
            PROFILE_CORE => select_core_tasks(&tasks),
            PROFILE_FULL => tasks,
            _ => select_quick_tasks(&tasks),
        }
    };
    let suite_ids = collect_suite_ids(&selected);
    BenchmarkProfileSelection {
        profile,
        tasks: selected,
        suite_ids,
    }
}

pub fn build_scorecard(task_aggregates: &[Value], attempts: &[Value]) -> Value {
    let total_score = mean_values(task_aggregates, "mean_score");
    let reliability_score = mean_values(task_aggregates, "pass_rate");
    let tool_success_score = tool_success_rate(attempts);
    let stability_score = stability_score(task_aggregates, attempts);
    let efficiency_score = efficiency_score(attempts);
    let readiness = readiness_label(total_score, reliability_score, tool_success_score);
    let weakest_suites = weakest_suites(task_aggregates);
    let top_failures = top_failures(attempts);

    json!({
        "readiness": readiness,
        "overall_score": round6(total_score),
        "reliability_score": round6(reliability_score),
        "tool_success_score": round6(tool_success_score),
        "stability_score": round6(stability_score),
        "efficiency_score": round6(efficiency_score),
        "weakest_suites": weakest_suites,
        "top_failures": top_failures,
    })
}

fn normalize_profile(profile: Option<&str>) -> String {
    let raw = profile
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(PROFILE_QUICK)
        .to_lowercase();
    match raw.as_str() {
        "smoke" | "fast" => PROFILE_QUICK.to_string(),
        "standard" | "balanced" => PROFILE_CORE.to_string(),
        "all" | "complete" => PROFILE_FULL.to_string(),
        PROFILE_QUICK | PROFILE_CORE | PROFILE_FULL => raw,
        _ => PROFILE_QUICK.to_string(),
    }
}

fn has_non_empty(values: &[String]) -> bool {
    values.iter().any(|value| !value.trim().is_empty())
}

fn filter_tasks(
    tasks: Vec<BenchmarkTaskSpec>,
    suite_ids: &[String],
    task_ids: &[String],
) -> Vec<BenchmarkTaskSpec> {
    let suite_filter = suite_ids
        .iter()
        .map(|value| value.trim().to_lowercase())
        .filter(|value| !value.is_empty())
        .collect::<HashSet<_>>();
    let task_filter = task_ids
        .iter()
        .map(|value| value.trim().to_lowercase())
        .filter(|value| !value.is_empty())
        .collect::<HashSet<_>>();
    tasks
        .into_iter()
        .filter(|task| {
            (suite_filter.is_empty()
                || suite_filter.contains(&task.frontmatter.suite.to_lowercase()))
                && (task_filter.is_empty()
                    || task_filter.contains(&task.frontmatter.id.to_lowercase()))
        })
        .collect()
}

fn select_quick_tasks(tasks: &[BenchmarkTaskSpec]) -> Vec<BenchmarkTaskSpec> {
    let mut by_suite = BTreeMap::<String, Vec<&BenchmarkTaskSpec>>::new();
    for task in tasks {
        by_suite
            .entry(task.frontmatter.suite.clone())
            .or_default()
            .push(task);
    }
    let mut selected = Vec::new();
    for (_, mut suite_tasks) in by_suite {
        suite_tasks.sort_by_key(|task| task_sort_key(task));
        if let Some(task) = suite_tasks.first() {
            selected.push((*task).clone());
        }
    }
    selected
}

fn select_core_tasks(tasks: &[BenchmarkTaskSpec]) -> Vec<BenchmarkTaskSpec> {
    let mut selected = select_quick_tasks(tasks);
    let mut selected_ids = selected
        .iter()
        .map(|task| task.id().to_string())
        .collect::<HashSet<_>>();
    let mut ranked = tasks.iter().collect::<Vec<_>>();
    ranked.sort_by_key(|task| task_sort_key(task));
    for task in ranked {
        if selected_ids.contains(task.id()) {
            continue;
        }
        selected_ids.insert(task.id().to_string());
        selected.push(task.clone());
        if selected.len() >= tasks.len().min(8) {
            break;
        }
    }
    selected
}

fn task_sort_key(task: &BenchmarkTaskSpec) -> (u8, u8, u64, String) {
    let grading_rank = match task.grading_type() {
        BenchmarkGradingType::Automated => 0,
        BenchmarkGradingType::Hybrid => 1,
        BenchmarkGradingType::LlmJudge => 2,
    };
    let difficulty_rank = match task.frontmatter.difficulty.trim().to_lowercase().as_str() {
        "easy" => 0,
        "medium" => 1,
        "hard" => 2,
        _ => 1,
    };
    (
        grading_rank,
        difficulty_rank,
        task.timeout_seconds(),
        task.id().to_string(),
    )
}

fn collect_suite_ids(tasks: &[BenchmarkTaskSpec]) -> Vec<String> {
    let mut values = tasks
        .iter()
        .map(|task| task.frontmatter.suite.clone())
        .filter(|value| !value.trim().is_empty())
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values
}

fn mean_values(items: &[Value], key: &str) -> f64 {
    let values = items
        .iter()
        .filter_map(|item| item.get(key).and_then(Value::as_f64))
        .collect::<Vec<_>>();
    mean(&values)
}

fn tool_success_rate(attempts: &[Value]) -> f64 {
    let mut total = 0usize;
    let mut ok = 0usize;
    for attempt in attempts {
        let Some(results) = attempt.get("tool_results").and_then(Value::as_array) else {
            continue;
        };
        for result in results {
            total += 1;
            if result.get("ok").and_then(Value::as_bool).unwrap_or(false) {
                ok += 1;
            }
        }
    }
    if total == 0 {
        return 1.0;
    }
    ok as f64 / total as f64
}

fn stability_score(task_aggregates: &[Value], attempts: &[Value]) -> f64 {
    let finished_rate = if attempts.is_empty() {
        0.0
    } else {
        attempts
            .iter()
            .filter(|attempt| attempt.get("status").and_then(Value::as_str) == Some("finished"))
            .count() as f64
            / attempts.len() as f64
    };
    let score_volatility = mean_values(task_aggregates, "std_score").clamp(0.0, 1.0);
    (finished_rate * 0.7) + ((1.0 - score_volatility) * 0.3)
}

fn efficiency_score(attempts: &[Value]) -> f64 {
    let elapsed = attempts
        .iter()
        .filter_map(|attempt| attempt.get("elapsed_s").and_then(Value::as_f64))
        .collect::<Vec<_>>();
    let avg_elapsed = mean(&elapsed);
    if avg_elapsed <= 0.0 {
        return 0.0;
    }
    (60.0 / avg_elapsed).clamp(0.0, 1.0)
}

fn weakest_suites(task_aggregates: &[Value]) -> Vec<Value> {
    let mut by_suite = HashMap::<String, Vec<f64>>::new();
    for task in task_aggregates {
        let suite = task
            .get("suite")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if suite.is_empty() {
            continue;
        }
        by_suite.entry(suite.to_string()).or_default().push(
            task.get("mean_score")
                .and_then(Value::as_f64)
                .unwrap_or(0.0),
        );
    }
    let mut values = by_suite
        .into_iter()
        .map(|(suite, scores)| {
            json!({
                "suite": suite,
                "mean_score": round6(mean(&scores)),
                "task_count": scores.len(),
            })
        })
        .collect::<Vec<_>>();
    values.sort_by(|left, right| {
        let left_score = left
            .get("mean_score")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let right_score = right
            .get("mean_score")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        left_score
            .partial_cmp(&right_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    values.truncate(3);
    values
}

fn top_failures(attempts: &[Value]) -> Vec<Value> {
    let mut failures = attempts
        .iter()
        .filter_map(|attempt| {
            let score = attempt
                .get("final_score")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let status = attempt
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            if score >= 0.8 && status == "finished" {
                return None;
            }
            Some(json!({
                "task_id": attempt.get("task_id").and_then(Value::as_str).unwrap_or(""),
                "attempt_no": attempt.get("attempt_no").and_then(Value::as_u64).unwrap_or(0),
                "status": status,
                "score": round6(score),
                "error": attempt.get("error").and_then(Value::as_str).unwrap_or(""),
            }))
        })
        .collect::<Vec<_>>();
    failures.sort_by(|left, right| {
        let left_score = left.get("score").and_then(Value::as_f64).unwrap_or(0.0);
        let right_score = right.get("score").and_then(Value::as_f64).unwrap_or(0.0);
        left_score
            .partial_cmp(&right_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    failures.truncate(5);
    failures
}

fn readiness_label(
    total_score: f64,
    reliability_score: f64,
    tool_success_score: f64,
) -> &'static str {
    if total_score >= 0.85 && reliability_score >= 0.8 && tool_success_score >= 0.95 {
        "production_ready"
    } else if total_score >= 0.7 && reliability_score >= 0.6 {
        "usable"
    } else if total_score >= 0.5 {
        "risky"
    } else {
        "not_ready"
    }
}

fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.iter().sum::<f64>() / values.len() as f64
}

fn round6(value: f64) -> f64 {
    (value * 1_000_000.0).round() / 1_000_000.0
}

#[cfg(test)]
mod tests {
    use super::{build_scorecard, resolve_profile_tasks};
    use crate::benchmark::spec::{
        BenchmarkGradingType, BenchmarkTaskFrontmatter, BenchmarkTaskSpec,
    };
    use serde_json::json;

    #[test]
    fn quick_profile_selects_one_fast_task_per_suite() {
        let tasks = vec![
            task(
                "task_a",
                "suite-a",
                BenchmarkGradingType::LlmJudge,
                "hard",
                300,
            ),
            task(
                "task_b",
                "suite-a",
                BenchmarkGradingType::Automated,
                "easy",
                60,
            ),
            task(
                "task_c",
                "suite-b",
                BenchmarkGradingType::Hybrid,
                "medium",
                180,
            ),
        ];
        let selection = resolve_profile_tasks(tasks, Some("quick"), &[], &[]);
        let ids = selection
            .tasks
            .iter()
            .map(|task| task.id())
            .collect::<Vec<_>>();
        assert_eq!(selection.profile, "quick");
        assert_eq!(ids, vec!["task_b", "task_c"]);
        assert_eq!(selection.suite_ids, vec!["suite-a", "suite-b"]);
    }

    #[test]
    fn scorecard_marks_strong_runs_ready() {
        let scorecard = build_scorecard(
            &[
                json!({"suite": "workspace", "mean_score": 0.9, "pass_rate": 1.0, "std_score": 0.0}),
                json!({"suite": "coding", "mean_score": 0.86, "pass_rate": 1.0, "std_score": 0.1}),
            ],
            &[
                json!({"status": "finished", "final_score": 0.9, "elapsed_s": 30.0, "tool_results": [{"ok": true}]}),
                json!({"status": "finished", "final_score": 0.86, "elapsed_s": 40.0, "tool_results": [{"ok": true}]}),
            ],
        );
        assert_eq!(scorecard["readiness"], "production_ready");
        assert_eq!(scorecard["overall_score"], json!(0.88));
    }

    #[test]
    fn scorecard_surfaces_failures_and_weak_suites() {
        let scorecard = build_scorecard(
            &[
                json!({"suite": "workspace", "mean_score": 0.92, "pass_rate": 1.0, "std_score": 0.0}),
                json!({"suite": "coding", "mean_score": 0.3, "pass_rate": 0.0, "std_score": 0.4}),
            ],
            &[
                json!({"task_id": "task_ok", "attempt_no": 1, "status": "finished", "final_score": 0.92, "elapsed_s": 20.0, "tool_results": [{"ok": true}]}),
                json!({"task_id": "task_bad", "attempt_no": 1, "status": "error", "final_score": 0.3, "elapsed_s": 90.0, "tool_results": [{"ok": false}], "error": "failed"}),
            ],
        );
        assert_eq!(scorecard["readiness"], "risky");
        assert_eq!(scorecard["tool_success_score"], json!(0.5));
        assert_eq!(scorecard["weakest_suites"][0]["suite"], "coding");
        assert_eq!(scorecard["top_failures"][0]["task_id"], "task_bad");
    }

    fn task(
        id: &str,
        suite: &str,
        grading_type: BenchmarkGradingType,
        difficulty: &str,
        timeout_seconds: u64,
    ) -> BenchmarkTaskSpec {
        BenchmarkTaskSpec {
            frontmatter: BenchmarkTaskFrontmatter {
                id: id.to_string(),
                name: id.to_string(),
                suite: suite.to_string(),
                category: "demo".to_string(),
                grading_type,
                timeout_seconds,
                difficulty: difficulty.to_string(),
                ..BenchmarkTaskFrontmatter::default()
            },
            prompt: "Do the task".to_string(),
            expected_behavior: "Done".to_string(),
            grading_criteria: Vec::new(),
            automated_checks: None,
            llm_judge_rubric: None,
            file_path: format!("{id}.md"),
        }
    }
}
