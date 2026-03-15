use super::spec::BenchmarkTaskSpec;
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub fn build_task_aggregate(task: &BenchmarkTaskSpec, attempts: &[Value]) -> Value {
    let scores = attempts
        .iter()
        .filter_map(|attempt| attempt.get("final_score").and_then(Value::as_f64))
        .collect::<Vec<_>>();
    let elapsed = attempts
        .iter()
        .filter_map(|attempt| attempt.get("elapsed_s").and_then(Value::as_f64))
        .collect::<Vec<_>>();
    let pass_rate = if attempts.is_empty() {
        0.0
    } else {
        attempts
            .iter()
            .filter(|attempt| {
                attempt
                    .get("final_score")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0)
                    >= 0.8
            })
            .count() as f64
            / attempts.len() as f64
    };
    let status = if attempts.is_empty() {
        "cancelled"
    } else if attempts
        .iter()
        .any(|attempt| attempt.get("status").and_then(Value::as_str) == Some("cancelled"))
    {
        "cancelled"
    } else if attempts
        .iter()
        .all(|attempt| attempt.get("status").and_then(Value::as_str) == Some("finished"))
    {
        "finished"
    } else if attempts
        .iter()
        .any(|attempt| attempt.get("status").and_then(Value::as_str) == Some("error"))
    {
        "error"
    } else {
        "mixed"
    };

    json!({
        "task_id": task.id(),
        "name": task.frontmatter.name,
        "suite": task.frontmatter.suite,
        "category": task.frontmatter.category,
        "grading_type": task.frontmatter.grading_type,
        "status": status,
        "attempt_count": attempts.len(),
        "mean_score": round6(mean(&scores)),
        "std_score": round6(stddev(&scores)),
        "min_score": round6(min_value(&scores)),
        "max_score": round6(max_value(&scores)),
        "pass_rate": round6(pass_rate),
        "mean_elapsed_s": round6(mean(&elapsed)),
        "attempts": attempts,
    })
}

pub fn build_run_summary(
    run_id: &str,
    user_id: &str,
    model_name: &str,
    judge_model_name: &str,
    suite_ids: &[String],
    tool_snapshot: &[String],
    task_aggregates: &[Value],
    attempts: &[Value],
    started_time: f64,
    finished_time: f64,
    status: &str,
    config_overrides: &Value,
) -> Value {
    let task_scores = task_aggregates
        .iter()
        .filter_map(|item| item.get("mean_score").and_then(Value::as_f64))
        .collect::<Vec<_>>();
    let total_context_tokens = attempts
        .iter()
        .filter_map(|attempt| {
            attempt
                .pointer("/usage/context_tokens")
                .and_then(Value::as_u64)
        })
        .sum::<u64>();
    let total_elapsed_s = attempts
        .iter()
        .filter_map(|attempt| attempt.get("elapsed_s").and_then(Value::as_f64))
        .sum::<f64>();
    let total_score = mean(&task_scores);
    let score_per_1k_context_tokens = if total_context_tokens > 0 {
        Some(total_score / (total_context_tokens as f64 / 1000.0))
    } else {
        None
    };
    let score_per_minute = if total_elapsed_s > 0.0 {
        Some(total_score / (total_elapsed_s / 60.0))
    } else {
        None
    };

    let mut suite_scores = BTreeMap::new();
    for task in task_aggregates {
        let Some(suite) = task.get("suite").and_then(Value::as_str) else {
            continue;
        };
        let score = task
            .get("mean_score")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        suite_scores
            .entry(suite.to_string())
            .or_insert_with(Vec::new)
            .push(score);
    }
    let suite_scores = suite_scores
        .into_iter()
        .map(|(suite, scores)| {
            (
                suite,
                json!({ "mean_score": round6(mean(&scores)), "task_count": scores.len() }),
            )
        })
        .collect::<BTreeMap<_, _>>();

    json!({
        "run_id": run_id,
        "user_id": user_id,
        "model_name": model_name,
        "judge_model_name": judge_model_name,
        "suite_ids": suite_ids,
        "tool_snapshot": tool_snapshot,
        "status": status,
        "task_count": task_aggregates.len(),
        "attempt_count": attempts.len(),
        "total_score": round6(total_score),
        "suite_scores": suite_scores,
        "efficiency": {
            "total_context_tokens": total_context_tokens,
            "total_elapsed_s": round6(total_elapsed_s),
            "score_per_1k_context_tokens": score_per_1k_context_tokens.map(round6),
            "score_per_minute": score_per_minute.map(round6),
        },
        "started_time": started_time,
        "finished_time": finished_time,
        "elapsed_s": round6((finished_time - started_time).max(0.0)),
        "config_overrides": config_overrides.clone(),
    })
}

fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.iter().sum::<f64>() / values.len() as f64
}

fn stddev(values: &[f64]) -> f64 {
    if values.len() < 2 {
        return 0.0;
    }
    let avg = mean(values);
    let variance = values
        .iter()
        .map(|value| {
            let delta = value - avg;
            delta * delta
        })
        .sum::<f64>()
        / values.len() as f64;
    variance.sqrt()
}

fn min_value(values: &[f64]) -> f64 {
    values.iter().copied().reduce(f64::min).unwrap_or(0.0)
}

fn max_value(values: &[f64]) -> f64 {
    values.iter().copied().reduce(f64::max).unwrap_or(0.0)
}

fn round6(value: f64) -> f64 {
    (value * 1_000_000.0).round() / 1_000_000.0
}

#[cfg(test)]
mod tests {
    use super::{build_task_aggregate, stddev};
    use crate::benchmark::spec::{
        BenchmarkGradingType, BenchmarkTaskFrontmatter, BenchmarkTaskSpec,
    };

    #[test]
    fn stddev_zero_for_single_value() {
        assert_eq!(stddev(&[0.8]), 0.0);
    }

    #[test]
    fn empty_attempts_are_marked_cancelled() {
        let task = BenchmarkTaskSpec {
            frontmatter: BenchmarkTaskFrontmatter {
                id: "task_demo".to_string(),
                name: "Demo".to_string(),
                suite: "workspace-core".to_string(),
                category: "workspace".to_string(),
                grading_type: BenchmarkGradingType::Automated,
                ..BenchmarkTaskFrontmatter::default()
            },
            prompt: "Do something".to_string(),
            expected_behavior: "Finish".to_string(),
            grading_criteria: vec!["File created".to_string()],
            automated_checks: Some(
                "def grade(transcript, workspace_path):
    return {'ok': 1.0}"
                    .to_string(),
            ),
            llm_judge_rubric: None,
            file_path: "task_demo.md".to_string(),
        };
        let aggregate = build_task_aggregate(&task, &[]);
        assert_eq!(aggregate["status"], "cancelled");
        assert_eq!(aggregate["attempt_count"], 0);
        assert_eq!(aggregate["mean_score"], 0.0);
    }
}
