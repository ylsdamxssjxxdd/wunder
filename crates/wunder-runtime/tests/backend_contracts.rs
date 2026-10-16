use axum::{
    body::{to_bytes, Body},
    http::{Method, Request, StatusCode},
    Router,
};
use serde_json::Value;
use std::collections::HashSet;
use std::sync::Arc;
use tempfile::TempDir;
use tower::ServiceExt;
use wunder_server::{
    auth::{is_admin_path, is_leader_path},
    build_desktop_router, build_router,
    config::Config,
    config_store::ConfigStore,
    state::{AppState, AppStateInitOptions},
    tools::{builtin_tool_specs, resolve_tool_name},
};

struct ContractContext {
    server_app: Router,
    desktop_app: Router,
    state: Arc<AppState>,
    _temp_dir: TempDir,
}

async fn build_contract_context() -> ContractContext {
    let temp_dir = tempfile::tempdir().expect("create temp dir");
    let mut config = Config::default();
    config.storage.backend = "sqlite".to_string();
    config.storage.db_path = temp_dir
        .path()
        .join("backend-contracts.db")
        .to_string_lossy()
        .to_string();
    config.workspace.root = temp_dir
        .path()
        .join("workspaces")
        .to_string_lossy()
        .to_string();
    config.server.max_active_sessions = 3;

    let config_store = ConfigStore::new(temp_dir.path().join("wunder.yaml"));
    let config_for_store = config.clone();
    config_store
        .update(|current| *current = config_for_store.clone())
        .await
        .expect("write test config");

    let state = Arc::new(
        AppState::new_with_options(config_store, config, AppStateInitOptions::cli_default())
            .expect("create app state"),
    );

    ContractContext {
        server_app: build_router(state.clone()),
        desktop_app: build_desktop_router(state.clone()),
        state,
        _temp_dir: temp_dir,
    }
}

async fn request_json(app: &Router, method: Method, path: &str, body: Option<Value>) -> StatusCode {
    let body = body
        .map(|value| Body::from(value.to_string()))
        .unwrap_or_else(Body::empty);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("content-type", "application/json")
                .body(body)
                .expect("build request"),
        )
        .await
        .expect("send request");
    let status = response.status();
    let _ = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("drain response");
    status
}

#[test]
fn admin_path_classifier_keeps_public_and_admin_surfaces_separate() {
    let public_paths = [
        "/",
        "/wunder/auth/login",
        "/wunder/chat/sessions",
        "/wunder/workspace/content",
        "/wunder/user_tools/catalog",
        "/wunder/user/agent",
        "/wunder/external/workflows",
    ];
    for path in public_paths {
        assert!(
            !is_admin_path(path),
            "{path} should remain public/user-scoped"
        );
    }

    let admin_paths = [
        "/wunder",
        "/wunder/admin/server",
        "/wunder/admin/performance/sample",
        "/wunder/admin/user_accounts",
        "/wunder/admin/gateway/status",
    ];
    for path in admin_paths {
        assert!(is_admin_path(path), "{path} should require admin guard");
    }
}

#[test]
fn leader_path_classifier_is_limited_to_org_and_user_account_admin_routes() {
    assert!(is_leader_path("/wunder/admin/org_units"));
    assert!(is_leader_path("/wunder/admin/org_units/root"));
    assert!(is_leader_path("/wunder/admin/user_accounts"));
    assert!(is_leader_path("/wunder/admin/user_accounts/user-id"));
    assert!(!is_leader_path("/wunder/admin/performance/sample"));
    assert!(!is_leader_path("/wunder/admin/gateway/status"));
    assert!(!is_leader_path("/wunder/admin/users"));
}

