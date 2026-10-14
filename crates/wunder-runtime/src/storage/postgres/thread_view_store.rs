use super::{PgTx, PostgresStorage};
use crate::storage::{
    thread_view_store::{self as view, ViewDb},
    StorageLifecycle,
};
use anyhow::Result;
use serde_json::Value;
use tokio_postgres::types::{ToSql, Type};
use wunder_core::thread_view::*;

pub(super) const SEARCH_SCHEMA:&str="CREATE TABLE IF NOT EXISTS thread_body_search(session_id TEXT NOT NULL,user_id TEXT NOT NULL,body_id TEXT NOT NULL,revision BIGINT NOT NULL,chunk_index BIGINT NOT NULL,content TEXT NOT NULL,document TSVECTOR NOT NULL,PRIMARY KEY(session_id,body_id,revision,chunk_index)); CREATE INDEX IF NOT EXISTS idx_thread_body_search_document ON thread_body_search USING GIN(document);CREATE INDEX IF NOT EXISTS idx_thread_body_search_scope ON thread_body_search(user_id,session_id);";
pub(super) struct Db<'a, 'b>(pub &'a mut PgTx<'b>);
fn sql(s: &str) -> String {
    let mut n = 0;
    s.chars()
        .map(|c| {
            if c == '?' {
                n += 1;
                format!("${n}")
            } else {
                c.to_string()
            }
        })
        .collect()
}
fn params(p: &[Value]) -> Vec<Box<dyn ToSql + Sync + Send>> {
    p.iter()
        .map(|v| -> Box<dyn ToSql + Sync + Send> {
            match v {
                Value::Null => Box::new(None::<String>),
                Value::Number(n) if n.is_i64() => Box::new(n.as_i64().unwrap()),
                Value::Number(n) => Box::new(n.as_f64().unwrap_or(0.0)),
                Value::Bool(b) => Box::new(*b),
                Value::String(s) => Box::new(s.clone()),
                _ => Box::new(v.to_string()),
            }
        })
        .collect()
}
impl ViewDb for Db<'_, '_> {
    fn is_postgres(&self) -> bool {
        true
    }
    fn json_text(&self, expression: &str, field: &str) -> String {
        format!("({expression})::jsonb->>'{field}'")
    }
    fn text_bytes(&self, expression: &str) -> String {
        format!("octet_length({expression})")
    }
    fn index_chunk(
        &mut self,
        user: &str,
        session: &str,
        id: &str,
        revision: i64,
        index: i64,
        content: &str,
    ) -> Result<()> {
        if index == 0 {
            self.0.execute(
                "DELETE FROM thread_body_search WHERE session_id=$1 AND user_id=$2 AND body_id=$3",
                &[&session, &user, &id],
            )?;
        }
        self.0.execute("INSERT INTO thread_body_search(session_id,user_id,body_id,revision,chunk_index,content,document) VALUES($1,$2,$3,$4,$5,$6,to_tsvector('simple',$6)) ON CONFLICT DO NOTHING",&[&session,&user,&id,&revision,&index,&content])?;
        Ok(())
    }
    fn search(&mut self, user: &str, session: &str, query: &str) -> Result<ThreadSearchPage> {
        let rows=self.0.query("SELECT s.body_id,s.revision,s.chunk_index,s.content FROM thread_body_search s JOIN thread_body_manifests m ON m.session_id=s.session_id AND m.body_id=s.body_id AND m.revision=s.revision JOIN thread_items i ON i.session_id=s.session_id AND i.item_id=split_part(s.body_id,'|',1) AND i.visibility='user' WHERE s.session_id=$1 AND s.user_id=$2 AND s.document @@ plainto_tsquery('simple',$3) LIMIT 21",&[&session,&user,&query])?;
        let more = rows.len() > 20;
        Ok(ThreadSearchPage {
            has_more: more,
            hits: rows
                .into_iter()
                .take(20)
                .map(|r| {
                    let id: String = r.get(0);
                    let content: String = r.get(3);
                    ThreadSearchHit {
                        item_id: id.split('|').next().unwrap_or("").into(),
                        body: BodyRef {
                            body_id: id,
                            revision: r.get(1),
                            bytes: content.len(),
                        },
                        cursor: r.get(2),
                        snippet: preview(&content, 256),
                    }
                })
                .collect(),
        })
    }
    fn execute(&mut self, s: &str, p: &[Value]) -> Result<u64> {
        let p = params(p);
        let refs = p
            .iter()
            .map(|v| v.as_ref() as &(dyn ToSql + Sync))
            .collect::<Vec<_>>();
        self.0.execute(&sql(s), &refs)
    }
    fn query(&mut self, s: &str, p: &[Value]) -> Result<Vec<Vec<Value>>> {
        let p = params(p);
        let refs = p
            .iter()
            .map(|v| v.as_ref() as &(dyn ToSql + Sync))
            .collect::<Vec<_>>();
        Ok(self
            .0
            .query(&sql(s), &refs)?
            .into_iter()
            .map(|r| {
                r.columns()
                    .iter()
                    .enumerate()
                    .map(|(i, c)| match *c.type_() {
                        Type::INT8 => r
                            .get::<_, Option<i64>>(i)
                            .map(Value::from)
                            .unwrap_or(Value::Null),
                        Type::INT4 => r
                            .get::<_, Option<i32>>(i)
                            .map(Value::from)
                            .unwrap_or(Value::Null),
                        Type::FLOAT8 => r
                            .get::<_, Option<f64>>(i)
                            .map(|v| serde_json::json!(v))
                            .unwrap_or(Value::Null),
                        _ => r
                            .get::<_, Option<String>>(i)
                            .map(Value::from)
                            .unwrap_or(Value::Null),
                    })
                    .collect()
            })
            .collect())
    }
}
impl PostgresStorage {
    pub(super) fn view_search(&self, u: &str, s: &str, q: &str) -> Result<ThreadSearchPage> {
        self.ensure_initialized()?;
        let mut c = self.conn()?;
        let mut tx = c.transaction()?;
        tx.batch_execute("SET LOCAL statement_timeout='1500ms'")?;
        let p = Db(&mut tx).search(u, s, q)?;
        tx.commit()?;
        Ok(p)
    }
    pub(super) fn view_pending(&self) -> Result<Vec<(String, String, i64)>> {
        self.ensure_initialized()?;
        let mut c = self.conn()?;
        let mut tx = c.transaction()?;
        tx.batch_execute("SET LOCAL statement_timeout='1500ms'; SET LOCAL lock_timeout='100ms'")?;
        if !tx
            .query_one(
                "SELECT pg_try_advisory_xact_lock(1937202,hashtext(current_schema()))",
                &[],
            )?
            .get::<_, bool>(0)
        {
            return Ok(Vec::new());
        }
        view::discover_rebuilds(&mut Db(&mut tx))?;
        let rows = view::pending_rebuilds(&mut Db(&mut tx))?;
        tx.commit()?;
        Ok(rows)
    }

