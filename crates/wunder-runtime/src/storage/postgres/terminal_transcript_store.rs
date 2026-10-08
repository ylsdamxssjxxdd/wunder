use super::PostgresStorage;
use crate::storage::StorageLifecycle;
use anyhow::Result;
use serde_json::{json, Value};

pub(super) trait PostgresTerminalTranscriptStorage {
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

impl PostgresTerminalTranscriptStorage for PostgresStorage {
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
        // Same idempotency rule as the SQLite backend: a chunk index is owned
        // by the stream position that produced it, so replays change nothing.
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO terminal_transcript_blocks(user_id,session_id,terminal_id,chunk_index,seq,payload,created_time) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT(user_id,session_id,terminal_id,chunk_index) DO UPDATE SET seq=EXCLUDED.seq,payload=EXCLUDED.payload WHERE terminal_transcript_blocks.seq<=EXCLUDED.seq AND terminal_transcript_blocks.payload<>EXCLUDED.payload",
            &[&user_id, &session_id, &terminal_id, &chunk_index, &seq, &text, &now],
        )?;
        Ok(())
    }

    fn terminal_transcript_next_chunk_impl(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let row = conn.query_one(
            "SELECT COALESCE(MAX(chunk_index),-1)+1 FROM terminal_transcript_blocks WHERE user_id=$1 AND session_id=$2",
            &[&user_id, &session_id],
        )?;
        Ok(row.get::<_, i64>(0))
    }

    fn list_terminal_transcript_blocks_impl(
        &self,
        user_id: &str,
        session_id: &str,
        from_chunk: i64,
        limit: i64,
    ) -> Result<Vec<Value>> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT terminal_id,chunk_index,seq,payload FROM terminal_transcript_blocks WHERE user_id=$1 AND session_id=$2 AND chunk_index>=$3 ORDER BY chunk_index LIMIT $4",
            &[&user_id, &session_id, &from_chunk, &limit],
        )?;
        Ok(rows
            .into_iter()
            .map(|row| {
                json!({
                    "terminal_id": row.get::<_, String>(0),
                    "chunk_index": row.get::<_, i64>(1),
                    "seq": row.get::<_, i64>(2),
                    "text": row.get::<_, String>(3),
                })
            })
            .collect())
    }

    fn prune_terminal_transcript_impl(
        &self,
        user_id: &str,
        session_id: &str,
        keep_chunks: i64,
    ) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        conn.execute(
            "DELETE FROM terminal_transcript_blocks WHERE user_id=$1 AND session_id=$2 AND chunk_index<(SELECT COALESCE(MAX(chunk_index)+1,0)-$3 FROM terminal_transcript_blocks WHERE user_id=$4 AND session_id=$5)",
            &[&user_id, &session_id, &keep_chunks, &user_id, &session_id],
        )?;
        Ok(())
    }
}
