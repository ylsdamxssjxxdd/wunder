//! Tool-selection persistence contract for the native settings editor.
//! The Slint editor sends exactly the names returned by `list_tools`;
//! after `update_agent_settings` a fresh `default_agent` must still report
//! them on the built-in `__default__` agent.

use std::sync::{Mutex, MutexGuard};

use wunder_desktop::{args::DesktopArgs, NativeDesktop};

/// 隔离运行时：WUNDER_HOME 决定配置、数据库与工作区的真实位置，`--temp-root`
/// 只管临时文件。锁保证同进程内的测试串行切换 WUNDER_HOME，避免任一测试
/// 读写到开发者的真实 ~/.wunder 状态。
static ISOLATION: Mutex<()> = Mutex::new(());

struct IsolatedRuntime {
    // 字段按声明顺序析构：先关运行时，再放锁，避免下一个测试在旧运行时
    // 收尾期间改写 WUNDER_HOME。
    runtime: NativeDesktop,
    _lock: MutexGuard<'static, ()>,
}

impl std::ops::Deref for IsolatedRuntime {
    type Target = NativeDesktop;

    fn deref(&self) -> &Self::Target {
        &self.runtime
    }
}

fn start_isolated() -> Result<IsolatedRuntime, Box<dyn std::error::Error>> {
    let lock = ISOLATION.lock().unwrap_or_else(|error| error.into_inner());
    let directory = tempfile::tempdir()?;
    let home = directory.path().join("home");
    let config = home.join("config");
    std::fs::create_dir_all(&config)?;
    std::env::set_var("WUNDER_HOME", &home);
    std::fs::write(config.join("wunder.yaml"), "{}\n")?;
    let settings = serde_json::json!({
        "workspace_root": "", "desktop_token": "", "updated_at": 0,
        "lan_mesh": {"enabled": false},
        "llm": {"default": "test-model", "models": {
            "test-model": {"provider": "openai", "model": "test-model", "model_type": "llm",
                           "base_url": "http://127.0.0.1:9/v1", "timeout_s": 5,
                           "support_vision": false}}}
    });
    std::fs::write(
        config.join("desktop.settings.json"),
        serde_json::to_vec(&settings)?,
    )?;
    let mut args = DesktopArgs::native_defaults();
    args.temp_root = Some(directory.path().join("runtime"));
    args.workspace = Some(directory.path().join("workspace"));
    std::mem::forget(directory);
    Ok(IsolatedRuntime {
        runtime: NativeDesktop::start_with_args(args)?,
        _lock: lock,
    })
}

fn save_tools(runtime: &NativeDesktop, tool_names: Vec<String>) -> wunder_desktop::AgentRecord {
    // 内置智能体的名称由预设拥有（桌面表单只读），测试沿用记录里的名称，不编造
    // 展示文案：万一隔离环境失效，也不会把别人的智能体改名成测试字符串。
    let current = runtime.default_agent().expect("default agent");
    runtime
        .update_agent_settings(
            "__default__",
            wunder_desktop::AgentSettingsEdit {
                name: current.name,
                description: String::new(),
                system_prompt: String::new(),
                model_name: String::new(),
                icon_name: String::new(),
                icon_color: String::new(),
                icon: None,
                tool_names,
                preset_questions: Vec::new(),
                approval_mode: "suggest".into(),
                preview_skill: false,
                silent: false,
                prefer_mother: false,
            },
        )
        .expect("update settings")
}

#[test]
fn tool_selection_persists_for_default_agent() {
    let runtime = start_isolated().expect("isolated runtime");
    let tools = runtime.list_tools().expect("tools");
    assert!(!tools.is_empty(), "catalog must expose tools");
    let names = tools
        .iter()
        .map(|tool| tool.name.clone())
        .collect::<Vec<_>>();
    let persisted = save_tools(&runtime, names.clone());
    assert_eq!(persisted.id, "__default__", "the single built-in agent");
    let missing = names
        .iter()
        .filter(|name| !persisted.tool_names.contains(*name))
        .cloned()
        .collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "tools lost after save: {missing:?}\nlisted: {names:?}\npersisted: {:?}",
        persisted.tool_names
    );
}

#[test]
fn desktop_control_is_listed_and_enabled_by_default() {
    let runtime = start_isolated().expect("isolated runtime");
    let tools = runtime.list_tools().expect("tools");
    assert!(
        tools.iter().any(|tool| tool.name == "桌面控制"),
        "desktop control missing from the builtin catalog: {:?}",
        tools.iter().map(|tool| tool.name.as_str()).collect::<Vec<_>>()
    );
    let agent = runtime.default_agent().expect("default agent");
    assert!(
        agent.tool_names.iter().any(|name| name == "桌面控制"),
        "desktop control is not selected by default: {:?}",
        agent.tool_names
    );
}

