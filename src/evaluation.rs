use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationCaseFile {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub case_set: String,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub cases: Vec<EvaluationCase>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationCase {
    pub id: String,
    pub dimension: EvaluationDimension,
    pub prompt: String,
    pub checker: EvaluationChecker,
    #[serde(default = "default_case_weight")]
    pub weight: f64,
    #[serde(default)]
    pub timeout_s: Option<u64>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub prerequisites: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum EvaluationDimension {
    Tool,
    Logic,
    Common,
    Complex,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EvaluationChecker {
    Choice { answer: String },
    Exact { answer: String },
    Contains { text: String },
    Regex { pattern: String },
    ToolCalled { tool: String },
    ToolArgs { tool: String, required: Value },
    FileExists { path: String },
    FileContains { path: String, text: String },
    JsonContains { path: String, required: Value },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DimensionWeights {
    pub tool: f64,
    pub logic: f64,
    pub common: f64,
    pub complex: f64,
}

impl Default for DimensionWeights {
    fn default() -> Self {
        Self {
            tool: 35.0,
            logic: 25.0,
            common: 20.0,
            complex: 20.0,
        }
    }
}

pub fn load_case_files(dir: &Path) -> Result<Vec<EvaluationCaseFile>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("读取目录失败: {dir:?}"))? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
            paths.push(path);
        }
    }
    paths.sort();
    let mut files = Vec::new();
    for path in paths {
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("读取评估用例失败: {path:?}"))?;
        let file = serde_json::from_str::<EvaluationCaseFile>(&text)
            .with_context(|| format!("解析评估用例失败: {path:?}"))?;
        files.push(file);
    }
    Ok(files)
}

pub fn default_cases_dir() -> PathBuf {
    PathBuf::from("./data/evaluation/cases")
}

fn default_case_weight() -> f64 {
    1.0
}

pub fn normalize_dimension_weights(weights: DimensionWeights) -> DimensionWeights {
    let total = weights.tool + weights.logic + weights.common + weights.complex;
    if total <= 0.0 {
        return DimensionWeights::default();
    }
    let scale = 100.0 / total;
    DimensionWeights {
        tool: weights.tool * scale,
        logic: weights.logic * scale,
        common: weights.common * scale,
        complex: weights.complex * scale,
    }
}
