//! Thread trajectory data access for the desktop shell. The trajectory page
//! reads the same storage snapshot the web shell's snapshot endpoint serves;
//! no separate pagination or export path is kept for it.
use super::NativeDesktop;
use anyhow::Result;
use serde_json::Value;
use std::sync::Arc;

impl NativeDesktop {
    /// Hydrate only the selected trajectory row. Bodies remain chunked in
    /// storage; enforce both byte and page bounds before decoding the payload.
    pub fn thread_trajectory_detail(
        &self,
        session: &str,
        body_id: &str,
        revision: i64,
    ) -> Result<Value> {
        self.open_session(session)?;
        read_detail(|cursor| {
            self.state().storage.thread_body_page(
                self.user_id(),
                session,
                body_id,
                revision,
                cursor,
            )
        })
    }
    /// Bounded full-text search used by the native trajectory view.
    pub fn thread_trajectory_search(
        &self,
        session: &str,
        query: &str,
    ) -> Result<serde_json::Value> {
        self.open_session(session)?;
        let page = self
            .state()
            .storage
            .thread_view_search(self.user_id(), session, query)?;
        Ok(serde_json::to_value(page)?)
    }

    /// Read the chunk containing a trajectory search hit without loading the
    /// complete message body into the UI model.
    pub fn thread_trajectory_body(
        &self,
        session: &str,
        body_id: &str,
        revision: i64,
        cursor: i64,
    ) -> Result<serde_json::Value> {
        self.open_session(session)?;
        let page = self.state().storage.thread_body_page(
            self.user_id(),
            session,
            body_id,
            revision,
            cursor,
        )?;
        Ok(serde_json::to_value(page)?)
    }

    /// Full user-visible snapshot of one thread: turns, user-visible items and
    /// text blocks, read directly from storage in one bounded query set.
    pub fn thread_snapshot(&self, session: &str) -> Result<Value> {
        self.open_session(session)?;
        self.state()
            .storage
            .thread_snapshot(self.user_id(), session)
    }

    /// Bounded presentation trajectory used by the native shell.  The web
    /// shell and the desktop shell now read the same durable projection: the
    /// rows are keyset paged while `tiles` contains the full, bounded overview
    /// pyramid.  This keeps a long thread from being reconstructed by replaying
    /// its raw event log on the UI thread.
    pub fn thread_trajectory_page(
        &self,
        session: &str,
        before: Option<i64>,
        limit: usize,
    ) -> Result<Value> {
        self.open_session(session)?;
        self.thread_trajectory_page_window(session, before, None, limit)
    }

    /// Read the next newer window after a bounded trajectory page.  Keeping
    /// this separate from the historical `thread_trajectory_page` signature
    /// lets callers add reverse paging without breaking existing façade users.
    pub fn thread_trajectory_page_after(
        &self,
        session: &str,
        after: Option<i64>,
        limit: usize,
    ) -> Result<Value> {
        self.open_session(session)?;
        self.thread_trajectory_page_window(session, None, after, limit)
    }

    fn thread_trajectory_page_window(
        &self,
        session: &str,
        before: Option<i64>,
        after: Option<i64>,
        limit: usize,
    ) -> Result<Value> {
        anyhow::ensure!(
            before.is_none() || after.is_none(),
            "ambiguous trajectory cursor"
        );
        let page = self.state().storage.thread_view_window(
            self.user_id(),
            session,
            "trajectory",
            before,
            after,
            None,
            limit,
        )?;
        Ok(serde_json::to_value(page)?)
    }

    /// Stream the same bounded NDJSON export used by the authenticated web
    /// client into a user-selected file. The producer remains paged in the
    /// storage layer, so exporting a large thread never materializes it in
    /// the Slint/UI thread.
    pub fn export_thread_log(&self, session: &str, path: &std::path::Path) -> Result<()> {
        use std::io::Write;
        let storage = Arc::clone(&self.state().storage);
        let owner = self.user_id().to_owned();
        let session = session.to_owned();
        let path = path.to_owned();
        self.runtime.block_on(async move {
            tokio::task::spawn_blocking(move || {
                let mut file = std::fs::File::create(path)?;
                writeln!(file, "{}", serde_json::json!({"record_type":"thread_meta","export_schema_version":5,"session_id":session}))?;
                let mut before = None;
                loop {
                    let turns = storage.list_thread_turns(&owner, &session, before, 50)?;
                    if turns.is_empty() { break; }
                    before = turns.last().and_then(|v| v.get("user_turn_index")).and_then(Value::as_i64);
                    for turn in turns {
                        writeln!(file, "{}", serde_json::json!({"record_type":"turn","turn":turn}))?;
                        let turn_id = turn.get("turn_id").and_then(Value::as_str).unwrap_or_default();
                        let mut after = -1;
                        loop {
                            let page = storage.get_thread_turn(&owner, &session, turn_id, after, 50, false)?
                                .ok_or_else(|| anyhow::anyhow!("turn deleted during export"))?;
                            for item in page.get("items").and_then(Value::as_array).into_iter().flatten() {
                                writeln!(file, "{}", serde_json::json!({"record_type":"item","item":item}))?;
                            }
                            if page.get("has_more") != Some(&Value::Bool(true)) { break; }
                            after = page.get("next_after").and_then(Value::as_i64)
                                .ok_or_else(|| anyhow::anyhow!("missing item cursor"))?;
                        }
                    }
                }
                writeln!(file, "{}", serde_json::json!({"record_type":"export_complete"}))?;
                Ok::<(), anyhow::Error>(())
            }).await??;
            Ok(())
        })
    }
}

