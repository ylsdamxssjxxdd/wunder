use crate::model::{
    AppState, MessageView, ModelOption, SessionView, TurnView, WorkflowKind, WorkflowView,
    WorkspaceView,
};

/// Workspaces shown by the fixture rail. The first one is pre-expanded in
/// `demo_state`, so the screenshot always contains a thread group.
pub fn demo_workspaces() -> Vec<WorkspaceView> {
    vec![
        WorkspaceView {
            id: "fixture-workspace".into(),
            name: "本地工作区".into(),
            root_path: "/home/user/workbench".into(),
            icon: "monitor".into(),
            color: "teal".into(),
            thread_count: 3,
        },
        WorkspaceView {
            id: "fixture-research".into(),
            name: "资料整理".into(),
            root_path: "/home/user/research".into(),
            icon: "flask".into(),
            color: "orange".into(),
            thread_count: 1,
        },
    ]
}

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
        sessions: vec![
            SessionView {
                id: "fixture-thread".into(),
                title: "Fixture conversation".into(),
                workspace: "本地工作区".into(),
                workspace_id: Some("fixture-workspace".into()),
                status: "done".into(),
                model_key: Some("fixture-main".into()),
                reasoning_effort: "default".into(),
                context_tokens: Some(12_800),
                context_capacity_tokens: Some(128_000),
                ..Default::default()
            },
            SessionView {
                id: "fixture-thread-running".into(),
                title: "补齐两个刻度用例".into(),
                workspace: "本地工作区".into(),
                workspace_id: Some("fixture-workspace".into()),
                status: "running".into(),
                ..Default::default()
            },
            SessionView {
                id: "fixture-thread-waiting".into(),
                title: "确认验收标准".into(),
                workspace: "本地工作区".into(),
                workspace_id: Some("fixture-workspace".into()),
                status: "waiting_input".into(),
                ..Default::default()
            },
            SessionView {
                id: "fixture-thread-research".into(),
                title: "整理访谈记录".into(),
                workspace: "资料整理".into(),
                workspace_id: Some("fixture-research".into()),
                status: "queued".into(),
                ..Default::default()
            },
        ],
        workspaces: demo_workspaces(),
        workspaces_loaded: true,
        // The rail opens the workspace that owns the selected thread, exactly
        // as `select_session` would after a restore.
        sidebar: {
            let mut tree = crate::sidebar_tree::SidebarTree::default();
            tree.reveal("fixture-workspace");
            tree
        },
        selected_session: Some("fixture-thread".into()),
        models: demo_models(),
        // The opening turn has no user text, so the ruler and footer skip it;
        // the four rounds below are what the screenshot fixture exercises.
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
                text: "## 已整理完成\n\n我已梳理资料中的关键目标和实现约束。建议按以下顺序推进：\n\n1. **确认工作范围**：明确本轮交付内容、验收标准和依赖。\n2. **复用已有能力**：先检查现有模块与配置，减少重复建设。\n3. **分阶段实现**：每个阶段保留可运行的结果，及时验证。\n4. **检查与交付**：完成必要测试，整理使用说明和后续事项。\n\n| 阶段 | 产出 | 验收 |\n| --- | --- | --- |\n| 范围确认 | 交付清单 | 目标与验收标准一致 |\n| 能力复用 | 模块评估 | 不重复建设 |\n\n> 每个阶段都保留可运行的结果，随时可以回退。\n\n```bash\ncargo test -p wunder-egui\n```\n\n这份计划可以直接作为下一步的工作清单。".into(),
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
        }, TurnView {
            id: "fixture-turn-2".into(),
            user: MessageView {
                id: "fixture-user-2".into(),
                text: "先按最小改动来，只调整界面层，不要新增依赖。".into(),
                mine: true,
                state: "completed".into(),
            },
            assistant: MessageView {
                id: "fixture-answer-2".into(),
                text: "好的。我会把改动限制在视图层，复用现有模块，完成后列出受影响的范围。".into(),
                mine: false,
                state: "completed".into(),
            },
            workflow: vec![WorkflowView {
                id: "fixture-tool-2".into(),
                kind: WorkflowKind::Tool,
                tool_name: "read_file".into(),
                targets: vec!["notes/constraints.md".into()],
                detail: "约束：只调整界面层；不新增依赖；保留现有交互行为。".into(),
                title: "read_file".into(),
                preview: "notes/constraints.md".into(),
                state: "completed".into(),
                ..Default::default()
            }],
            process_steps: 1,
            // A settled turn already knows its page; skipping the reload keeps
            // the row from flickering through a loading state on first open.
            process_loaded: true,
            ..Default::default()
        }, TurnView {
            id: "fixture-turn-3".into(),
            user: MessageView {
                id: "fixture-user-3".into(),
                text: "现有测试够用吗？还是需要再补几个用例？".into(),
                mine: true,
                state: "completed".into(),
            },
            assistant: MessageView {
                id: "fixture-answer-3".into(),
                text: "建议补两个用例：一个覆盖刻度出现的边界，一个覆盖工具失败计数。\n\n我先试跑了一次测试，但目标名写错了，命令直接失败；修正目标后就能继续验证。".into(),
                mine: false,
                state: "completed".into(),
            },
            workflow: vec![
                WorkflowView {
                    id: "fixture-thought-3".into(),
                    kind: WorkflowKind::Reasoning,
                    detail: "先看现有用例的覆盖面，再决定补哪两个边界，避免重复建设。".into(),
                    title: "已思考".into(),
                    preview: "先看现有用例的覆盖面，再决定补哪两个边界，避免重复建设。".into(),
                    state: "completed".into(),
                    ..Default::default()
                },
                WorkflowView {
                    id: "fixture-tool-3".into(),
                    kind: WorkflowKind::Tool,
                    tool_name: "run_tests".into(),
                    targets: vec!["crates/wunder-egui".into()],
                    detail: "找不到指定的测试目标，命令以非零状态退出。".into(),
                    title: "run_tests".into(),
                    preview: "crates/wunder-egui".into(),
                    state: "failed".into(),
                    ..Default::default()
                },
            ],
            process_steps: 2,
            process_loaded: true,
            ..Default::default()
        }, TurnView {
            id: "fixture-turn-4".into(),
            user: MessageView {
                id: "fixture-user-4".into(),
                text: "那就按这个方案开始吧。".into(),
                mine: true,
                state: "completed".into(),
            },
            assistant: MessageView {
                id: "fixture-answer-4".into(),
                text: "正在补齐两个用例：先写刻度可见性的边界，再补工具失败计数。".into(),
                mine: false,
                // A live tail keeps the footer in the mixed "running" state
                // while the finished rounds above stay completed.
                state: "streaming".into(),
            },
            ..Default::default()
        }],
        connection: crate::model::ConnectionState::Ready,
        // A small tree so the screenshot exercises the directory panel: one
        // expanded folder with a nested directory, plus two root files.
        workspace_files: vec![
            crate::model::WorkspaceFileView {
                name: "notes".into(),
                path: "notes".into(),
                is_directory: true,
                size: 0,
            },
            crate::model::WorkspaceFileView {
                name: "plan.md".into(),
                path: "plan.md".into(),
                is_directory: false,
                size: 2_048,
            },
            crate::model::WorkspaceFileView {
                name: "readme.md".into(),
                path: "readme.md".into(),
                is_directory: false,
                size: 512,
            },
        ],
        workspace_tree: {
            let mut tree = crate::workspace_tree::Tree::default();
            tree.set_children(
                "notes",
                vec![
                    crate::model::WorkspaceFileView {
                        name: "archive".into(),
                        path: "notes/archive".into(),
                        is_directory: true,
                        size: 0,
                    },
                    crate::model::WorkspaceFileView {
                        name: "overview.md".into(),
                        path: "notes/overview.md".into(),
                        is_directory: false,
                        size: 18_432,
                    },
                ],
            );
            // Pre-expanded: the first frame of the fixture must already show
            // depth-1 rows, otherwise the screenshot cannot prove indentation.
            let _ = tree.expand("notes");
            tree
        },
        // Two files and a directory: enough chips to exercise the strip's
        // layout (icon, name, size, remove button) in the visual baseline.
        attachments: {
            let mut set = crate::composer_attachments::Attachments::default();
            let _ = set.add(crate::composer_attachments::Attachment::new(
                "/home/user/workbench/notes/overview.md",
                "overview.md",
                18_432,
            ));
            let _ = set.add(crate::composer_attachments::Attachment::new(
                "/home/user/workbench/notes/constraints.md",
                "constraints.md",
                4_096,
            ));
            let _ = set.add(crate::composer_attachments::Attachment::directory(
                "/home/user/workbench/assets",
                "assets",
            ));
            set
        },
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_thread_has_multiple_rounds_with_user_text() {
        let state = demo_state();
        // Mirror the ruler's own filter so the test fails when the fixture
        // stops matching what `app.rs` indexes.
        let rounds: Vec<&TurnView> = state
            .turns
            .iter()
            .filter(|turn| !turn.user.text.is_empty())
            .collect();
        // `timeline_ruler::visible` needs more than one round; a single round
        // would silently drop the lane from every screenshot.
        assert!(crate::timeline_ruler::visible(rounds.len()));
        assert!(rounds.len() >= 2);
        // Whitespace-only prompts would pass the ruler filter yet read as an
        // empty round in the transcript.
        assert!(rounds.iter().all(|turn| !turn.user.text.trim().is_empty()));
    }

    #[test]
    fn demo_thread_has_a_round_with_a_failed_tool() {
        let state = demo_state();
        // The footer only prints "N 个工具 · 1 失败" when a tool row is failed;
        // a failed reasoning row intentionally does not count.
        let failed_tool = state.turns.iter().any(|turn| {
            turn.workflow
                .iter()
                .any(|item| item.kind == WorkflowKind::Tool && item.state == "failed")
        });
        assert!(failed_tool);
    }
}
