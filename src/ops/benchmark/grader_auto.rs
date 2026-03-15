use super::models::ExecutionCapture;
use super::spec::BenchmarkTaskSpec;
use crate::config::Config;
use anyhow::{anyhow, Context, Result};
use serde_json::{json, Map, Value};
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

pub async fn grade_automated(
    task: &BenchmarkTaskSpec,
    capture: &ExecutionCapture,
    workspace_path: &Path,
    config: &Config,
) -> Result<Value> {
    let Some(grading_code) = task.automated_checks.as_deref() else {
        return Ok(json!({
            "score": 0.0,
            "breakdown": {},
            "notes": "no automated checks",
            "error": "",
        }));
    };

    let transcript_path = workspace_path.join(".benchmark_transcript.json");
    let wrapper_path = workspace_path.join(".benchmark_grade_wrapper.py");
    std::fs::write(
        &transcript_path,
        serde_json::to_vec_pretty(&capture.transcript).context("serialize transcript failed")?,
    )?;
    std::fs::write(&wrapper_path, build_wrapper_script(grading_code))?;

    let output = run_python_script(config, &wrapper_path, workspace_path, &transcript_path).await?;
    let parsed = parse_last_json_line(&output.stdout).unwrap_or_else(|_| {
        json!({
            "scores": {},
            "notes": output.stderr,
            "error": "invalid grading json",
        })
    });
    let breakdown = normalize_breakdown(parsed.get("scores").cloned().unwrap_or(Value::Null));
    let score = average_breakdown(&breakdown);
    Ok(json!({
        "score": score,
        "breakdown": breakdown,
        "notes": parsed.get("notes").and_then(Value::as_str).unwrap_or(""),
        "error": parsed.get("error").and_then(Value::as_str).unwrap_or(""),
        "stdout": output.stdout,
        "stderr": output.stderr,
    }))
}

struct PythonOutput {
    stdout: String,
    stderr: String,
}

async fn run_python_script(
    config: &Config,
    script_path: &Path,
    workspace_path: &Path,
    transcript_path: &Path,
) -> Result<PythonOutput> {
    let candidates = python_candidates(config);
    let mut last_error = None;
    for (program, args) in candidates {
        let mut command = Command::new(&program);
        command.args(&args);
        command.arg(script_path);
        command.current_dir(workspace_path);
        command.env("PYTHONIOENCODING", "utf-8");
        command.env("BENCHMARK_TRANSCRIPT_PATH", transcript_path);
        command.env("BENCHMARK_WORKSPACE_PATH", workspace_path);
        command.stdin(Stdio::null());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        match command.spawn() {
            Ok(child) => {
                let output = timeout(Duration::from_secs(20), child.wait_with_output())
                    .await
                    .map_err(|_| anyhow!("automated grading timeout"))??;
                return Ok(PythonOutput {
                    stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
                });
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => {
                last_error = Some(err.to_string());
                break;
            }
        }
    }
    Err(anyhow!(last_error.unwrap_or_else(|| {
        "python interpreter not found".to_string()
    })))
}

fn python_candidates(config: &Config) -> Vec<(String, Vec<String>)> {
    let mut output = Vec::new();
    if let Some(path) = config.tools.browser.python_path.as_deref() {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            output.push((trimmed.to_string(), Vec::new()));
        }
    }
    output.push(("python".to_string(), Vec::new()));
    output.push(("python3".to_string(), Vec::new()));
    if cfg!(windows) {
        output.push(("py".to_string(), vec!["-3".to_string()]));
    }
    output
}

fn build_wrapper_script(grading_code: &str) -> String {
    format!(
        "import json\nimport os\nimport traceback\n\nwith open(os.environ['BENCHMARK_TRANSCRIPT_PATH'], 'r', encoding='utf-8') as fp:\n    transcript = json.load(fp)\nworkspace_path = os.environ['BENCHMARK_WORKSPACE_PATH']\n\n{}\n\nresult = {{'scores': {{}}, 'notes': '', 'error': ''}}\ntry:\n    if 'grade' not in globals() or not callable(grade):\n        result['error'] = 'grade function missing'\n    else:\n        scores = grade(transcript, workspace_path)\n        if isinstance(scores, dict):\n            result['scores'] = scores\n        else:\n            result['error'] = 'grade() must return dict'\nexcept Exception as exc:\n    result['error'] = ''.join(traceback.format_exception_only(type(exc), exc)).strip()\nprint(json.dumps(result, ensure_ascii=False))\n",
        grading_code
    )
}

fn parse_last_json_line(text: &str) -> Result<Value> {
    let candidate = text
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .ok_or_else(|| anyhow!("empty grading output"))?;
    serde_json::from_str(candidate).context("parse grading output failed")
}

fn normalize_breakdown(value: Value) -> Value {
    let mut output = Map::new();
    if let Value::Object(map) = value {
        for (key, value) in map {
            if let Some(score) = value.as_f64() {
                output.insert(key, json!(score.clamp(0.0, 1.0)));
            }
        }
    }
    Value::Object(output)
}

fn average_breakdown(value: &Value) -> f64 {
    let Some(map) = value.as_object() else {
        return 0.0;
    };
    let values = map.values().filter_map(Value::as_f64).collect::<Vec<_>>();
    if values.is_empty() {
        return 0.0;
    }
    values.iter().sum::<f64>() / values.len() as f64
}
