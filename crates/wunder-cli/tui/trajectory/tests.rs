use super::super::snapshot_tests::{fixture_app, TempRoot};
use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{backend::TestBackend, Terminal};
use serde_json::json;

fn rows(start: i64, end: i64) -> Vec<TrajectoryViewRow> {
    (start..end)
        .map(|n| TrajectoryViewRow {
            item_id: format!("fixture-item-{n}"),
            root_turn_id: format!("root-{}", n / 10),
            turn_id: format!("root-{}", n / 10),
            user_round: n / 10,
            created_seq: n,
            kind: "assistant_message".into(),
            status: "completed".into(),
            started_at: n as f64,
            completed_at: Some(n as f64 + 0.5),
            summary: format!("record {n}"),
            ..Default::default()
        })
        .collect()
}
#[test]
fn trajectory_window_preserves_anchor_and_reads_evicted_directions() {
    let mut v = TrajectoryView::new("fixture-thread".into());
    v.apply_page(
        Direction::Latest,
        ThreadViewPage {
            trajectory: rows(100, 700),
            has_more: true,
            ..Default::default()
        },
    );
    v.selected = 0;
    let id = v.row().unwrap().item_id.clone();
    v.apply_page(
        Direction::Older,
        ThreadViewPage {
            trajectory: rows(20, 100),
            has_more: true,
            ..Default::default()
        },
    );
    assert_eq!(v.rows.len(), MAX_ROWS);
    assert_eq!(v.rows[0].created_seq, 20);
    assert_eq!(v.row().unwrap().item_id, id);
    assert!(v.newer);
    v.apply_page(
        Direction::Newer,
        ThreadViewPage {
            trajectory: rows(620, 700),
            has_newer: false,
            ..Default::default()
        },
    );
    assert_eq!(v.rows[0].created_seq, 100);
    assert!(!v.newer);
    assert!(v.older);
}
#[test]
fn trajectory_timeline_respects_time_gaps_and_model_lane() {
    assert_eq!(coordinate(10., (0., 100.), 101), 10);
    assert_eq!(coordinate(90., (0., 100.), 101), 90);
    assert_eq!(lane("model_call"), 1);
    let mut v = TrajectoryView::new("fixture-thread".into());
    v.rows = rows(100, 110);
    v.tiles = vec![TrajectoryTile {
        start: 10.,
        end: 200.,
        bucket: 0,
        count: 10,
        ..Default::default()
    }];
    v.duration = true;
    assert_eq!(v.domain(), (10., 200.));
    v.zoom = 2;
    assert!((v.domain().1 - v.domain().0 - 47.5).abs() < 0.01);
}
#[test]
fn trajectory_folding_keeps_turn_anchor_and_context_records() {
    let mut v = TrajectoryView::new("fixture-thread".into());
    v.rows = rows(0, 30);
    v.selected = 21;
    v.fold_turns = true;
    v.reconcile();
    assert_eq!(v.selected, 20);
    assert_eq!(v.visible().len(), 3);
    v.fold_turns = false;
    v.fold_calls = true;
    v.rows[21].kind = "tool_call".into();
    v.rows[22].kind = "tool_call".into();
    v.rows[23].kind = "approval".into();
    let visible = v.visible();
    assert!(visible.contains(&21));
    assert!(!visible.contains(&22));
    assert!(visible.contains(&23));
}
async fn settle(app: &mut TuiApp) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            app.poll_trajectory_results();
            if app
                .trajectory_view
                .as_ref()
                .is_none_or(|v| v.pending.is_none())
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("trajectory I/O completed");
    assert!(
        app.trajectory_view
            .as_ref()
            .is_none_or(|v| v.error.is_none()),
        "{:?}",
        app.trajectory_view.as_ref().and_then(|v| v.error.as_ref())
    );
}
async fn key(app: &mut TuiApp, code: KeyCode) {
    app.on_key(KeyEvent::new(code, KeyModifiers::NONE))
        .await
        .unwrap();
    settle(app).await;
}
fn render(app: &mut TuiApp, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| crate::tui::ui::draw(frame, app))
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>()
}
#[tokio::test]
async fn trajectory_storage_keyboard_search_detail_export_and_frames() {
    let root = TempRoot::new("trajectory");
    let mut app = fixture_app(root.path()).await;
    let store = app.runtime.state.storage.clone();
    let user = app.runtime.user_id.clone();
    let session = app.session_id.clone();
    let turn = store
        .accept_thread_turn(&user, &session, &json!({"content":"fixture request"}))
        .unwrap();
    let turn_id = turn["turn_id"].as_str().unwrap();
    let content = format!("{}\nneedle-final", "fixture line\n".repeat(1400));
    store.append_thread_item(&user, &json!({"session_id":session,"turn_id":turn_id,"item_id":"fixture-answer", "kind":"assistant_message","status":"completed","content":content})).unwrap();
    app.open_trajectory_view(None).await.unwrap();
    settle(&mut app).await;
    assert!(app
        .trajectory_view
        .as_ref()
        .unwrap()
        .rows
        .iter()
        .any(|r| r.item_id == "fixture-answer"));
    let wide = render(&mut app, 140, 34);
    assert!(wide.contains("Trajectory"));
    assert!(wide.contains("Ledger"));
    assert!(wide.contains("Details"));
    key(&mut app, KeyCode::Enter).await;
    let view = app.trajectory_view.as_ref().unwrap();
    assert_eq!(view.root.as_deref(), Some(turn_id));
    assert_eq!(view.detail.value["content"], content);
    key(&mut app, KeyCode::Tab).await;
    key(&mut app, KeyCode::Char('i')).await;
    let narrow = render(&mut app, 70, 24);
    assert!(narrow.contains("Details"));
    key(&mut app, KeyCode::PageDown).await;
    assert_eq!(app.trajectory_view.as_ref().unwrap().detail_scroll, 8);
    key(&mut app, KeyCode::End).await;
    let tail = render(&mut app, 70, 24);
    assert!(tail.contains("needle-final"));
    key(&mut app, KeyCode::Esc).await;
    key(&mut app, KeyCode::Char('/')).await;
    app.on_paste("needle-final".into());
    key(&mut app, KeyCode::Enter).await;
    let view = app.trajectory_view.as_ref().unwrap();
    assert!(view
        .hits
        .as_ref()
        .unwrap()
        .iter()
        .any(|r| r.item_id == "fixture-answer"));
    assert!(view.detail.value["content"]
        .as_str()
        .unwrap()
        .contains("needle-final"));
    for (width, height) in [(1, 1), (20, 8), (80, 24), (140, 34)] {
        render(&mut app, width, height);
    }
    let path = root.path().join("fixture-export.jsonl");
    crate::trajectory::export(&app.runtime, session, path.clone())
        .await
        .unwrap();
    let exported = std::fs::read_to_string(&path).unwrap();
    let records: Vec<serde_json::Value> = exported
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(records.last().unwrap()["record_type"], "export_complete");
    assert!(exported.contains("needle-final"));
    assert!(
        crate::trajectory::export(&app.runtime, app.session_id.clone(), path)
            .await
            .is_err()
    );
    app.trajectory_view = None;
}

