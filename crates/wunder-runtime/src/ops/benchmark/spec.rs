use serde::{Deserialize, Serialize};
use std::collections::HashMap;

fn default_runs_recommended() -> u32 {
    3
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BenchmarkGradingType {
    Automated,
    LlmJudge,
    Hybrid,
}

impl Default for BenchmarkGradingType {
    fn default() -> Self {
        Self::Automated
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WorkspaceFileSpec {
    Asset { source: String, dest: String },
    Inline { path: String, content: String },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BenchmarkTaskFrontmatter {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub suite: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub grading_type: BenchmarkGradingType,
    #[serde(default)]
    pub timeout_seconds: u64,
    #[serde(default = "default_runs_recommended")]
    pub runs_recommended: u32,
    #[serde(default)]
    pub grading_weights: HashMap<String, f64>,
    #[serde(default)]
    pub workspace_files: Vec<WorkspaceFileSpec>,
    #[serde(default)]
    pub difficulty: String,
    #[serde(default)]
    pub required_tools: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub languages: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkTaskSpec {
    pub frontmatter: BenchmarkTaskFrontmatter,
    pub prompt: String,
    pub expected_behavior: String,
    #[serde(default)]
    pub grading_criteria: Vec<String>,
    #[serde(default)]
    pub automated_checks: Option<String>,
    #[serde(default)]
    pub llm_judge_rubric: Option<String>,
    pub file_path: String,
}

impl BenchmarkTaskSpec {
    pub fn id(&self) -> &str {
        self.frontmatter.id.trim()
    }

    pub fn suite(&self) -> &str {
        self.frontmatter.suite.trim()
    }

    pub fn grading_type(&self) -> &BenchmarkGradingType {
        &self.frontmatter.grading_type
    }

    pub fn timeout_seconds(&self) -> u64 {
        self.frontmatter.timeout_seconds.max(30)
    }

    pub fn has_automated_checks(&self) -> bool {
        self.automated_checks
            .as_deref()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
    }

    pub fn has_judge_rubric(&self) -> bool {
        self.llm_judge_rubric
            .as_deref()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
            || !self.grading_criteria.is_empty()
    }

    pub fn preferred_language(&self) -> Option<String> {
        self.frontmatter
            .languages
            .iter()
            .find(|value| !value.trim().is_empty())
            .map(|value| value.trim().to_string())
    }
}
