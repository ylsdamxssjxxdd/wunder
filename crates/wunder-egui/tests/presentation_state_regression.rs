use wunder_egui::{process_panel_visible, AppState, MessageView, TurnView, WorkspaceFileView};

#[test]
fn workspace_refresh_clears_the_stale_row_selection() {
    let mut state = AppState {
        workspace_files: vec![WorkspaceFileView {
            name: "notes.txt".to_owned(),
            path: "notes.txt".to_owned(),
            ..Default::default()
        }],
        workspace_file_error: "previous refresh failed".to_owned(),
        ..Default::default()
    };
    state.select_workspace_file("notes.txt".to_owned());

    state.begin_workspace_refresh();

    assert_eq!(state.selected_workspace_file, None);
    assert!(state.workspace_file_loading);
    assert!(state.workspace_file_error.is_empty());
    assert_eq!(state.workspace_files.len(), 1);
}

#[test]
fn user_turn_with_an_empty_process_keeps_its_process_disclosure() {
    let turn = TurnView {
        user: MessageView {
            text: "summarize the note".to_owned(),
            mine: true,
            ..Default::default()
        },
        process_loaded: true,
        ..Default::default()
    };

    assert!(turn.workflow.is_empty());
    assert_eq!(turn.process_steps, 0);
    assert!(process_panel_visible(&turn));
}
