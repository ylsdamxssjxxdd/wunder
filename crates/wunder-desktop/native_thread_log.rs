//! Bounded durable thread-log pages, shared with the web history semantics.
use super::NativeDesktop;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};

pub struct NativeThreadLogPage {
    pub overview: Value,
    pub username: String,
    pub agent_name: String,
    pub turns: Vec<Value>,
    pub next_before: Option<i64>,
    pub has_more: bool,
    pub turn_total: i64,
    pub item_total: i64,
}

pub struct NativeThreadLogTurn {
    pub items: Vec<Value>,
    pub next_after: i64,
    pub has_more: bool,
}

impl NativeDesktop {
    /// Stream the complete user-visible durable log to a user-selected file.
    /// Each query is bounded; no full-session array is retained in memory.
    pub fn export_thread_log(&self, session: &str, target: &std::path::Path) -> Result<()> {
        use std::io::Write;
        self.get_session_info(session)?;
        let mut writer = std::io::BufWriter::new(std::fs::File::create(target)?);
        writer.write_all(b"{\"overview\":")?;
        serde_json::to_writer(&mut writer, &self.state().monitor.get_log_overview(session))?;
        writer.write_all(b",\"turns\":[")?;
        let mut before = None;
        let mut first_turn = true;
        loop {
            let turns =
                self.state()
                    .storage
                    .list_thread_turns(self.user_id(), session, before, 50)?;
            if turns.is_empty() {
                break;
            }
            for turn in &turns {
                if !first_turn {
                    writer.write_all(b",")?;
                }
                first_turn = false;
                writer.write_all(b"{\"turn\":")?;
                serde_json::to_writer(&mut writer, turn)?;
                writer.write_all(b",\"items\":[")?;
                let mut after = -1;
                let mut first_item = true;
                loop {
                    let page = self.thread_log_turn(
                        session,
                        turn["turn_id"].as_str().unwrap_or_default(),
                        after,
                    )?;
                    for item in &page.items {
                        if !first_item {
                            writer.write_all(b",")?;
                        }
                        first_item = false;
                        serde_json::to_writer(&mut writer, item)?;
                    }
                    if !page.has_more {
                        break;
                    }
                    anyhow::ensure!(page.next_after > after, "日志分页游标未前进");
                    after = page.next_after;
                }
                writer.write_all(b"]}")?;
            }
            let next = turns
                .last()
                .and_then(|turn| turn["user_turn_index"].as_i64());
            anyhow::ensure!(
                next.is_some() && (before.is_none() || next < before),
                "轮次分页游标未前进"
            );
            before = next;
        }
        writer.write_all(b"]}")?;
        writer.flush()?;
        Ok(())
    }

    pub fn thread_log_page(
        &self,
        session: &str,
        before: Option<i64>,
    ) -> Result<NativeThreadLogPage> {
        let session_info = self.get_session_info(session)?;
        let agent_id = session_info.agent_id.as_deref().unwrap_or("__default__");
        let agent_name = self
            .list_agents()?
            .into_iter()
            .find(|agent| agent.id == agent_id)
            .map(|agent| agent.name)
            .unwrap_or_else(|| agent_id.into());
        let storage = &self.state().storage;
        let (turn_total, item_total) =
            storage.get_thread_log_counts(self.user_id(), session, false)?;
        let mut turns = storage.list_thread_turns(self.user_id(), session, before, 51)?;
        let has_more = turns.len() > 50;
        turns.truncate(50);
        let next_before = turns.last().and_then(|row| row["user_turn_index"].as_i64());
        Ok(NativeThreadLogPage {
            overview: self
                .state()
                .monitor
                .get_log_overview(session)
                .unwrap_or_else(|| json!({"session_id": session})),
            username: self.get_profile()?.username,
            agent_name,
            turns,
            next_before,
            has_more,
            turn_total,
            item_total,
        })
    }

    pub fn thread_log_turn(
        &self,
        session: &str,
        turn: &str,
        after: i64,
    ) -> Result<NativeThreadLogTurn> {
        let value = self
            .state()
            .storage
            .get_thread_turn(self.user_id(), session, turn, after, 100, false)?
            .ok_or_else(|| anyhow!("线程轮次不存在"))?;
        Ok(NativeThreadLogTurn {
            items: value["items"].as_array().cloned().unwrap_or_default(),
            next_after: value["next_after"].as_i64().unwrap_or(after),
            has_more: value["has_more"] == true,
        })
    }
}
