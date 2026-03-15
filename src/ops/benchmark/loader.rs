use super::spec::{BenchmarkGradingType, BenchmarkTaskFrontmatter, BenchmarkTaskSpec};
use anyhow::{anyhow, Context, Result};
use regex::Regex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub fn default_tasks_dir() -> PathBuf {
    PathBuf::from("./config/benchmark/tasks")
}

pub fn default_assets_dir() -> PathBuf {
    PathBuf::from("./config/benchmark/assets")
}

pub fn load_task_specs(dir: &Path) -> Result<Vec<BenchmarkTaskSpec>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut paths = Vec::new();
    for entry in
        std::fs::read_dir(dir).with_context(|| format!("读取 benchmark 任务目录失败: {dir:?}"))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) == Some("md") {
            paths.push(path);
        }
    }
    paths.sort();

    let mut tasks = Vec::new();
    for path in paths {
        let task = load_task_spec(&path)?;
        if task.id() == "task_XX_name" {
            continue;
        }
        tasks.push(task);
    }
    Ok(tasks)
}

pub fn load_task_spec(path: &Path) -> Result<BenchmarkTaskSpec> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("读取任务文件失败: {path:?}"))?;
    let (frontmatter_text, body_text) = split_frontmatter(&text)?;
    let frontmatter: BenchmarkTaskFrontmatter = serde_yaml::from_str(&frontmatter_text)
        .with_context(|| format!("解析任务 frontmatter 失败: {path:?}"))?;
    validate_frontmatter(&frontmatter, path)?;

    let sections = parse_sections(&body_text);
    let prompt = sections.get("Prompt").cloned().unwrap_or_default();
    let expected_behavior = sections
        .get("Expected Behavior")
        .cloned()
        .unwrap_or_default();
    let grading_criteria = extract_grading_criteria(
        sections
            .get("Grading Criteria")
            .map(String::as_str)
            .unwrap_or_default(),
    );
    let automated_checks = sections
        .get("Automated Checks")
        .and_then(|value| extract_code_block(value, "python"));
    let llm_judge_rubric = sections.get("LLM Judge Rubric").cloned();

    let spec = BenchmarkTaskSpec {
        frontmatter,
        prompt: prompt.trim().to_string(),
        expected_behavior: expected_behavior.trim().to_string(),
        grading_criteria,
        automated_checks,
        llm_judge_rubric: llm_judge_rubric.map(|value| value.trim().to_string()),
        file_path: path.to_string_lossy().to_string(),
    };
    validate_task_spec(&spec, path)?;
    Ok(spec)
}

fn split_frontmatter(content: &str) -> Result<(String, String)> {
    let normalized = content.replace("\r\n", "\n");
    let Some(rest) = normalized.strip_prefix("---\n") else {
        return Err(anyhow!("missing yaml frontmatter"));
    };
    let Some(end) = rest.find("\n---\n") else {
        return Err(anyhow!("unterminated yaml frontmatter"));
    };
    let frontmatter = rest[..end].to_string();
    let body = rest[(end + 5)..].to_string();
    Ok((frontmatter, body))
}

fn parse_sections(body: &str) -> HashMap<String, String> {
    let mut sections = HashMap::new();
    let mut current_section = String::new();
    let mut current_lines = Vec::new();
    for line in body.lines() {
        if let Some(name) = line.strip_prefix("## ") {
            if !current_section.is_empty() {
                sections.insert(
                    current_section.clone(),
                    current_lines.join("\n").trim().to_string(),
                );
            }
            current_section = name.trim().to_string();
            current_lines.clear();
            continue;
        }
        if !current_section.is_empty() {
            current_lines.push(line.to_string());
        }
    }
    if !current_section.is_empty() {
        sections.insert(current_section, current_lines.join("\n").trim().to_string());
    }
    sections
}

fn extract_grading_criteria(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            trimmed
                .strip_prefix("- [ ] ")
                .or_else(|| trimmed.strip_prefix("- [x] "))
                .map(|value| value.trim().to_string())
        })
        .filter(|value| !value.is_empty())
        .collect()
}

fn extract_code_block(text: &str, language: &str) -> Option<String> {
    let pattern = format!(r"(?s)```{}\s*(.*?)\s*```", regex::escape(language));
    let regex = Regex::new(&pattern).ok()?;
    regex.captures(text).and_then(|captures| {
        captures
            .get(1)
            .map(|value| value.as_str().trim().to_string())
    })
}

fn validate_frontmatter(frontmatter: &BenchmarkTaskFrontmatter, path: &Path) -> Result<()> {
    if frontmatter.id.trim().is_empty() {
        return Err(anyhow!("task id required: {path:?}"));
    }
    if frontmatter.name.trim().is_empty() {
        return Err(anyhow!("task name required: {path:?}"));
    }
    if frontmatter.suite.trim().is_empty() {
        return Err(anyhow!("task suite required: {path:?}"));
    }
    Ok(())
}

fn validate_task_spec(spec: &BenchmarkTaskSpec, path: &Path) -> Result<()> {
    if spec.prompt.trim().is_empty() {
        return Err(anyhow!("Prompt section required: {path:?}"));
    }
    match spec.grading_type() {
        BenchmarkGradingType::Automated => {
            if !spec.has_automated_checks() {
                return Err(anyhow!(
                    "Automated task missing python grade block: {path:?}"
                ));
            }
        }
        BenchmarkGradingType::LlmJudge => {
            if !spec.has_judge_rubric() {
                return Err(anyhow!(
                    "LLM judge task missing rubric or criteria: {path:?}"
                ));
            }
        }
        BenchmarkGradingType::Hybrid => {
            if !spec.has_automated_checks() {
                return Err(anyhow!("Hybrid task missing python grade block: {path:?}"));
            }
            if !spec.has_judge_rubric() {
                return Err(anyhow!("Hybrid task missing rubric or criteria: {path:?}"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{default_tasks_dir, load_task_spec, load_task_specs};

    #[test]
    fn load_task_markdown_spec() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("task_demo.md");
        std::fs::write(
            &path,
            "---\nid: task_demo\nname: Demo\nsuite: workspace-core\ncategory: workspace\ngrading_type: automated\ntimeout_seconds: 120\nworkspace_files: []\n---\n\n## Prompt\n\nDo something.\n\n## Expected Behavior\n\nFinish the job.\n\n## Grading Criteria\n\n- [ ] File created\n\n## Automated Checks\n\n```python\ndef grade(transcript, workspace_path):\n    return {\"ok\": 1.0}\n```\n",
        )
        .expect("write spec");
        let spec = load_task_spec(&path).expect("load spec");
        assert_eq!(spec.frontmatter.id, "task_demo");
        assert_eq!(spec.frontmatter.suite, "workspace-core");
        assert_eq!(spec.grading_criteria, vec!["File created"]);
        assert!(spec.has_automated_checks());
    }

    #[test]
    fn load_repository_benchmark_tasks() {
        let tasks = load_task_specs(default_tasks_dir().as_path())
            .expect("load repository benchmark tasks");
        assert!(!tasks.is_empty());
    }
}
