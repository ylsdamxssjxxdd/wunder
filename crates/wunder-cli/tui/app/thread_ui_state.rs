//! Owned render state moved between the visible thread and the bounded cache.
use super::*;

#[derive(Default)]
pub(super) struct ThreadUiState {
    input: String,
    input_cursor: usize,
    transcript_offset_from_bottom: usize,
    pending_paste: VecDeque<String>,
    history_draft: String,
    logs: Vec<LogEntry>,
    active_assistant: Option<usize>,
    active_reasoning: Option<usize>,
    assistant_markdown_stream: Option<StreamedMarkdownState>,
    reasoning_markdown_stream: Option<StreamedMarkdownState>,
    command_sessions: CommandSessionDisplayState,
    command_log_indices: HashMap<String, usize>,
    session_warnings: Vec<super::session_warnings::SessionWarning>,
    warning_panel_open: bool,
    tool_log_indices: HashMap<ToolCallKey, usize>,
    pending_temp_tool_cells: VecDeque<PendingTempToolCell>,
    stream_saw_output: bool,
    stream_saw_final: bool,
    stream_received_content_delta: bool,
    stream_tool_markup_open: bool,
    turn_final_answer: String,
    turn_final_stop_reason: Option<String>,
    tool_phase_notice_emitted: bool,
    active_inquiry_panel: Option<InquiryPanelState>,
    inquiry_selected_index: usize,
    last_usage: Option<String>,
    turn_llm_started_at: Option<Instant>,
    turn_llm_active_secs: f64,
    turn_output_tokens: u64,
    turn_tool_calls: u64,
    last_turn_elapsed_secs: Option<f64>,
    last_turn_speed_tps: Option<f64>,
    last_turn_tool_calls: u64,
    pending_attachments: Vec<crate::attachments::PreparedAttachment>,
    pending_attachment_paths: VecDeque<String>,
    pending_large_pastes: Vec<(String, String)>,
    large_paste_counters: HashMap<usize, usize>,
}

impl ThreadUiState {
    pub(super) fn take(app: &mut TuiApp) -> Self {
        Self {
            input: std::mem::take(&mut app.input),
            input_cursor: std::mem::take(&mut app.input_cursor),
            transcript_offset_from_bottom: std::mem::take(&mut app.transcript_offset_from_bottom),
            pending_paste: std::mem::take(&mut app.pending_paste),
            history_draft: std::mem::take(&mut app.history_draft),
            logs: std::mem::take(&mut app.logs),
            active_assistant: std::mem::take(&mut app.active_assistant),
            active_reasoning: std::mem::take(&mut app.active_reasoning),
            assistant_markdown_stream: std::mem::take(&mut app.assistant_markdown_stream),
            reasoning_markdown_stream: std::mem::take(&mut app.reasoning_markdown_stream),
            command_sessions: std::mem::take(&mut app.command_sessions),
            command_log_indices: std::mem::take(&mut app.command_log_indices),
            session_warnings: std::mem::take(&mut app.session_warnings),
            warning_panel_open: std::mem::take(&mut app.warning_panel_open),
            tool_log_indices: std::mem::take(&mut app.tool_log_indices),
            pending_temp_tool_cells: std::mem::take(&mut app.pending_temp_tool_cells),
            stream_saw_output: std::mem::take(&mut app.stream_saw_output),
            stream_saw_final: std::mem::take(&mut app.stream_saw_final),
            stream_received_content_delta: std::mem::take(&mut app.stream_received_content_delta),
            stream_tool_markup_open: std::mem::take(&mut app.stream_tool_markup_open),
            turn_final_answer: std::mem::take(&mut app.turn_final_answer),
            turn_final_stop_reason: std::mem::take(&mut app.turn_final_stop_reason),
            tool_phase_notice_emitted: std::mem::take(&mut app.tool_phase_notice_emitted),
            active_inquiry_panel: std::mem::take(&mut app.active_inquiry_panel),
            inquiry_selected_index: std::mem::take(&mut app.inquiry_selected_index),
            last_usage: std::mem::take(&mut app.last_usage),
            turn_llm_started_at: std::mem::take(&mut app.turn_llm_started_at),
            turn_llm_active_secs: std::mem::take(&mut app.turn_llm_active_secs),
            turn_output_tokens: std::mem::take(&mut app.turn_output_tokens),
            turn_tool_calls: std::mem::take(&mut app.turn_tool_calls),
            last_turn_elapsed_secs: std::mem::take(&mut app.last_turn_elapsed_secs),
            last_turn_speed_tps: std::mem::take(&mut app.last_turn_speed_tps),
            last_turn_tool_calls: std::mem::take(&mut app.last_turn_tool_calls),
            pending_attachments: std::mem::take(&mut app.pending_attachments),
            pending_attachment_paths: std::mem::take(&mut app.pending_attachment_paths),
            pending_large_pastes: std::mem::take(&mut app.pending_large_pastes),
            large_paste_counters: std::mem::take(&mut app.large_paste_counters),
        }
    }

    pub(super) fn restore(self, app: &mut TuiApp) {
        app.input = self.input;
        app.input_cursor = self.input_cursor;
        app.transcript_offset_from_bottom = self.transcript_offset_from_bottom;
        app.pending_paste = self.pending_paste;
        app.history_draft = self.history_draft;
        app.logs = self.logs;
        app.active_assistant = self.active_assistant;
        app.active_reasoning = self.active_reasoning;
        app.assistant_markdown_stream = self.assistant_markdown_stream;
        app.reasoning_markdown_stream = self.reasoning_markdown_stream;
        app.command_sessions = self.command_sessions;
        app.command_log_indices = self.command_log_indices;
        app.session_warnings = self.session_warnings;
        app.warning_panel_open = self.warning_panel_open;
        app.tool_log_indices = self.tool_log_indices;
        app.pending_temp_tool_cells = self.pending_temp_tool_cells;
        app.stream_saw_output = self.stream_saw_output;
        app.stream_saw_final = self.stream_saw_final;
        app.stream_received_content_delta = self.stream_received_content_delta;
        app.stream_tool_markup_open = self.stream_tool_markup_open;
        app.turn_final_answer = self.turn_final_answer;
        app.turn_final_stop_reason = self.turn_final_stop_reason;
        app.tool_phase_notice_emitted = self.tool_phase_notice_emitted;
        app.active_inquiry_panel = self.active_inquiry_panel;
        app.inquiry_selected_index = self.inquiry_selected_index;
        app.last_usage = self.last_usage;
        app.turn_llm_started_at = self.turn_llm_started_at;
        app.turn_llm_active_secs = self.turn_llm_active_secs;
        app.turn_output_tokens = self.turn_output_tokens;
        app.turn_tool_calls = self.turn_tool_calls;
        app.last_turn_elapsed_secs = self.last_turn_elapsed_secs;
        app.last_turn_speed_tps = self.last_turn_speed_tps;
        app.last_turn_tool_calls = self.last_turn_tool_calls;
        app.pending_attachments = self.pending_attachments;
        app.pending_attachment_paths = self.pending_attachment_paths;
        app.pending_large_pastes = self.pending_large_pastes;
        app.large_paste_counters = self.large_paste_counters;
        // Terminal scrollback belongs to the terminal, not to this thread.
        app.reset_scrollback_archive();
        app.invalidate_transcript_metrics();
    }
}
