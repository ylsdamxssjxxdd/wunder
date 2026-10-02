use super::tests::SubagentTestHarness;
use super::*;

#[tokio::test]
async fn every_selector_checks_tree_before_read_or_mutation() {
    let harness = SubagentTestHarness::new();
    harness.upsert_session("root-a", None);
    harness.upsert_session("root-b", None);
    harness.upsert_session("worker-a", Some("root-a"));
    harness.upsert_session("worker-b", Some("root-b"));
    harness.upsert_run("run-b", "worker-b", Some("root-b"));
    let context = harness.context("root-a");
    for action in [
        "status",
        "wait",
        "history",
        "send",
        "interrupt",
        "close",
        "resume",
    ] {
        let result=execute(&context,&json!({"action":action,"session_id":"worker-b","message":"Fixture task","wait_seconds":0})).await;
        assert!(
            result.is_err(),
            "action {action} must reject an unrelated worker"
        );
    }
    for selector in [json!({"run_ids":["run-b"]}), json!({"parent_id":"root-b"})] {
        let mut args = selector;
        args["action"] = json!("status");
        assert!(execute(&context, &args).await.is_err());
    }
    assert!(
        execute(&context, &json!({"action":"list","parent_id":"root-b"}))
            .await
            .is_err()
    );
    // Validation covers the entire batch before any worker is mutated.
    assert!(execute(
        &context,
        &json!({"action":"close","session_ids":["worker-a","worker-b"]})
    )
    .await
    .is_err());
    assert_eq!(
        harness
            .storage
            .get_chat_session("fixture-owner", "worker-a")
            .unwrap()
            .unwrap()
            .status,
        "active"
    );
    let sibling = harness.context("worker-a");
    harness.upsert_session("worker-c", Some("root-a"));
    let result = execute(&sibling, &json!({"action":"list","parent_id":"/root"}))
        .await
        .unwrap();
    assert!(result["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["task_path"] == "/root/worker-c"));
    assert!(execute(
        &sibling,
        &json!({"action":"status","session_id":"/root/worker-c"})
    )
    .await
    .is_ok());
}
