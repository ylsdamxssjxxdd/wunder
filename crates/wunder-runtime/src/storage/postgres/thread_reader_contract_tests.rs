use super::*;
use crate::storage::canonical_thread_log::wait;
use crate::storage::{postgres::PostgresStorage, ThreadLogStore};
use std::collections::HashSet;
use wunder_core::storage_backend::LogStatsStore;

#[test]
#[ignore = "requires an isolated WUNDER_TEST_POSTGRES_DSN"]
fn postgres_reader_item_pages_bound_bodies_without_skips_or_duplicates() {
    use crate::storage::rollout::ROLLOUT_MAX_RECORD_BYTES;

    const MIB: usize = 1024 * 1024;
    let storage = std::sync::Arc::new(
        PostgresStorage::new(std::env::var("WUNDER_TEST_POSTGRES_DSN").unwrap(), 5, 4).unwrap(),
    );
    let owner = format!("sample-reader-{}", uuid::Uuid::new_v4().simple());
    let spec = StressThreadSpec {
        session_id: format!("sample-thread-{}", uuid::Uuid::new_v4().simple()),
        title: "Sample bounded reader history".into(),
        workspace_id: None,
        user_rounds: 1,
        model_rounds_per_turn: 2,
        seed: Some(11),
        scenario: StressThreadScenario::Complex,
    };
    storage
        .generate_stress_thread(&owner, &spec, |_, _| {})
        .unwrap();
    let store = storage.service_threads().unwrap();
    let thread = ThreadId::new(spec.session_id).unwrap();
    let turn = TurnId::new(format!("sample-page-{}", uuid::Uuid::new_v4().simple())).unwrap();
    let other_turn =
        TurnId::new(format!("sample-other-{}", uuid::Uuid::new_v4().simple())).unwrap();
    let item = |id: &str, turn: &TurnId, size: usize| {
        CanonicalRecord::Event(CanonicalEvent::ItemCompleted(CanonicalItem {
            item_id: ItemId::new(id).unwrap(),
            turn_id: turn.clone(),
            root_turn_id: None,
            created_at_ms: 20,
            kind: CanonicalItemKind::AssistantMessage,
            // Large trace metadata exercises actual canonical reader bodies;
            // the displayed final reply remains small.
            payload: json!({"content":"sample answer", "reader_fixture":"x".repeat(size)}),
            display_output: None,
            display_output_truncated: false,
        }))
    };
    let mut expected = Vec::new();
    // Reverse page order: two 1.5 MiB records, one 5 MiB record alone,
    // then two more 1.5 MiB records. Interleave a different turn to ensure
    // both metadata and body reads retain the turn filter.
    for (index, size) in [
        MIB + MIB / 2,
        MIB + MIB / 2,
        5 * MIB,
        MIB + MIB / 2,
        MIB + MIB / 2,
    ]
    .into_iter()
    .enumerate()
    {
        let id = format!("{turn}:item-{index}");
        wait(store.append_canonical(&thread, vec![item(&id, &turn, size)]))
            .unwrap()
            .unwrap();
        expected.push(id);
        wait(store.append_canonical(
            &thread,
            vec![item(&format!("{other_turn}:item-{index}"), &other_turn, 16)],
        ))
        .unwrap()
        .unwrap();
    }
    expected.reverse();
    let mut cursor = None;
    let mut first_cursor = None;
    let mut revision = None;
    let mut found = Vec::new();
    let mut counts = Vec::new();
    loop {
        let page = wait(store.list_items(ItemListQuery {
            thread_id: thread.clone(),
            turn_id: Some(turn.clone()),
            cursor: cursor.clone(),
            limit: 500,
        }))
        .unwrap()
        .unwrap();
        assert!(!page.entries.is_empty());
        assert_eq!(
            revision.get_or_insert_with(|| page.history_revision.clone()),
            &page.history_revision
        );
        counts.push(page.entries.len());
        let mut body_bytes = 0;
        for entry in &page.entries {
            assert_eq!(entry.item.turn_id, turn);
            assert_eq!(entry.item.payload["content"], "sample answer");
            body_bytes += entry.item.payload["reader_fixture"].as_str().unwrap().len();
            found.push(entry.item.item_id.to_string());
        }
        if body_bytes > 4 * MIB {
            assert_eq!(page.entries.len(), 1);
        }
        match page.next_cursor {
            Some(next) => {
                assert_ne!(cursor.as_ref(), Some(&next));
                first_cursor.get_or_insert_with(|| next.clone());
                cursor = Some(next);
            }
            None => break,
        }
    }
    assert_eq!(counts, [2, 1, 2]);
    assert_eq!(found, expected);
    assert_eq!(found.iter().collect::<HashSet<_>>().len(), found.len());

    wait(store.append_canonical(&thread, vec![item(&format!("{turn}:late"), &turn, 16)]))
        .unwrap()
        .unwrap();
    assert!(matches!(
        wait(store.list_items(ItemListQuery {
            thread_id: thread.clone(),
            turn_id: Some(turn.clone()),
            cursor: first_cursor,
            limit: 500,
        }))
        .unwrap(),
        Err(ThreadStoreError::HistoryChanged { .. })
    ));

    // Simulate corrupt out-of-contract storage without allowing the reader to
    // transfer or decode a record larger than the canonical write ceiling.
    let latest_id = format!("{turn}:late");
    storage.conn().unwrap().execute(
        "UPDATE service_thread_records SET record_json=repeat('x',$3) WHERE thread_id=$1 AND item_id=$2 AND record_type='item_completed'",
        &[&thread.as_str(), &latest_id, &((ROLLOUT_MAX_RECORD_BYTES + 1) as i32)],
    ).unwrap();
    assert!(matches!(
        wait(store.list_items(ItemListQuery {
            thread_id: thread.clone(),
            turn_id: Some(turn),
            cursor: None,
            limit: 500,
        }))
        .unwrap(),
        Err(ThreadStoreError::CorruptHistory { .. })
    ));
    wait(store.delete_thread(&thread)).unwrap().unwrap();
}