fn read_detail(
    mut read: impl FnMut(i64) -> Result<wunder_server::stable_core::thread_view::ThreadBodyPage>,
) -> Result<Value> {
    let mut cursor = 0;
    let mut text = String::new();
    for _ in 0..256 {
        let page = read(cursor)?;
        for chunk in page.chunks {
            anyhow::ensure!(
                text.len() + chunk.len() <= 2 * 1024 * 1024,
                "trajectory detail exceeds display limit; export the complete log"
            );
            text.push_str(&chunk);
        }
        if !page.has_more {
            if text.is_empty() {
                return Ok(Value::Null);
            }
            return Ok(serde_json::from_str(&text)?);
        }
        let next = page
            .next_cursor
            .ok_or_else(|| anyhow::anyhow!("missing trajectory body cursor"))?;
        anyhow::ensure!(next > cursor, "trajectory body cursor did not advance");
        cursor = next;
    }
    anyhow::bail!("trajectory detail exceeds page limit; export the complete log")
}

#[cfg(test)]
mod tests {
    use super::*;
    use wunder_server::stable_core::thread_view::ThreadBodyPage;

    #[test]
    fn detail_uses_returned_cursors_and_preserves_summary() {
        let mut visited = Vec::new();
        let value = read_detail(|cursor| {
            visited.push(cursor);
            Ok(ThreadBodyPage {
                chunks: vec![if cursor == 0 {
                    "{\"summary_text\":\"Fixture "
                } else {
                    "retained context.\"}"
                }
                .into()],
                has_more: cursor == 0,
                next_cursor: (cursor == 0).then_some(16),
                ..Default::default()
            })
        })
        .unwrap();
        assert_eq!(visited, [0, 16]);
        assert_eq!(value["summary_text"], "Fixture retained context.");
        assert_eq!(
            read_detail(|_| Ok(ThreadBodyPage::default())).unwrap(),
            Value::Null
        );
        assert!(read_detail(|_| Ok(ThreadBodyPage {
            has_more: true,
            next_cursor: Some(0),
            ..Default::default()
        }))
        .is_err());
        assert!(read_detail(|_| Ok(ThreadBodyPage {
            chunks: vec!["x".repeat(2 * 1024 * 1024 + 1)],
            ..Default::default()
        }))
        .is_err());
    }

    #[cfg(feature = "sqlite-storage")]
    #[test]
    fn stored_compaction_detail_survives_chunked_native_read() {
        use wunder_server::storage::{SqliteStorage, ThreadLogStore};
        let directory = tempfile::tempdir().unwrap();
        let storage =
            SqliteStorage::new(directory.path().join("fixture.db").to_string_lossy().into());
        let accepted = storage
            .accept_thread_turn(
                "fixture-user",
                "fixture-thread",
                &serde_json::json!({"content":"Fixture input"}),
            )
            .unwrap();
        let turn = accepted["turn_id"].as_str().unwrap();
        let summary = "Fixture retained context.\n".repeat(2400);
        storage
            .append_thread_item(
                "fixture-user",
                &serde_json::json!({
                    "session_id":"fixture-thread", "turn_id":turn, "item_id":"fixture-compaction",
                    "kind":"compaction", "status":"completed", "content":"history",
                    "summary_text":summary
                }),
            )
            .unwrap();
        let page = storage
            .thread_view_trajectory_page("fixture-user", "fixture-thread", None, None, 20)
            .unwrap();
        assert!(!page.timeline.is_empty());
        let body = page
            .trajectory
            .iter()
            .find(|row| row.item_id == "fixture-compaction")
            .unwrap()
            .detail
            .as_ref()
            .unwrap();
        let mut pages = 0;
        let value = read_detail(|cursor| {
            pages += 1;
            storage.thread_body_page(
                "fixture-user",
                "fixture-thread",
                &body.body_id,
                body.revision,
                cursor,
            )
        })
        .unwrap();
        assert_eq!(value["summary_text"], summary);
        assert!(pages >= 1);
    }
}
