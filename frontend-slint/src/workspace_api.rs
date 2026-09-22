//! Bounded reads of the existing workspace protocol; paths remain backend scoped.
use crate::chat_api::{encode_query_value, parse_file, ChatApi, FileRecord};
use serde_json::Value;

pub const PAGE_SIZE: i32 = 100;
pub struct Directory {
    pub path: String,
    pub parent: String,
    pub total: i32,
    pub entries: Vec<FileRecord>,
}

impl ChatApi {
    pub fn workspace_directory(
        &self,
        agent: &str,
        path: &str,
        offset: i32,
    ) -> Result<Directory, String> {
        let payload = self.get_json(&format!(
            "/workspace?agent_id={}&path={}&offset={}&limit={PAGE_SIZE}&sort_by=name&order=asc&refresh_tree=true",
            encode_query_value(agent), encode_query_value(path), offset.max(0)
        ))?;
        let entries = payload["entries"].as_array().ok_or("工作目录响应无效")?;
        Ok(Directory {
            path: payload["path"].as_str().unwrap_or_default().into(),
            parent: payload["parent"].as_str().unwrap_or_default().into(),
            total: payload["total"].as_u64().unwrap_or(0).min(i32::MAX as u64) as i32,
            entries: entries
                .iter()
                .filter_map(parse_file)
                .take(PAGE_SIZE as usize)
                .collect(),
        })
    }

    pub fn workspace_preview(&self, agent: &str, path: &str) -> Result<String, String> {
        let payload = self.get_json(&format!(
            "/workspace/content?agent_id={}&path={}&max_bytes=32768&include_content=true",
            encode_query_value(agent),
            encode_query_value(path)
        ))?;
        let text = payload["content"].as_str().ok_or("该文件没有可预览文本")?;
        if text.chars().any(|ch| ch == '\0') {
            return Ok("此文件为二进制内容，暂不提供预览。".into());
        }
        let mut text = text.to_string();
        if payload["truncated"] == Value::Bool(true) {
            text.push_str("\n\n（仅预览前 32 KiB）");
        }
        Ok(text)
    }
}