#[test]
#[ignore = "requires an isolated WUNDER_TEST_POSTGRES_DSN"]
fn postgres_reader_pages_and_checkpoint_context_preserve_budgets_and_utf8() {
    let storage = std::sync::Arc::new(
        PostgresStorage::new(std::env::var("WUNDER_TEST_POSTGRES_DSN").unwrap(), 5, 4).unwrap(),
    );
    let owner = format!("sample-reader-{}", uuid::Uuid::new_v4().simple());
    let spec = StressThreadSpec {
        session_id: format!("sample-thread-{}", uuid::Uuid::new_v4().simple()),
        title: "Sample reader history".into(),
        workspace_id: None,
        user_rounds: 3,
        model_rounds_per_turn: 4,
        seed: Some(7),
        scenario: StressThreadScenario::Complex,
    };
    let stats = storage
        .generate_stress_thread(&owner, &spec, |_, _| {})
        .unwrap();
    let store = storage.service_threads().unwrap();
    let thread = ThreadId::new(spec.session_id.clone()).unwrap();
    let mut cursor = None;
    let mut turn_ids = Vec::new();
    loop {
        let page = wait(store.list_turns(TurnListQuery {
            thread_id: thread.clone(),
            cursor: cursor.clone(),
            limit: 1,
        }))
        .unwrap()
        .unwrap();
        assert!(page.entries.len() <= 1);
        for turn in page.entries {
            assert_eq!(turn.status, TurnCompletionStatus::Completed);
            assert!(turn.item_count >= 5);
            assert!(!turn_ids.contains(&turn.turn_id));
            turn_ids.push(turn.turn_id);
        }
        match page.next_cursor {
            Some(next) => {
                assert_ne!(cursor.as_ref(), Some(&next));
                cursor = Some(next);
            }
            None => break,
        }
    }
    assert_eq!(turn_ids.len(), 3);
    let mut cursor = None;
    let mut item_ids = HashSet::new();
    let mut ordinals = Vec::new();
    let mut first_cursor = None;
    loop {
        let page = wait(store.list_items(ItemListQuery {
            thread_id: thread.clone(),
            turn_id: None,
            cursor: cursor.clone(),
            limit: 3,
        }))
        .unwrap()
        .unwrap();
        assert!(page.entries.len() <= 3);
        if first_cursor.is_none() {
            first_cursor = page.next_cursor.clone();
        }
        for item in page.entries {
            assert!(item_ids.insert(item.item.item_id));
            ordinals.push(item.rollout_ordinal);
        }
        match page.next_cursor {
            Some(next) => {
                assert_ne!(cursor.as_ref(), Some(&next));
                cursor = Some(next);
            }
            None => break,
        }
    }
    assert_eq!(item_ids.len() as i64, stats.items);
    assert!(ordinals.windows(2).all(|pair| pair[0] > pair[1]));
    let filtered = wait(store.list_items(ItemListQuery {
        thread_id: thread.clone(),
        turn_id: Some(turn_ids[0].clone()),
        cursor: None,
        limit: 500,
    }))
    .unwrap()
    .unwrap();
    assert!(filtered
        .entries
        .iter()
        .all(|item| item.item.turn_id == turn_ids[0]));
    let bounded = wait(store.load_model_context(ModelContextRequest {
        thread_id: thread.clone(),
        position: None,
        max_records: 2,
        max_bytes: 128 * 1024,
    }))
    .unwrap()
    .unwrap();
    assert!(bounded.truncated);
    assert_eq!(bounded.records.len(), 2);
    assert!(bounded.records.iter().all(|record| matches!(
        record.record,
        CanonicalRecord::ResponseItem(_) | CanonicalRecord::Compacted(_)
    )));
    let byte_bounded = wait(store.load_model_context(ModelContextRequest {
        thread_id: thread.clone(),
        position: None,
        max_records: 100,
        max_bytes: 1,
    }))
    .unwrap()
    .unwrap();
    assert!(byte_bounded.truncated && byte_bounded.records.is_empty());
    let boundary = wait(store.flush(&thread))
        .unwrap()
        .unwrap()
        .persisted_through;
    let turn = &turn_ids[0];
    let text = "sample 中文🙂 tail";
    let item_id = ItemId::new(format!("{turn}:unicode")).unwrap();
    wait(store.append_canonical(
        &thread,
        vec![
            CanonicalRecord::Compacted(CompactionCheckpoint {
                turn_id: turn.clone(),
                summary: json!({"content":"sample checkpoint"}),
                retained_context: json!([{"role":"user","content":"sample retained context"}]),
            }),
            CanonicalRecord::ResponseItem(ResponseItem {
                item_id: ItemId::new(format!("{turn}:unicode-model")).unwrap(),
                turn_id: turn.clone(),
                root_turn_id: Some(turn.clone()),
                created_at_ms: 10,
                kind: CanonicalItemKind::AssistantMessage,
                payload: json!({"role":"assistant","content":text}),
            }),
            CanonicalRecord::Event(CanonicalEvent::ItemCompleted(CanonicalItem {
                item_id: item_id.clone(),
                turn_id: turn.clone(),
                root_turn_id: Some(turn.clone()),
                created_at_ms: 10,
                kind: CanonicalItemKind::AssistantMessage,
                payload: json!({"content":text}),
                display_output: None,
                display_output_truncated: false,
            })),
        ],
    ))
    .unwrap()
    .unwrap();
    assert!(matches!(
        wait(store.list_items(ItemListQuery {
            thread_id: thread.clone(),
            turn_id: None,
            cursor: first_cursor,
            limit: 3
        }))
        .unwrap(),
        Err(ThreadStoreError::HistoryChanged { .. })
    ));
    let context = wait(store.load_model_context(ModelContextRequest {
        thread_id: thread.clone(),
        position: None,
        max_records: 2,
        max_bytes: 128 * 1024,
    }))
    .unwrap()
    .unwrap();
    assert!(!context.truncated);
    assert_eq!(context.records.len(), 2);
    assert!(matches!(
        context.records[0].record,
        CanonicalRecord::Compacted(_)
    ));
    let old = wait(store.load_model_context(ModelContextRequest {
        thread_id: thread.clone(),
        position: Some(boundary),
        max_records: 2,
        max_bytes: 128 * 1024,
    }))
    .unwrap()
    .unwrap();
    assert!(old.truncated);
    assert!(old
        .records
        .iter()
        .all(|record| matches!(record.record, CanonicalRecord::ResponseItem(_))));
    let messages = wait(store.model_messages(&thread, None)).unwrap().unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["content"], "sample retained context");
    assert_eq!(messages[1]["content"], text);
    let mut offset = 0;
    let mut reconstructed = String::new();
    loop {
        let page = wait(store.read_item_content(ItemContentRequest {
            thread_id: thread.clone(),
            item_id: item_id.clone(),
            offset_bytes: offset,
            max_bytes: 4,
        }))
        .unwrap()
        .unwrap();
        assert_eq!(page.offset_bytes, offset);
        reconstructed.push_str(&page.content);
        match page.next_offset_bytes {
            Some(next) => {
                assert!(next > offset);
                offset = next;
            }
            None => break,
        }
    }
    assert_eq!(reconstructed, text);
    assert!(matches!(
        wait(store.read_item_content(ItemContentRequest {
            thread_id: thread.clone(),
            item_id: item_id.clone(),
            offset_bytes: 8,
            max_bytes: 4,
        }))
        .unwrap(),
        Err(ThreadStoreError::Conflict { .. })
    ));
    assert!(matches!(
        wait(store.read_item_content(ItemContentRequest {
            thread_id: thread.clone(),
            item_id,
            offset_bytes: 7,
            max_bytes: 1,
        }))
        .unwrap(),
        Err(ThreadStoreError::Conflict { .. })
    ));
    // Search must find decoded text beyond preview boundaries without loading
    // whole records, and exclude internal fields even when they also match.
    let needle = "sample 搜索🙂 \"quoted\" \\escaped\nend";
    let prefix = "sample plain prefix ".repeat(4096);
    let first_part = format!("{prefix}{needle}");
    let full_text = format!("{first_part}\nsample final text");
    let direct_id = format!("{turn}:search-direct");
    let wrapped_id = format!("{turn}:search-wrapped");
    let compaction_id = format!("{turn}:search-compaction");
    let search_item = |id: &str, payload: Value| {
        CanonicalRecord::Event(CanonicalEvent::ItemCompleted(CanonicalItem {
            item_id: ItemId::new(id).unwrap(),
            turn_id: turn.clone(),
            root_turn_id: Some(turn.clone()),
            created_at_ms: 20,
            kind: CanonicalItemKind::AssistantMessage,
            payload,
            display_output: None,
            display_output_truncated: false,
        }))
    };
    let mut search_records = vec![
        search_item(&direct_id, json!({"content":full_text})),
        search_item(
            &wrapped_id,
            json!({"item_id":wrapped_id,"kind":"assistant_message","payload":{
                "content":[{"type":"text","text":first_part},{"type":"image","image_url":"sample-unused"},{"type":"text","text":"sample final text"}]
            }}),
        ),
        search_item(
            &compaction_id,
            json!({"kind":"system_message","content":needle,"model_content":"sample private context","meta":{"type":"compaction_summary","replacement_history":[{"content":"sample private context"}]}}),
        ),
        search_item(
            &format!("{turn}:search-hidden"),
            json!({"content":needle,"meta":{"hidden":true}}),
        ),
        search_item(
            &format!("{turn}:search-internal"),
            json!({"content":needle,"visibility":"internal"}),
        ),
        search_item(
            &format!("{turn}:search-system"),
            json!({"kind":"system_message","content":needle}),
        ),
        search_item(
            &format!("{turn}:search-model"),
            json!({"content":"sample public summary","model_content":needle,"reasoning":needle,"meta":{"replacement_history":[{"content":needle}]}}),
        ),
        search_item(
            &format!("{turn}:search-bad-parts"),
            json!({"content":[{"type":"text","text":needle},"type"]}),
        ),
    ];
    search_records.extend((0..21).map(|index| {
        search_item(
            &format!("{turn}:search-hit-{index}"),
            json!({"content":format!("sample paging match {index}")}),
        )
    }));
    wait(store.append_canonical(&thread, search_records))
        .unwrap()
        .unwrap();
    let found = storage
        .thread_view_search(&owner, thread.as_str(), needle)
        .unwrap();
    assert!(!found.has_more);
    assert_eq!(found.hits.len(), 3);
    assert_eq!(
        found
            .hits
            .iter()
            .map(|hit| hit.item_id.clone())
            .collect::<HashSet<_>>(),
        HashSet::from([direct_id.clone(), wrapped_id.clone(), compaction_id.clone()])
    );
    for hit in &found.hits {
        assert!(hit.snippet.starts_with(needle));
        assert!(hit.snippet.len() <= 256);
        assert_eq!(
            hit.body.bytes,
            if hit.item_id == compaction_id {
                needle.len()
            } else {
                full_text.len()
            }
        );
    }
    assert!(serde_json::to_vec(&found).unwrap().len() < 8192);
    assert!(storage
        .thread_view_search(&owner, thread.as_str(), "sample private context")
        .unwrap()
        .hits
        .is_empty());
    assert!(
        storage
            .thread_view_search(&owner, thread.as_str(), "sample public summary")
            .unwrap()
            .hits
            .len()
            == 1
    );
    let paged = storage
        .thread_view_search(&owner, thread.as_str(), "sample paging match")
        .unwrap();
    assert!(paged.has_more);
    assert_eq!(paged.hits.len(), 20);
    assert!(paged
        .hits
        .windows(2)
        .all(|hits| hits[0].cursor > hits[1].cursor));
    assert!(storage
        .thread_view_search("sample-other", thread.as_str(), needle)
        .is_err());
    assert!(storage
        .thread_view_search(&owner, thread.as_str(), &"s".repeat(1025))
        .is_err());
    wait(store.delete_thread(&thread)).unwrap().unwrap();
}

