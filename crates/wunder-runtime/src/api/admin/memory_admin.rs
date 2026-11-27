// 管理员诊断：进程 / cgroup 内存快照与常驻缓存的占用估算。
//
// 只读接口，不写入任何存储、不序列化会话事件；用于判断 800MB 是匿名堆、
// page cache、还是某类按会话/用户常驻的缓存。
use crate::api::admin::now_ts;
use crate::core::memory::cgroup_memory;
use crate::core::runtime_metrics;
use crate::state::AppState;
use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

pub(super) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/wunder/admin/memory", get(admin_memory))
        .route("/wunder/admin/memory/release", post(admin_release_memory))
}

async fn admin_memory(State(state): State<Arc<AppState>>) -> Json<Value> {
    runtime_metrics::record_loop_tick("api.admin.memory", "request");
    let snapshot = memory_snapshot(&state).await;
    let mut payload = snapshot;
    if let Value::Object(ref mut map) = payload {
        map.insert("generated_at".into(), json!(now_ts()));
        map.insert(
            "notes".into(),
            json!({
                "process": "VmRSS/VmHWM 来自 /proc/self/status；RssAnon/RssFile 拆出匿名堆与文件页。",
                "cgroup": "memory.current 是 cgroup 记账值，含 page cache，不等于进程 RSS。",
                "cache_bytes_estimate": "各项为字符串字节估算；monitor 事件用每事件上限×条数，是上界而非实测。",
            }),
        );
    }
    Json(payload)
}

async fn admin_release_memory(State(state): State<Arc<AppState>>) -> Json<Value> {
    runtime_metrics::record_loop_tick("api.admin.memory.release", "request");
    let before = memory_snapshot(&state).await;
    let workspace = state.workspace.release_memory_caches();
    let user_tools = state.user_tool_manager.release_memory_caches();
    let monitor = state.monitor.release_memory_caches();
    let thread_meta_entries = super::monitor_admin::clear_thread_meta_cache();
    let knowledge_bases = crate::services::knowledge::clear_cache().await;
    crate::core::runtime_tuning::release_free_memory();
    tracing::info!(
        knowledge_bases,
        thread_meta_entries,
        "admin memory release completed"
    );
    let after = memory_snapshot(&state).await;
    Json(json!({
        "before": before,
        "after": after,
        "cleared": {
            "workspace": workspace,
            "user_tools": user_tools,
            "monitor": monitor,
            "thread_meta_entries": thread_meta_entries,
            "knowledge_bases": knowledge_bases,
        },
        "actions": [
            "workspace_caches",
            "user_tool_caches",
            "monitor_terminal_events",
            "thread_meta_cache",
            "knowledge_cache",
            "malloc_trim",
        ],
        "notes": {
            "safe": "只清空可重建缓存并归还空闲堆页；会话记录、文件、配置和持久化数据都不删除。",
            "cost": "知识库和用户技能缓存会在下一次检索/打开时重新加载，首次请求可能变慢。",
            "cgroup": "malloc_trim 只影响匿名堆；cgroup 的 file/page cache 由内核按压力回收，不保证立即下降。",
        }
    }))
}

async fn memory_snapshot(state: &AppState) -> Value {
    let mut snapshot = json!({
        "process": process_memory(),
        "cgroup": serde_json::to_value(cgroup_memory()).unwrap_or(Value::Null),
        "monitor": state.monitor.memory_stats(),
        "workspace": state.workspace.memory_stats(),
        "knowledge": crate::services::knowledge::memory_stats().await,
        "user_tools": state.user_tool_manager.memory_stats(),
        "thread_meta_cache": super::monitor_admin::thread_meta_cache_stats(),
    });
    if let Value::Object(ref mut map) = snapshot {
        let estimate = cache_bytes(map);
        map.insert("cache_bytes_estimate".into(), json!(estimate));
    }
    snapshot
}

fn cache_bytes(map: &serde_json::Map<String, Value>) -> u64 {
    let monitor = map.get("monitor").cloned().unwrap_or(Value::Null);
    let workspace = map.get("workspace").cloned().unwrap_or(Value::Null);
    let knowledge = map.get("knowledge").cloned().unwrap_or(Value::Null);
    let user_tools = map.get("user_tools").cloned().unwrap_or(Value::Null);
    let thread_meta = map.get("thread_meta_cache").cloned().unwrap_or(Value::Null);
    json_value_bytes(&monitor, "event_bytes_upper_bound")
        + json_value_bytes(&monitor, "summary_cache_bytes")
        + json_nested_bytes(&workspace, "search_cache", "bytes_estimate")
        + json_nested_bytes(&workspace, "tree_cache", "bytes")
        + json_nested_bytes(&workspace, "context_text_cache", "bytes")
        + json_value_bytes(&knowledge, "content_bytes")
        + json_nested_bytes(&user_tools, "skill_cache", "spec_bytes_estimate")
        + json_value_bytes(&thread_meta, "bytes_estimate")
}

fn json_nested_bytes(root: &Value, first: &str, second: &str) -> u64 {
    root.get(first)
        .and_then(|value| value.get(second))
        .and_then(Value::as_u64)
        .unwrap_or(0)
}

fn json_value_bytes(root: &Value, key: &str) -> u64 {
    root.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn read_trim(path: &str) -> Option<String> {
    fs::read_to_string(Path::new(path))
        .ok()
        .map(|value| value.trim().to_string())
}

fn parse_kv(text: &str) -> BTreeMap<String, u64> {
    let mut output = BTreeMap::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        let number = value
            .strip_suffix("kB")
            .map(str::trim)
            .unwrap_or(value)
            .parse::<u64>()
            .ok();
        if let Some(number) = number {
            output.insert(key.trim().to_string(), number);
        }
    }
    output
}

fn process_memory() -> Value {
    let status = read_trim("/proc/self/status").unwrap_or_default();
    let fields = parse_kv(&status);
    let kb = |key: &str| fields.get(key).copied().unwrap_or(0).saturating_mul(1024);
    json!({
        "vm_rss_bytes": kb("VmRSS"),
        "vm_hwm_bytes": kb("VmHWM"),
        "vm_size_bytes": kb("VmSize"),
        "vm_data_bytes": kb("VmData"),
        "rss_anon_bytes": kb("RssAnon"),
        "rss_file_bytes": kb("RssFile"),
        "rss_shmem_bytes": kb("RssShmem"),
        "threads": fields.get("Threads").copied().unwrap_or(0),
        "source": "/proc/self/status",
    })
}