    pub(super) fn view_page(
        &self,
        u: &str,
        s: &str,
        k: &str,
        b: Option<i64>,
        r: Option<&str>,
        l: usize,
    ) -> Result<ThreadViewPage> {
        self.ensure_initialized()?;
        let mut c = self.conn()?;
        let mut tx = c.transaction()?;
        tx.batch_execute("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY; SET LOCAL statement_timeout='1500ms'")?;
        let p = view::page(&mut Db(&mut tx), u, s, k, b, r, l)?;
        tx.commit()?;
        Ok(p)
    }
    pub(super) fn view_changes(&self, u: &str, s: &str, a: i64) -> Result<ThreadViewChanges> {
        self.ensure_initialized()?;
        let mut c = self.conn()?;
        let mut tx = c.transaction()?;
        tx.batch_execute("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY; SET LOCAL statement_timeout='1500ms'")?;
        let p = view::changes(&mut Db(&mut tx), u, s, a)?;
        tx.commit()?;
        Ok(p)
    }
    pub(super) fn view_body(
        &self,
        u: &str,
        s: &str,
        id: &str,
        r: i64,
        cursor: i64,
    ) -> Result<ThreadBodyPage> {
        self.ensure_initialized()?;
        let mut c = self.conn()?;
        let mut tx = c.transaction()?;
        tx.batch_execute("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY; SET LOCAL statement_timeout='1500ms'")?;
        let p = view::body(&mut Db(&mut tx), u, s, id, r, cursor)?;
        tx.commit()?;
        Ok(p)
    }
    pub(super) fn view_rebuild(&self, u: &str, s: &str, a: i64) -> Result<(i64, bool)> {
        self.ensure_initialized()?;
        let mut c = self.conn()?;
        let mut tx = c.transaction()?;
        tx.batch_execute("SET LOCAL statement_timeout='1500ms'; SET LOCAL lock_timeout='100ms'")?;
        // The normal writer also locks the thread envelope. Serialize the
        // backfill with it so revision checks and statistics cannot race.
        tx.query_opt(
            "SELECT user_id FROM thread_logs WHERE session_id=$1 AND user_id=$2 FOR UPDATE NOWAIT",
            &[&s, &u],
        )?;
        if let Some(row) = tx.query_opt("SELECT rebuild_after,projection_state FROM thread_view_heads WHERE session_id=$1 AND user_id=$2", &[&s,&u])? {
            let current: i64 = row.get(0);
            let more = row.get::<_,String>(1) == "rebuilding";
            // Another replica or a live writer can advance the projection
            // while this page waits. Never replay an obsolete cursor.
            if more && current != a { return Ok((current,true)); }
        }
        let p = view::rebuild(&mut Db(&mut tx), u, s, a)?;
        tx.commit()?;
        Ok(p)
    }
}