#[test]
#[ignore = "requires an isolated WUNDER_TEST_POSTGRES_DSN"]
fn postgres_pending_chat_previews_and_batch_tails_preserve_body_and_active_head() {
    let storage = std::sync::Arc::new(
        PostgresStorage::new(std::env::var("WUNDER_TEST_POSTGRES_DSN").unwrap(), 5, 4).unwrap(),
    );
    let owner = format!("sample-preview-{}", uuid::Uuid::new_v4().simple());
    let session = format!("sample-pending-{}", uuid::Uuid::new_v4().simple());
    let other_owner = format!("sample-preview-{}", uuid::Uuid::new_v4().simple());
    let other_session = format!("sample-pending-{}", uuid::Uuid::new_v4().simple());
    let admissions = storage.service_threads().unwrap().admission_store();
    let content = "sample 中文🙂 ".repeat(4096);
    let full_text = format!("{content}\nsample final part");
    // Each projected field keeps 128 UTF-8 bytes. Escaped characters retain
    // that budget while exercising the larger serialized response boundary.
    let card_text = "\"\\".repeat(64);
    let input = json!({
        "content":[{"type":"text","text":content},{"type":"text","text":"sample final part"}],
        "attachments":(0..8).map(|_|json!({"name":card_text,"content_type":card_text,"public_path":card_text})).collect::<Vec<_>>(),
    });
    let first = admissions.accept(&owner, &session, &input).unwrap();
    let active_turn = first["turn_id"].as_str().unwrap();
    admissions
        .update_with_control(&session, active_turn, "running", false, &Value::Null)
        .unwrap();
    for index in 0..22 {
        let accepted = admissions.accept(&owner, &session, &input).unwrap();
        admissions.update_with_control(&session, accepted["turn_id"].as_str().unwrap(), "queued", false,
            &json!({"queued_input":{"queue_id":format!("sample-queue-{index}"),"reason":"sample queued input","priority":0}})).unwrap();
    }
    admissions
        .accept(
            &other_owner,
            &other_session,
            &json!({"content":"sample other input"}),
        )
        .unwrap();
    let page = storage
        .thread_view_page(&owner, &session, "chat", None, 20)
        .unwrap();
    assert!(
        !page.chat.is_empty() && page.chat.len() < 20,
        "response budget fixture: rows={}, bytes={}, attachment_cards={}",
        page.chat.len(),
        serde_json::to_vec(&page).unwrap().len(),
        page.chat
            .first()
            .map_or(0, |row| row.user.attachments.len())
    );
    assert!(page.has_more);
    assert_eq!(page.active_turn_id, active_turn);
    assert!(
        serde_json::to_vec(&page).unwrap().len() < wunder_core::thread_view::CHAT_RESPONSE_BYTES
    );
    for turn in &page.chat {
        assert!(turn.user.content.len() <= 2048);
        assert_eq!(turn.user.content_offset, full_text.encode_utf16().count());
        assert_eq!(turn.user.body.as_ref().unwrap().bytes, full_text.len());
    }
    let mut rounds = page
        .chat
        .iter()
        .map(|turn| turn.user_round)
        .collect::<HashSet<_>>();
    let mut before = page.next_before;
    loop {
        let older = storage
            .thread_view_page(&owner, &session, "chat", before, 20)
            .unwrap();
        assert!(
            serde_json::to_vec(&older).unwrap().len()
                < wunder_core::thread_view::CHAT_RESPONSE_BYTES
        );
        for turn in &older.chat {
            assert!(rounds.insert(turn.user_round));
        }
        if !older.has_more {
            break;
        }
        assert_ne!(older.next_before, before);
        before = older.next_before;
    }
    assert_eq!(rounds.len(), 23);
    let body_id = &page
        .chat
        .last()
        .unwrap()
        .user
        .body
        .as_ref()
        .unwrap()
        .body_id;
    let mut reconstructed = String::new();
    let mut cursor = 0;
    loop {
        let body = storage
            .thread_body_page(&owner, &session, body_id, 1, cursor)
            .unwrap();
        assert!(
            body.chunks.iter().map(String::len).sum::<usize>()
                <= 8 * wunder_core::thread_view::BODY_CHUNK_BYTES
        );
        reconstructed.push_str(&body.chunks.concat());
        match body.next_cursor {
            Some(next) => {
                assert!(next > cursor);
                cursor = next;
            }
            None => break,
        }
    }
    assert_eq!(reconstructed, full_text);
    assert!(storage
        .thread_body_page(&other_owner, &session, body_id, 1, 0)
        .is_err());
    let tails = storage
        .thread_turn_tails(
            &owner,
            &[
                session.clone(),
                other_session.clone(),
                session.clone(),
                String::new(),
            ],
        )
        .unwrap();
    assert_eq!(tails.len(), 1);
    let tail = &tails[&session];
    assert_eq!(tail.user_round, 23);
    assert_eq!(tail.turn_status, "queued");
    assert_eq!(
        tail.runtime_head.as_ref().unwrap().active_turn_id,
        active_turn
    );
    assert_eq!(
        tail.runtime_head.as_ref().unwrap().runtime_status,
        "running"
    );
    assert!(storage.thread_turn_tails(&owner, &[]).unwrap().is_empty());
    assert_eq!(storage.delete_thread_logs_by_user(&owner).unwrap(), 1);
    assert_eq!(storage.delete_thread_logs_by_user(&other_owner).unwrap(), 1);
}