#[tokio::test]
async fn trajectory_reads_completed_content_and_exports_without_private_fields() {
    let root = TempRoot::new("trajectory-blocks");
    let app = fixture_app(root.path()).await;
    let store = app.runtime.state.storage.clone();
    let user = &app.runtime.user_id;
    let session = &app.session_id;
    let turn = store
        .accept_thread_turn(user, session, &json!({"content":"fixture request"}))
        .unwrap();
    store.append_thread_item(user, &json!({"session_id":session,"turn_id":turn["turn_id"],"item_id":"fixture-blocked-answer","kind":"assistant_message","status":"completed","content":"streamed fixture answer","reasoning":"streamed fixture reasoning","model_content":"fixture-private-field"})).unwrap();
    let row =
        crate::trajectory::public_item_row(store.as_ref(), user, session, "fixture-blocked-answer")
            .unwrap();
    let detail = crate::trajectory::load_detail(store.as_ref(), user, session, &row).unwrap();
    assert_eq!(detail.value["content"], "streamed fixture answer");
    assert_eq!(detail.value["reasoning"], "streamed fixture reasoning");
    assert!(detail.value.get("model_content").is_none());
    let path = root.path().join("fixture-blocks.jsonl");
    crate::trajectory::export(&app.runtime, session.clone(), path.clone())
        .await
        .unwrap();
    let exported = std::fs::read_to_string(path).unwrap();
    assert!(exported.contains("streamed fixture answer"));
    assert!(exported.contains("streamed fixture reasoning"));
    assert!(!exported.contains("fixture-private-field"));
    assert!(!exported.contains("item_block"));
}

