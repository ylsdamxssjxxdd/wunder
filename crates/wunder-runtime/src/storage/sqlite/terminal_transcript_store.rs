use super::SqliteStorage;
use crate::storage::StorageLifecycle;
use anyhow::Result;
use rusqlite::params;
use serde_json::Value;

pub(super) trait SqliteTerminalTranscriptStorage {
    fn upsert_terminal_transcript_block_impl(
        &self,
        user_id: &str,
        session_id: &str,
        terminal_id: &str,
        chunk_index: i64,
        seq: i64,
        text: &str,
    ) -> Result<()>;
    fn terminal_transcript_next_chunk_impl(&self, user_id: &str, session_id: &str) -> Result<i64>;
    fn list_terminal_transcript_blocks_impl(
        &self,
        user_id: &str,
        session_id: &str,
        from_chunk: i64,
        limit: i64,
    ) -> Result<Vec<Value>>;
    fn prune_terminal_transcript_impl(
        &self,
        user_id: &str,
        session_id: &str,
        keep_chunks: i64,
    ) -> Result<()>;
}

impl SqliteTerminalTranscriptStorage for SqliteStorage {
    fn upsert_terminal_transcript_block_impl(
        &self,
        user_id: &str,
        session_id: &str,
        terminal_id: &str,
        chunk_index: i64,
        seq: i64,
        text: &str,
    ) -> Result<()> {
        self.ensure_initialized()?;
        let now = Self::now_ts();
        // The chunk index is the identity of a durable chunk, so a re-delivery
        // of the same index only lands when it carries a newer stream position
        // and different bytes: replays stay no-ops and history never reorders.
        self.open()?.execute(
            "INSERT INTO terminal_transcript_blocks(user_id,session_id,terminal_id,chunk_index,seq,payload,created_time) VALUES(?,?,?,?,?,?,?) ON CONFLICT(user_id,session_id,terminal_id,chunk_index) DO UPDATE SET seq=excluded.seq,payload=excluded.payload WHERE terminal_transcript_blocks.seq<=excluded.seq AND terminal_transcript_blocks.payload<>excluded.payload",
            params![user_id, session_id, terminal_id, chunk_index, seq, text, now],
        )?;
        Ok(())
    }

    fn terminal_transcript_next_chunk_impl(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let next: i64 = self.open()?.query_row(
            "SELECT COALESCE(MAX(chunk_index),-1)+1 FROM terminal_transcript_blocks WHERE user_id=? AND session_id=?",
            params![user_id, session_id],
            |row| row.get(0),
        )?;
        Ok(next)
    }

    fn list_terminal_transcript_blocks_impl(
        &self,
        user_id: &str,
        session_id: &str,
        from_chunk: i64,
        limit: i64,
    ) -> Result<Vec<Value>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let mut statement = conn.prepare(
            "SELECT terminal_id,chunk_index,seq,payload FROM terminal_transcript_blocks WHERE user_id=? AND session_id=? AND chunk_index>=? ORDER BY chunk_index LIMIT ?",
        )?;
        let rows = statement
            .query_map(params![user_id, session_id, from_chunk, limit], |row| {
                Ok(serde_json::json!({
                    "terminal_id": row.get::<_, String>(0)?,
                    "chunk_index": row.get::<_, i64>(1)?,
                    "seq": row.get::<_, i64>(2)?,
                    "text": row.get::<_, String>(3)?,
                }))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn prune_terminal_transcript_impl(
        &self,
        user_id: &str,
        session_id: &str,
        keep_chunks: i64,
    ) -> Result<()> {
        self.ensure_initialized()?;
        // `MAX+1` is the next free index, so the boundary drops exactly the
        // chunks outside the newest `keep_chunks`.
        self.open()?.execute(
            "DELETE FROM terminal_transcript_blocks WHERE user_id=? AND session_id=? AND chunk_index<(SELECT COALESCE(MAX(chunk_index)+1,0)-? FROM terminal_transcript_blocks WHERE user_id=? AND session_id=?)",
            params![user_id, session_id, keep_chunks, user_id, session_id],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::SqliteStorage;
    use super::SqliteTerminalTranscriptStorage;
    use crate::storage::StorageLifecycle;

    #[test]
    fn chunks_are_idempotent_ordered_and_bounded() {
        let dir = tempfile::tempdir().expect("temp dir");
        let storage = SqliteStorage::new(
            dir.path()
                .join("transcript.db")
                .to_string_lossy()
                .into_owned(),
        );
        storage.ensure_initialized().expect("init schema");
        let (user, session) = ("transcript_user", "transcript_session");

        storage
            .upsert_terminal_transcript_block_impl(user, session, "term_a", 0, 10, "first")
            .expect("chunk 0");
        storage
            .upsert_terminal_transcript_block_impl(user, session, "term_a", 1, 20, "second")
            .expect("chunk 1");
        // Re-delivery of a chunk index with an older stream position must not
        // rewrite durable history.
        storage
            .upsert_terminal_transcript_block_impl(user, session, "term_a", 1, 15, "rewritten")
            .expect("chunk 1 replay");
        let blocks = storage
            .list_terminal_transcript_blocks_impl(user, session, 0, 100)
            .expect("list");
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0]["text"], "first");
        assert_eq!(blocks[1]["text"], "second");
        assert_eq!(blocks[1]["seq"], 20);

        // A later shell run continues the numbering instead of reusing it.
        assert_eq!(
            storage
                .terminal_transcript_next_chunk_impl(user, session)
                .expect("next chunk"),
            2
        );
        storage
            .upsert_terminal_transcript_block_impl(user, session, "term_b", 2, 5, "third")
            .expect("chunk 2");

        // Retention keeps the newest chunks and drops the head.
        storage
            .prune_terminal_transcript_impl(user, session, 1)
            .expect("prune");
        let kept = storage
            .list_terminal_transcript_blocks_impl(user, session, 0, 100)
            .expect("list after prune");
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0]["text"], "third");
        assert_eq!(kept[0]["terminal_id"], "term_b");
    }
}
