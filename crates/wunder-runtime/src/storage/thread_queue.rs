//! Atomic hand-off between the durable task queue and the thread log.
use super::thread_view_store::ViewDb;
use anyhow::Result;
use serde_json::{json, Value};

fn one(db: &mut impl ViewDb, sql: &str, params: &[Value]) -> Result<Option<Vec<Value>>> {
    Ok(db.query(sql, params)?.into_iter().next())
}
fn int(value: &Value) -> i64 {
    value.as_i64().unwrap_or(0)
}

/// Called inside the task insertion transaction: a pending task and its
/// composer ownership become visible together, including the replay cursor.
pub(crate) fn park_input(
    db: &mut impl ViewDb,
    user: &str,
    session: &str,
    request: &Value,
) -> Result<()> {
    let Some(wait) = request.get("__queue_wait") else {
        return Ok(());
    };
    let Some(turn) = request
        .pointer("/config_overrides/__thread_log_turn_id")
        .and_then(Value::as_str)
    else {
        return Ok(());
    };
    let payload = json!({"queued_input":wait});
    let now = chrono::Utc::now().timestamp_millis() as f64 / 1000.;
    let changed=db.execute("UPDATE thread_turns SET status='queued',payload=?,updated_time=? WHERE user_id=? AND session_id=? AND turn_id=? AND status='accepted'", &[json!(payload.to_string()),json!(now),json!(user),json!(session),json!(turn)])?;
    if changed == 0 {
        return Ok(());
    }
    let turn_row=one(db,"SELECT root_turn_id,trigger_kind,user_turn_index FROM thread_turns WHERE user_id=? AND session_id=? AND turn_id=?",&[json!(user),json!(session),json!(turn)])?.unwrap();
    db.execute("UPDATE thread_logs SET latest_change_seq=latest_change_seq+1,updated_time=? WHERE user_id=? AND session_id=?",&[json!(now),json!(user),json!(session)])?;
    let seq = one(
        db,
        "SELECT latest_change_seq FROM thread_logs WHERE user_id=? AND session_id=?",
        &[json!(user), json!(session)],
    )?
    .map(|row| int(&row[0]))
    .unwrap_or(0);
    let change = json!({"turn_id":turn,"root_turn_id":turn_row[0],"trigger_kind":turn_row[1],"user_round":turn_row[2],"status":"queued","queued_input":wait});
    db.execute("INSERT INTO thread_log_changes(session_id,change_seq,user_id,change_type,turn_id,revision,payload,created_time) VALUES(?,?,?,'turn_upsert',?,?,?,?)",&[json!(session),json!(seq),json!(user),json!(turn),json!(seq),json!(change.to_string()),json!(now)])?;
    super::thread_view_store::project_turn(db, user, session, turn)
}
