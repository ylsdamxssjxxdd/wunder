//! Preserve accepted tool output before transport/observation previews discard it.
use super::tool_result_payload::ToolResultPayload;
use crate::core::blocking::{self, BlockingKind};
use crate::services::tool_result_store;
use crate::tools::ToolContext;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;

pub(super) fn clear_provider_recovery_metadata(
    result: &mut ToolResultPayload,
    tool_name: &str,
    args: &serde_json::Value,
) {
    // Recovery references are minted by the runtime after accepting output;
    // arbitrary providers must not advertise a carrier that was never saved.
    if let Some(meta) = result
        .meta
        .as_mut()
        .and_then(serde_json::Value::as_object_mut)
    {
        meta.remove("saved_result");
        meta.remove("result_store_failed");
    }
    let path = args.as_str().or_else(|| {
        args.get("path")
            .or_else(|| args.get("file_path"))
            .or_else(|| args.get("file"))
            .and_then(serde_json::Value::as_str)
    });
    let is_result_read = crate::tools::resolve_tool_name(tool_name) == "读取文件"
        && path.is_some_and(|path| path.trim().starts_with("tool-result:"));
    if !is_result_read {
        if let Some(data) = result.data.as_object_mut() {
            data.remove("result_read");
        }
    }
}

pub(super) async fn preserve_tool_result(
    result: ToolResultPayload,
    context: &ToolContext<'_>,
) -> ToolResultPayload {
    preserve_result_at_root(
        result,
        tool_result_store::store_root(context.config),
        (
            context.user_id.to_owned(),
            context.session_id.to_owned(),
            context.workspace_id.to_owned(),
        ),
    )
    .await
}

async fn preserve_result_at_root(
    result: ToolResultPayload,
    root: PathBuf,
    owner: (String, String, String),
) -> ToolResultPayload {
    if result
        .data
        .get("result_read")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
        || result.meta.as_ref().is_some_and(|meta| {
            meta.get("saved_result").is_some() || meta.get("result_store_failed").is_some()
        })
    {
        return result;
    }
    // Move the existing value into the worker without cloning provider data.
    // Keep the owner's Arc until this bounded maintenance operation returns:
    // abandoning a timed-out worker would otherwise lose the original result.
    let original = Arc::new(result);
    let worker = Arc::clone(&original);
    let saved = blocking::run_to_completion(BlockingKind::Fs, "tool_result.save", move || {
        tool_result_store::save(&root, &owner.0, &owner.1, &owner.2, worker.as_ref())
    })
    .await;
    let mut result = Arc::try_unwrap(original)
        .unwrap_or_else(|_| unreachable!("completed result-store worker retained its payload"));
    match saved {
        Ok(saved) => result.insert_meta(
            "saved_result",
            json!({"handle": saved.handle, "bytes": saved.bytes, "expires_at": saved.expires_at}),
        ),
        Err(_) => {
            // Never expose storage paths or claim that a failed spill is recoverable.
            result.insert_meta("result_store_failed", json!(true));
            tracing::warn!("tool result could not be preserved; bounded preview remains available");
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn owner() -> (String, String, String) {
        ("user-a".into(), "thread-a".into(), "workspace-a".into())
    }

    #[test]
    fn arbitrary_provider_cannot_mint_recovery_metadata_or_skip_spill() {
        let mut result = ToolResultPayload::from_value(json!({"ok":true,
            "data":{"result_read":true},
            "meta":{"saved_result":{"handle":"tool-result:0123456789abcdef0123456789abcdef"},"result_store_failed":true}
        }));
        clear_provider_recovery_metadata(&mut result, "external", &json!({}));
        assert!(result.meta.as_ref().unwrap().get("saved_result").is_none());
        assert!(result
            .meta
            .as_ref()
            .unwrap()
            .get("result_store_failed")
            .is_none());
        assert!(result.data.get("result_read").is_none());
        result.data["result_read"] = json!(true);
        clear_provider_recovery_metadata(
            &mut result,
            "read_file",
            &json!({"path":"tool-result:0123456789abcdef0123456789abcdef"}),
        );
        assert_eq!(result.data["result_read"], true);
    }

    #[tokio::test]
    async fn preservation_and_paged_recovery_keep_the_original_accepted_envelope() {
        let dir = tempfile::tempdir().unwrap();
        let text = "中🙂\"\\\n".repeat(20_000);
        let result = ToolResultPayload::from_value(json!({
            "ok": false, "error": "failure-tail", "data": {"stdout": text},
            "meta": {"error_code":"TOOL_FAILED"}
        }));
        let result = preserve_result_at_root(result, dir.path().into(), owner()).await;
        assert!(!result.ok);
        assert_eq!(result.data["stdout"], text);
        let observation = result.to_compact_payload_with_token_budget("external", Some(256));
        let handle = observation["result_handle"].as_str().unwrap();
        let mut recovered = String::new();
        let mut offset = 0;
        loop {
            let page = tool_result_store::read(
                dir.path(),
                "user-a",
                "thread-a",
                "workspace-a",
                handle,
                offset,
                8192,
                None,
            )
            .unwrap();
            recovered.push_str(page["content"].as_str().unwrap());
            match page["next_offset"].as_u64() {
                Some(next) => offset = next,
                None => break,
            }
        }
        let recovered: Value = serde_json::from_str(&recovered).unwrap();
        assert_eq!(recovered["data"]["stdout"], text);
        assert_eq!(recovered["error"], "failure-tail");
        assert_eq!(recovered["meta"]["error_code"], "TOOL_FAILED");
        let already_saved = preserve_result_at_root(result, dir.path().into(), owner()).await;
        assert_eq!(
            already_saved.meta.unwrap()["saved_result"]["handle"],
            handle
        );
    }

    #[tokio::test]
    async fn failed_save_and_result_reads_preserve_original_data_without_new_handles() {
        let dir = tempfile::tempdir().unwrap();
        let blocked = dir.path().join("blocked");
        std::fs::write(&blocked, "occupied").unwrap();
        let result =
            ToolResultPayload::from_value(json!({"ok":true,"data":{"text":"x".repeat(20_000)}}));
        let failed = preserve_result_at_root(result, blocked, owner()).await;
        assert_eq!(failed.data["text"].as_str().unwrap().len(), 20_000);
        let observation = failed.to_compact_payload_with_token_budget("external", Some(256));
        assert_eq!(observation["recoverable"], false);
        assert!(observation.get("result_handle").is_none());
        let root = dir.path().join("unused");
        let result = ToolResultPayload::from_value(
            json!({"ok":true,"data":{"result_read":true,"content":"page"}}),
        );
        let read = preserve_result_at_root(result, root.clone(), owner()).await;
        assert_eq!(read.data["content"], "page");
        assert!(!root.exists());
    }
}