#[tokio::test]
async fn server_router_exposes_admin_surface_but_desktop_router_does_not() {
    let context = build_contract_context().await;

    let server_status = request_json(
        &context.server_app,
        Method::GET,
        "/wunder/admin/server",
        None,
    )
    .await;
    let desktop_status = request_json(
        &context.desktop_app,
        Method::GET,
        "/wunder/admin/server",
        None,
    )
    .await;
    let desktop_user_status =
        request_json(&context.desktop_app, Method::GET, "/wunder/auth/me", None).await;

    assert_eq!(server_status, StatusCode::OK);
    assert_eq!(desktop_status, StatusCode::NOT_FOUND);
    assert_eq!(desktop_user_status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn performance_sample_rejects_invalid_concurrency_before_running_work() {
    let context = build_contract_context().await;

    let zero_status = request_json(
        &context.server_app,
        Method::POST,
        "/wunder/admin/performance/sample",
        Some(serde_json::json!({ "concurrency": 0 })),
    )
    .await;
    let over_limit_status = request_json(
        &context.server_app,
        Method::POST,
        "/wunder/admin/performance/sample",
        Some(serde_json::json!({ "concurrency": 4 })),
    )
    .await;

    assert_eq!(zero_status, StatusCode::BAD_REQUEST);
    assert_eq!(over_limit_status, StatusCode::BAD_REQUEST);
}

#[test]
fn builtin_tool_catalog_keeps_core_file_tools_available_and_unique() {
    let specs = builtin_tool_specs();
    let mut names = HashSet::new();
    for spec in &specs {
        let name = spec.name.trim();
        assert!(!name.is_empty(), "builtin tool name must not be empty");
        assert!(
            names.insert(name.to_string()),
            "builtin tool name must be unique: {name}"
        );
        assert!(
            !spec.description.trim().is_empty(),
            "builtin tool description must not be empty: {name}"
        );
        assert!(
            spec.input_schema.is_object(),
            "builtin tool input schema must be an object: {name}"
        );
    }

    for alias in [
        "list_files",
        "write_file",
        "read_file",
        "search_content",
        "apply_patch",
        "execute_command",
    ] {
        let canonical = resolve_tool_name(alias);
        assert!(
            names.contains(&canonical),
            "{alias} should resolve to builtin tool {canonical}"
        );
    }
}

async fn request_tools_catalog(app: &Router) -> Value {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/wunder/tools?user_id=preset_template")
                .body(Body::empty())
                .expect("build tools request"),
        )
        .await
        .expect("request tools catalog");
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .expect("read tools catalog");
    serde_json::from_slice(&body).expect("decode tools catalog")
}

fn assert_builtin_catalog_names(catalog: &Value, expected: &HashSet<String>) {
    let names = catalog["builtin_tools"]
        .as_array()
        .expect("builtin tools")
        .iter()
        .map(|tool| tool["name"].as_str().expect("tool name").to_string())
        .collect::<Vec<_>>();
    assert_eq!(names.len(), expected.len(), "unexpected or duplicate tools");
    assert_eq!(names.into_iter().collect::<HashSet<_>>(), *expected);

    // 预设选择器优先使用 items；结构化条目与兼容工具列表必须一致。
    let items = catalog["items"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter(|item| item["group"] == "builtin")
        .map(|item| {
            item["runtime_name"]
                .as_str()
                .expect("runtime name")
                .to_string()
        })
        .collect::<Vec<_>>();
    assert_eq!(items.len(), expected.len(), "unexpected or duplicate items");
    assert_eq!(items.into_iter().collect::<HashSet<_>>(), *expected);
}

#[tokio::test]
async fn tools_catalog_hides_unavailable_tools_and_resolves_enabled_aliases() {
    let ctx = build_contract_context().await;
    ctx.state
        .config_store
        .update(|config| {
            config.server.mode = "api".to_string();
            config.tools.builtin.enabled = [
                "ptc",
                "programmatic_tool_call",
                "网页搜索",
                "web_search",
                "网页抓取",
                "read_file",
                "get_goal",
                "create_goal",
                "update_goal",
            ]
            .into_iter()
            .map(str::to_string)
            .collect();
            config.tools.web.search.enabled = true;
            config.tools.web.fetch.enabled = false;
            config.tools.browser.enabled = false;
        })
        .await
        .expect("configure tool visibility");

    let catalog = request_tools_catalog(&ctx.server_app).await;
    let read_name = if wunder_server::i18n::get_language()
        .to_lowercase()
        .starts_with("en")
    {
        "read_file"
    } else {
        "读取文件"
    };
    assert_builtin_catalog_names(&catalog, &HashSet::from([read_name.to_string()]));
}

#[tokio::test]
async fn tools_catalog_tracks_browser_switches_without_builtin_whitelist_entries() {
    let ctx = build_contract_context().await;
    for (runtime_enabled, tool_enabled, legacy_entries, visible) in [
        (true, true, false, true),
        (true, false, true, false),
        (false, true, true, false),
        (true, true, false, true),
    ] {
        ctx.state
            .config_store
            .update(|config| {
                config.server.mode = "api".to_string();
                config.browser.enabled = runtime_enabled;
                config.tools.browser.enabled = tool_enabled;
                config.tools.builtin.enabled = if legacy_entries {
                    wunder_server::tools::browser_tool_names()
                } else {
                    Vec::new()
                };
            })
            .await
            .expect("configure browser visibility");

        let catalog = request_tools_catalog(&ctx.server_app).await;
        let expected = if visible {
            HashSet::from([wunder_server::tools::browser_tool_summary_spec(
                &wunder_server::i18n::get_language().to_lowercase(),
            )
            .name])
        } else {
            HashSet::new()
        };
        assert_builtin_catalog_names(&catalog, &expected);
    }
}

#[tokio::test]
async fn default_agents_select_all_available_builtins_in_every_form() {
    let ctx = build_contract_context().await;
    for mode in ["api", "desktop", "cli"] {
        ctx.state
            .config_store
            .update(|config| {
                config.server.mode = mode.to_string();
                config.tools.builtin.enabled =
                    wunder_server::default_tool_profile::default_tool_candidates();
                config.browser.enabled = true;
                config.tools.browser.enabled = true;
            })
            .await
            .expect("configure available builtins");
        let user_id = format!("sample_{mode}");
        let agent = wunder_server::agent_management::owned(&ctx.state, &user_id, "__default__")
            .await
            .expect("resolve default agent");
        let config = ctx.state.config_store.get().await;
        let skills = ctx.state.skills.read().await.clone();
        let available = wunder_server::tools::collect_available_tool_names(&config, &skills, None);
        let expected = wunder_server::default_tool_profile::default_tool_names(&available);
        assert_eq!(
            agent.tool_names.iter().collect::<HashSet<_>>(),
            expected.iter().collect(),
            "{mode}: default selection must follow runtime availability"
        );
        for name in ["浏览器", "文本编辑", "计划面板", "问询面板"] {
            assert!(
                agent.tool_names.iter().any(|item| item == name),
                "{mode}: missing {name}"
            );
            assert!(agent.declared_tool_names.iter().any(|item| item == name));
            assert!(agent
                .ability_items
                .iter()
                .any(|item| item.runtime_name == name));
        }
        let selected: HashSet<String> = agent.tool_names.iter().cloned().collect();
        let expanded = wunder_server::tools::expand_browser_group_selection(selected, &available);
        for member in wunder_server::tools::browser_tool_names() {
            assert!(
                expanded.contains(&member),
                "{mode}: missing browser member {member}"
            );
        }
    }
}

#[tokio::test]
async fn default_agent_upgrades_old_tools_and_preserves_later_manual_selection() {
    use wunder_server::default_agent_protocol::{default_agent_meta_key, DefaultAgentConfig};
    let ctx = build_contract_context().await;
    ctx.state
        .config_store
        .update(|config| {
            config.server.mode = "api".to_string();
            config.tools.builtin.enabled = ["read_file", "edit", "update_plan", "question_panel"]
                .into_iter()
                .map(str::to_string)
                .collect();
        })
        .await
        .expect("configure builtins");
    let user_id = "sample_migration";
    let key = default_agent_meta_key(user_id);
    let legacy = DefaultAgentConfig {
        system_prompt: "Sample instructions".to_string(),
        visible_unit_ids: vec!["sample_unit".to_string()],
        tool_names: vec!["read_file".to_string()],
        declared_tool_names: vec!["read_file".to_string()],
        ..Default::default()
    };
    ctx.state
        .user_store
        .set_meta(&key, &serde_json::to_string(&legacy).unwrap())
        .unwrap();
    let upgraded = wunder_server::agent_management::owned(&ctx.state, user_id, "__default__")
        .await
        .expect("upgrade default agent");
    for name in ["读取文件", "文本编辑", "计划面板", "问询面板"] {
        assert!(upgraded.tool_names.iter().any(|item| item == name));
        assert!(upgraded.declared_tool_names.iter().any(|item| item == name));
        assert!(upgraded
            .ability_items
            .iter()
            .any(|item| item.runtime_name == name));
    }
    assert_eq!(upgraded.system_prompt, legacy.system_prompt);
    let config: DefaultAgentConfig = serde_json::from_str(
        &ctx.state
            .user_store
            .get_meta(&key)
            .expect("load upgraded default config")
            .expect("default config persisted"),
    )
    .expect("parse upgraded default config");
    assert_eq!(config.visible_unit_ids, legacy.visible_unit_ids);
    assert_eq!(config.system_prompt, legacy.system_prompt);

    // 模拟编辑器完整保存一个手动缩小的选择，后续读取不得再次恢复已取消项。
    let manual = DefaultAgentConfig {
        tool_names: vec!["读取文件".to_string()],
        declared_tool_names: vec!["读取文件".to_string()],
        ..legacy
    };
    ctx.state
        .user_store
        .set_meta(&key, &serde_json::to_string(&manual).unwrap())
        .unwrap();
    let read_back = wunder_server::agent_management::owned(&ctx.state, user_id, "__default__")
        .await
        .expect("read manual selection");
    assert_eq!(read_back.tool_names, manual.tool_names);

    // 后续首次启用的新能力自动加入；不会因此恢复之前手动取消的工具。
    ctx.state
        .config_store
        .update(|config| {
            config.browser.enabled = true;
            config.tools.browser.enabled = true;
        })
        .await
        .expect("enable browser later");
    let updated = wunder_server::agent_management::owned(&ctx.state, user_id, "__default__")
        .await
        .expect("initialize newly available capability");
    assert_eq!(
        updated.tool_names.into_iter().collect::<HashSet<_>>(),
        ["读取文件", "浏览器"]
            .into_iter()
            .map(str::to_string)
            .collect()
    );
}

#[tokio::test]
async fn default_agent_initialization_respects_user_tool_permissions() {
    let ctx = build_contract_context().await;
    let user_id = "sample_restricted";
    ctx.state
        .config_store
        .update(|config| {
            config.server.mode = "api".to_string();
            config.tools.builtin.enabled =
                wunder_server::default_tool_profile::default_tool_candidates();
            config.browser.enabled = true;
            config.tools.browser.enabled = true;
        })
        .await
        .expect("configure builtin availability");
    ctx.state
        .user_store
        .set_user_tool_access(user_id, Some(&vec!["读取文件".to_string()]))
        .expect("restrict tool permissions");
    let record = wunder_server::agent_management::owned(&ctx.state, user_id, "__default__")
        .await
        .expect("resolve restricted default agent");
    assert_eq!(record.tool_names, vec!["读取文件".to_string()]);

    ctx.state
        .user_store
        .set_user_tool_access(
            user_id,
            Some(&vec!["读取文件".to_string(), "浏览器".to_string()]),
        )
        .expect("grant browser capability");
    let record = wunder_server::agent_management::owned(&ctx.state, user_id, "__default__")
        .await
        .expect("resolve newly granted capability");
    assert_eq!(
        record.tool_names.into_iter().collect::<HashSet<_>>(),
        ["读取文件", "浏览器"]
            .into_iter()
            .map(str::to_string)
            .collect()
    );
}
