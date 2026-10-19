use crate::model::{
    AppState, MessageView, ModelOption, SessionView, TurnView, WorkflowKind, WorkflowView,
};

/// Fixed LLM-only model catalogue for the visual review adapter.
///
/// The values stay neutral and local to the fixture; production data comes
/// exclusively from `NativeDesktop::get_desktop_settings`.
pub fn demo_models() -> Vec<ModelOption> {
    vec![
        ModelOption {
            key: "fixture-main".into(),
            provider: "示例提供方".into(),
            model: "通用模型".into(),
            is_default: true,
            max_context_tokens: Some(128_000),
            reasoning_effort: "default".into(),
        },
        ModelOption {
            key: "fixture-reasoning".into(),
            provider: "示例提供方".into(),
            model: "推理模型".into(),
            is_default: false,
            max_context_tokens: Some(64_000),
            reasoning_effort: "high".into(),
        },
    ]
}

/// Neutral content shared with the captured web workbench for visual review.
pub fn demo_state() -> AppState {
    AppState {
        sessions: vec![SessionView {
            id: "fixture-thread".into(),
            title: "Fixture conversation".into(),
            workspace: "本地工作区".into(),
            status: "done".into(),
            model_key: Some("fixture-main".into()),
            reasoning_effort: "default".into(),
            context_tokens: Some(12_800),
            context_capacity_tokens: Some(128_000),
            ..Default::default()
        }],
        selected_session: Some("fixture-thread".into()),
        models: demo_models(),
        turns: vec![TurnView {
            id: "fixture-greeting".into(),
            assistant: MessageView {
                id: "fixture-greeting-body".into(),
                text: "你好！我是你的智能体助手，有什么可以帮你？".into(),
                state: "completed".into(),
                ..Default::default()
            },
            ..Default::default()
        }, TurnView {
            id: "fixture-turn".into(),
            user: MessageView {
                id: "fixture-user".into(),
                text: "请帮我整理这份项目资料，生成一个清晰的实施计划。".into(),
                mine: true,
                state: "completed".into(),
            },
            assistant: MessageView {
                id: "fixture-answer".into(),
                text: "## 已整理完成\n\n我已梳理资料中的关键目标和实现约束。建议按以下顺序推进：\n\n1. **确认工作范围**：明确本轮交付内容、验收标准和依赖。\n2. **复用已有能力**：先检查现有模块与配置，减少重复建设。\n3. **分阶段实现**：每个阶段保留可运行的结果，及时验证。\n4. **检查与交付**：完成必要测试，整理使用说明和后续事项。\n\n这份计划可以直接作为下一步的工作清单。".into(),
                mine: false,
                state: "completed".into(),
            },
            workflow: vec![
                WorkflowView {
                    id: "fixture-thought-1".into(),
                    kind: WorkflowKind::Reasoning,
                    detail: "我先梳理资料中的目标、约束和已有实现，再把工作拆成可以独立验证的步骤。".into(),
                    title: "已思考".into(),
                    preview: "我先梳理资料中的目标、约束和已有实现，再把工作拆成可以独立验证的步骤。".into(),
                    state: "completed".into(),
                    ..Default::default()
                },
                WorkflowView {
                    id: "fixture-tool-1".into(),
                    kind: WorkflowKind::Tool,
                    tool_name: "read_file".into(),
                    targets: vec!["notes/overview.md".into()],
                    detail: "项目目标：整理现有能力，形成可以执行和验证的工作清单。\n交付要求：保持清晰、简洁、可维护。".into(),
                    title: "read_file".into(),
                    preview: "notes/overview.md".into(),
                    state: "completed".into(),
                    ..Default::default()
                },
                WorkflowView {
                    id: "fixture-thought-2".into(),
                    kind: WorkflowKind::Reasoning,
                    detail: "已核对现有配置和相关模块，下面给出建议的推进顺序。".into(),
                    title: "已思考".into(),
                    preview: "已核对现有配置和相关模块，下面给出建议的推进顺序。".into(),
                    state: "completed".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }],
        connection: crate::model::ConnectionState::Ready,
        ..Default::default()
    }
}