#[tokio::test]
async fn trajectory_refresh_catches_every_page_and_recovers_revisions() {
    let root = TempRoot::new("trajectory-refresh");
    let mut app = fixture_app(root.path()).await;
    let store = app.runtime.state.storage.clone();
    let owner = app.runtime.user_id.clone();
    let session = app.session_id.clone();
    let turn = store
        .accept_thread_turn(&owner, &session, &json!({"content":"fixture request"}))
        .unwrap();
    let turn_id = turn["turn_id"].as_str().unwrap();
    store.append_thread_item(&owner, &json!({"session_id":session,"turn_id":turn_id,"item_id":"fixture-running","kind":"assistant_message","status":"running","content":"partial"})).unwrap();
    app.open_trajectory_view(None).await.unwrap();
    settle(&mut app).await;
    for i in 0..105 {
        store.append_thread_item(&owner, &json!({"session_id":session,"turn_id":turn_id,"item_id":format!("fixture-new-{i}"),"kind":"tool_call","status":"completed","tool":"fixture_tool","args":{"index":i},"result":"fixture result"})).unwrap();
    }
    key(&mut app, KeyCode::Char('r')).await;
    assert!(app.trajectory_view.as_ref().unwrap().newer);
    app.trajectory_view.as_mut().unwrap().refreshed = Instant::now() - REFRESH;
    settle(&mut app).await;
    let view = app.trajectory_view.as_ref().unwrap();
    assert!(!view.newer);
    assert_eq!(
        view.rows
            .iter()
            .filter(|r| r.item_id.starts_with("fixture-new-"))
            .count(),
        105
    );
    let running_index = view
        .rows
        .iter()
        .position(|r| r.item_id == "fixture-running")
        .unwrap();
    app.trajectory_view.as_mut().unwrap().selected = running_index;
    app.trajectory_view.as_mut().unwrap().live = false;
    key(&mut app, KeyCode::Char('i')).await;
    store.append_thread_item(&owner, &json!({"session_id":session,"turn_id":turn_id,"item_id":"fixture-running","kind":"assistant_message","status":"completed","content":"final revision"})).unwrap();
    key(&mut app, KeyCode::Char('r')).await;
    let view = app.trajectory_view.as_ref().unwrap();
    assert_eq!(view.row().unwrap().item_id, "fixture-running");
    assert_eq!(view.row().unwrap().status, "completed");
    assert_eq!(view.detail.value["content"], "final revision");
}
#[tokio::test]
async fn trajectory_reader_discards_stale_detail_and_surfaces_errors() {
    let root = TempRoot::new("trajectory-stale");
    let mut app = fixture_app(root.path()).await;
    let mut view = TrajectoryView::new("fixture-thread".into());
    view.rows = rows(0, 3);
    view.live = false;
    view.detail_key = Some((view.rows[0].item_id.clone(), 0));
    let (tx, receiver) = oneshot::channel();
    let detail = Detail {
        value: json!("stale body"),
        ..Default::default()
    };
    tx.send(Ok(Reply::Detail(view.rows[0].item_id.clone(), 0, detail)))
        .ok();
    view.selected = 1;
    view.pending = Some(Pending {
        receiver,
        task: tokio::spawn(async {}),
    });
    app.trajectory_view = Some(view);
    settle(&mut app).await;
    assert_ne!(
        app.trajectory_view.as_ref().unwrap().detail.value,
        json!("stale body")
    );
    let (tx, receiver) = oneshot::channel();
    tx.send(Err(anyhow::anyhow!("fixture read failure"))).ok();
    app.trajectory_view.as_mut().unwrap().pending = Some(Pending {
        receiver,
        task: tokio::spawn(async {}),
    });
    app.poll_trajectory_results();
    assert_eq!(
        app.trajectory_view.as_ref().unwrap().error.as_deref(),
        Some("fixture read failure")
    );
    assert!(render(&mut app, 100, 24).contains("fixture read failure"));
    key(&mut app, KeyCode::Char('r')).await;
    assert!(app.trajectory_view.as_ref().unwrap().error.is_none());
}

#[test]
fn trajectory_degraded_page_keeps_rows_and_epoch_for_recovery() {
    let mut v = TrajectoryView::new("fixture-thread".into());
    v.apply_page(
        Direction::Latest,
        ThreadViewPage {
            epoch: 1,
            trajectory: rows(0, 10),
            ..Default::default()
        },
    );
    v.apply_page(
        Direction::Refresh,
        ThreadViewPage {
            epoch: -1,
            degraded: true,
            diagnostic: Some("fixture busy".into()),
            ..Default::default()
        },
    );
    assert_eq!(v.rows.len(), 10);
    assert_eq!(v.epoch, Some(1));
    assert_eq!(v.diagnostic.as_deref(), Some("fixture busy"));
    v.apply_page(
        Direction::Refresh,
        ThreadViewPage {
            epoch: 2,
            trajectory: rows(20, 30),
            ..Default::default()
        },
    );
    assert_eq!(v.rows.first().unwrap().created_seq, 20);
}
