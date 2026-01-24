use crate::i18n;
use anyhow::{anyhow, Result};
use regex::Regex;
use std::path::Path;
use std::sync::OnceLock;
use tracing::error;

#[derive(Debug, Clone)]
pub struct ConversionResult {
    pub converter: String,
    pub warnings: Vec<String>,
}

pub fn get_supported_extensions() -> Vec<String> {
    static CACHE: OnceLock<Vec<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let mut exts = crate::doc2md::supported_extensions();
            exts.sort();
            exts
        })
        .clone()
}

pub fn sanitize_filename_stem(name: &str) -> String {
    let cleaned = match filename_safe_regex() {
        Some(regex) => regex.replace_all(name.trim(), "_").to_string(),
        None => name.trim().to_string(),
    };
    let cleaned = cleaned.trim_matches(['.', ' '].as_ref()).to_string();
    cleaned.replace("..", "_")
}

pub async fn convert_to_markdown(
    input_path: &Path,
    output_path: &Path,
    extension: &str,
) -> Result<ConversionResult> {
    let conversion = crate::doc2md::convert_path(input_path, extension).await?;
    if conversion.markdown.trim().is_empty() {
        return Err(anyhow!(i18n::t("error.converter_empty_result")));
    }
    if let Some(parent) = output_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(output_path, conversion.markdown).await?;
    Ok(ConversionResult {
        converter: conversion.converter,
        warnings: conversion.warnings,
    })
}

fn filename_safe_regex() -> Option<&'static Regex> {
    static REGEX: OnceLock<Option<Regex>> = OnceLock::new();
    REGEX
        .get_or_init(|| compile_regex(r#"[\\/:*?"<>|]+"#, "filename_safe"))
        .as_ref()
}

fn compile_regex(pattern: &str, label: &str) -> Option<Regex> {
    match Regex::new(pattern) {
        Ok(regex) => Some(regex),
        Err(err) => {
            error!("invalid attachment regex {label}: {err}");
            None
        }
    }
}