#[test]
fn tool_catalog_keeps_canonical_identity_and_a_display_title() {
    let runtime = start_isolated().expect("isolated runtime");
    let tools = runtime.list_tools().expect("tools");
    let read = tools
        .iter()
        .find(|tool| tool.name == "读取文件")
        .expect("read_file is part of the builtin catalog");
    assert_eq!(
        read.title, "读取文件",
        "the catalog title is what the settings page shows; the name stays the identity"
    );
    assert!(
        tools
            .iter()
            .filter(|tool| tool.category == "内置工具")
            .all(|tool| !tool.title.trim().is_empty()),
        "every builtin needs a display title, otherwise the UI falls back to identifiers"
    );
}

/// 回归：用户在「工具与技能」里上传的自建技能只存在于用户技能目录与用户
/// 绑定里，不会写进全局 `skills.enabled`。智能体设置页的能力清单必须把它
/// 当成「技能」列出来，否则上传后无法勾选是否加载。
#[test]
fn uploaded_user_skill_is_listed_as_selectable_skill() {
    let runtime = start_isolated().expect("isolated runtime");
    let root = runtime
        .state()
        .user_tool_store
        .get_skill_root(runtime.user_id());
    let skill_dir = root.join("上传的技能");
    std::fs::create_dir_all(&skill_dir).expect("skill dir");
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: 上传的技能\ndescription: 用户上传的自建技能\n---\n\n# 上传的技能\n",
    )
    .expect("skill file");
    runtime
        .state()
        .user_tool_store
        .sync_skills_from_disk(runtime.user_id())
        .expect("sync skills");
    runtime
        .state()
        .user_tool_manager
        .clear_skill_cache(Some(runtime.user_id()));

    let tools = runtime.list_tools().expect("tools");
    let uploaded = tools.iter().find(|tool| tool.name == "上传的技能");
    assert!(
        uploaded.is_some(),
        "uploaded skill missing from the agent catalog: {:?}",
        tools
            .iter()
            .map(|tool| (tool.category.as_str(), tool.name.as_str()))
            .collect::<Vec<_>>()
    );
    assert_eq!(uploaded.unwrap().category, "技能");

    // 勾选后保存必须留存：清单里的身份与智能体 tool_names 的身份一致。
    let persisted = save_tools(&runtime, vec!["上传的技能".into()]);
    assert!(
        persisted.tool_names.iter().any(|name| name == "上传的技能"),
        "uploaded skill was dropped when saving: {:?}",
        persisted.tool_names
    );
}

#[test]
fn settings_updates_are_rejected_for_unknown_agents() {
    let runtime = start_isolated().expect("isolated runtime");
    let result = runtime.update_agent_settings(
        "agent_custom",
        wunder_desktop::AgentSettingsEdit {
            name: "不应该存在".into(),
            description: String::new(),
            system_prompt: String::new(),
            model_name: String::new(),
            icon_name: String::new(),
            icon_color: String::new(),
            icon: None,
            tool_names: Vec::new(),
            preset_questions: Vec::new(),
            approval_mode: "suggest".into(),
            preview_skill: false,
            silent: false,
            prefer_mother: false,
        },
    );
    assert!(result.is_err(), "only the built-in agent is editable");
}

/// 回归：内置智能体的名称由预设拥有，桌面表单把它渲染成只读字段。客户端仍会
/// 把当前名称随表单回传，所以引擎必须忽略这份名称草稿——否则一份陈旧或伪造的
/// 草稿会把用户看到的智能体名称改成无关文案（工具勾选保存尤其常见）。
#[test]
fn builtin_agent_name_ignores_the_client_draft() {
    let runtime = start_isolated().expect("isolated runtime");
    let before = runtime.default_agent().expect("default agent");
    assert!(!before.name.trim().is_empty(), "builtin agent needs a name");

    let saved = runtime
        .update_agent_settings(
            "__default__",
            wunder_desktop::AgentSettingsEdit {
                name: "工具选择".into(),
                description: before.description.clone(),
                system_prompt: before.system_prompt.clone(),
                model_name: String::new(),
                icon_name: String::new(),
                icon_color: String::new(),
                icon: None,
                tool_names: before.tool_names.clone(),
                preset_questions: Vec::new(),
                approval_mode: before.approval_mode.clone(),
                preview_skill: false,
                silent: false,
                prefer_mother: false,
            },
        )
        .expect("save settings");

    assert_eq!(
        saved.name, before.name,
        "the built-in agent's name is preset-owned and must survive a client save"
    );
    assert_eq!(
        runtime
            .default_agent()
            .expect("reload default agent")
            .name,
        before.name,
        "the stored record must keep the preset name too"
    );
}
