//! Isolated real HTTP/WS peer for release-browser display regression.
//! All state lives in a fresh temporary directory; no repository config is read.
use std::sync::Arc;
use anyhow::Result;
use axum::{extract::State, routing::post, Json, Router};
use serde_json::{json, Value};
use tower_http::services::{ServeDir, ServeFile};
use wunder_server::{api,config::Config,config_store::ConfigStore,state::{AppState,AppStateInitOptions},storage::ChatSessionRecord};

#[tokio::main]
async fn main()->Result<()> {
    let dir=tempfile::tempdir()?;
    let mut config=Config::default();
    config.storage.backend="sqlite".into();
    config.storage.db_path=dir.path().join("fixture.db").to_string_lossy().into();
    config.workspace.root=dir.path().join("fixture-workspace").to_string_lossy().into();
    config.skills.enabled.clear();
    let store=ConfigStore::new(dir.path().join("fixture.yaml"));
    let saved=config.clone();store.update(|c|*c=saved.clone()).await?;
    let state=Arc::new(AppState::new_with_options(store,config,AppStateInitOptions::cli_default().with_start_thread_runtime(false))?);
    let user=state.user_store.create_user("fixture-user",None,"fixture-password",None,None,vec!["user".into()],"active",false)?;
    state.storage.upsert_chat_session(&ChatSessionRecord{session_id:"fixture-thread".into(),user_id:user.user_id.clone(),title:"Fixture conversation".into(),status:"active".into(),created_at:1.,updated_at:1.,last_message_at:1.,agent_id:None,workspace_id:None,tool_overrides:vec![],parent_session_id:None,parent_message_id:None,spawn_label:None,spawned_by:None})?;
    let mut conn=rusqlite::Connection::open(dir.path().join("fixture.db"))?;
    let tx=conn.transaction()?;
    tx.execute("INSERT INTO thread_logs(session_id,user_id,latest_user_turn,latest_change_seq,created_time,updated_time) VALUES('fixture-thread',?,200,20000,1,1)",[&user.user_id])?;
    // The disk benchmark covers 500k rows; the browser fixture keeps startup
    // bounded while still exercising the real API/WS path with 2,000 items.
    for round in 1..=20i64 {
        let root=format!("fixture-turn-{round}");
        tx.execute("INSERT INTO thread_turns(session_id,turn_id,user_id,root_turn_id,trigger_kind,user_turn_index,status,summary,payload,created_time,updated_time) VALUES('fixture-thread',?, ?,?,'user',?,'completed','','{}',?,?)",rusqlite::params![root,user.user_id,root,round,round as f64,round as f64])?;
        for index in 0..100i64 {
            let (id,kind,content)=if index==0 {(format!("{root}:user"),"user_message",format!("Fixture input {round}"))}
                else if index==99 {(format!("{root}:text-1"),"assistant_message",format!("Fixture answer {round}"))}
                else {(format!("{root}:tool-{index}"),"tool_call","Fixture tool".into())};
            let payload=json!({"content":content,"tool_name":"fixture_tool","args":{"value":"fixture"}}).to_string();
            tx.execute("INSERT INTO thread_items(session_id,item_id,turn_id,root_turn_id,user_id,visibility,item_index,kind,status,payload,created_time,updated_time,created_seq) VALUES('fixture-thread',?,?,?,?,'user',?,?,'completed',?,?,?,?)",rusqlite::params![id,root,root,user.user_id,index,kind,payload,round as f64,round as f64,(round-1)*100+index+1])?;
        }
    }
    tx.execute("INSERT INTO thread_view_heads(session_id,user_id,projection_state) VALUES('fixture-thread',?,'rebuilding') ON CONFLICT DO NOTHING",[&user.user_id])?;
    tx.commit()?;drop(conn);
    let mut after=0;loop{let(next,more)=state.storage.rebuild_thread_view_page(&user.user_id,"fixture-thread",after)?;after=next;if !more{break;}}
    let dist=std::path::PathBuf::from("frontend/dist");
    let app=api::build_router(state.clone()).merge(Router::new().route("/fixture/append",post(append)).with_state(state))
        .fallback_service(ServeDir::new(&dist).not_found_service(ServeFile::new(dist.join("index.html"))));
    let listener=tokio::net::TcpListener::bind("127.0.0.1:4176").await?;
    println!("fixture ready: 2000 persisted items");
    axum::serve(listener,app).await?;drop(dir);Ok(())
}
async fn append(State(state):State<Arc<AppState>>)->Json<Value>{
    let user=state.user_store.get_user_by_username("fixture-user").unwrap().unwrap();
    let turn=state.storage.accept_thread_turn(&user.user_id,"fixture-thread",&json!({"content":"Fixture live input","client_message_id":"fixture-live"})).unwrap();
    let id=turn["turn_id"].as_str().unwrap();
    state.storage.append_thread_item(&user.user_id,&json!({"session_id":"fixture-thread","turn_id":id,"item_id":format!("{id}:text-1"),"kind":"assistant_message","status":"completed","content":"Fixture live answer"})).unwrap();
    state.storage.update_thread_turn(&user.user_id,"fixture-thread",id,"completed","",&json!({})).unwrap();
    Json(json!({"ok":true}))
}
