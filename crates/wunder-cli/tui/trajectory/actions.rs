use super::*;
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

impl TuiApp {
    pub(crate) fn trajectory_view_open(&self) -> bool {
        self.trajectory_view.is_some()
    }
    pub(crate) async fn open_trajectory_view(&mut self, session: Option<String>) -> Result<()> {
        self.trajectory_view = Some(TrajectoryView::new(
            session.unwrap_or_else(|| self.session_id.clone()),
        ));
        self.request_trajectory_page(Direction::Latest);
        Ok(())
    }
    fn trajectory_job<F>(&mut self, work: F)
    where
        F: std::future::Future<Output = Result<Reply>> + Send + 'static,
    {
        let Some(v) = self.trajectory_view.as_mut() else {
            return;
        };
        if v.pending.is_some() {
            return;
        }
        let (sender, receiver) = oneshot::channel();
        let frames = self.frame_requester.clone();
        v.error = None;
        let task = tokio::spawn(async move {
            let result = work.await;
            let _ = sender.send(result);
            frames.schedule_frame();
        });
        v.pending = Some(Pending { receiver, task });
    }
    fn request_trajectory_page(&mut self, direction: Direction) {
        let Some(v) = self.trajectory_view.as_ref() else {
            return;
        };
        if v.pending.is_some() {
            return;
        }
        let before = (direction == Direction::Older)
            .then(|| v.rows.first().map(|r| r.created_seq))
            .flatten()
            .or_else(|| {
                (direction == Direction::Reload)
                    .then(|| v.row().map(|r| r.created_seq.saturating_add(1)))
                    .flatten()
            })
            .or_else(|| {
                (direction == Direction::Latest && v.root.is_none())
                    .then_some(v.return_anchor)
                    .flatten()
                    .map(|n| n.saturating_add(1))
            });
        let after = (direction == Direction::Newer || direction == Direction::Refresh)
            .then(|| v.rows.last().map(|r| r.created_seq))
            .flatten();
        let store = self.runtime.state.storage.clone();
        let user = self.runtime.user_id.clone();
        let session = v.session.clone();
        let root = v.root.clone();
        self.trajectory_job(async move {
            let page = crate::trajectory::read(move || {
                let page = store.thread_view_window(
                    &user,
                    &session,
                    "trajectory",
                    before,
                    after,
                    root.as_deref(),
                    PAGE,
                )?;
                if direction == Direction::Refresh && page.trajectory.is_empty() {
                    // The forward cursor catches every new record. Once caught up,
                    // reread the bounded tail to apply revisions to running items.
                    store.thread_view_window(
                        &user,
                        &session,
                        "trajectory",
                        None,
                        None,
                        root.as_deref(),
                        PAGE,
                    )
                } else {
                    Ok(page)
                }
            })
            .await?;
            Ok(Reply::Page(direction, page))
        });
    }
    fn request_trajectory_search(&mut self) {
        let Some(v) = self.trajectory_view.as_ref() else {
            return;
        };
        if v.pending.is_some() {
            return;
        }
        let query = v.search.trim().to_string();
        if query.is_empty() {
            self.trajectory_view.as_mut().unwrap().hits = None;
            return;
        }
        let store = self.runtime.state.storage.clone();
        let owner = self.runtime.user_id.clone();
        let session = v.session.clone();
        self.trajectory_job(async move {
            let text = query.clone();
            let (rows, more) = crate::trajectory::read(move || -> Result<_> {
                let found = store.thread_view_search(&owner, &session, &text)?;
                let mut seen = HashSet::new();
                let mut rows = Vec::new();
                for hit in found.hits {
                    if !seen.insert(hit.item_id.clone()) {
                        continue;
                    }
                    let mut row = crate::trajectory::public_item_row(
                        store.as_ref(),
                        &owner,
                        &session,
                        &hit.item_id,
                    )?;
                    row.summary = hit.snippet;
                    rows.push(row);
                }
                Ok((rows, found.has_more))
            })
            .await?;
            Ok(Reply::Search(query, rows, more))
        });
    }
    fn request_trajectory_detail(&mut self) {
        let Some(v) = self.trajectory_view.as_mut() else {
            return;
        };
        if v.pending.is_some() {
            return;
        }
        if v.error.is_some() {
            return;
        }
        let Some(row) = v.row().cloned() else {
            return;
        };
        let key = (row.item_id.clone(), row.revision);
        if v.detail_key.as_ref() == Some(&key) {
            return;
        }
        v.detail_key = Some(key.clone());
        v.detail = Detail::default();
        v.detail_scroll = 0;
        v.detail_x = 0;
        let store = self.runtime.state.storage.clone();
        let user = self.runtime.user_id.clone();
        let session = v.session.clone();
        self.trajectory_job(async move {
            let detail = crate::trajectory::read(move || {
                crate::trajectory::load_detail(store.as_ref(), &user, &session, &row)
            })
            .await?;
            Ok(Reply::Detail(key.0, key.1, detail))
        });
    }
    pub(crate) fn poll_trajectory_results(&mut self) {
        let Some(v) = self.trajectory_view.as_mut() else {
            return;
        };
        if let Some(pending) = v.pending.as_mut() {
            match pending.receiver.try_recv() {
                Ok(result) => {
                    v.pending = None;
                    match result {
                        Ok(Reply::Page(direction, page)) => v.apply_page(direction, page),
                        Ok(Reply::Detail(id, revision, detail)) => {
                            if v.row()
                                .is_some_and(|r| r.item_id == id && r.revision == revision)
                            {
                                v.detail = detail;
                            } else {
                                v.detail_key = None;
                            }
                        }
                        Ok(Reply::Search(query, rows, more)) => {
                            if v.search.trim() == query {
                                v.hits = Some(rows);
                                v.selected = 0;
                                v.more_hits = more;
                                v.detail_key = None;
                            }
                        }
                        Ok(Reply::Export(path)) => {
                            v.notice = format!("Exported / 已导出: {}", path.display());
                        }
                        Err(error) => v.error = Some(error.to_string()),
                    }
                }
                Err(oneshot::error::TryRecvError::Closed) => {
                    v.pending = None;
                    v.error = Some("trajectory task stopped / 轨迹任务已停止".into());
                }
                Err(oneshot::error::TryRecvError::Empty) => {}
            }
        }
        let refresh = v.pending.is_none()
            && v.live
            && v.hits.is_none()
            && !v.searching
            && v.refreshed.elapsed() >= REFRESH
            && v.error.is_none();
        if refresh {
            self.request_trajectory_page(Direction::Refresh);
        } else if self
            .trajectory_view
            .as_ref()
            .is_some_and(|v| v.error.is_none())
        {
            self.request_trajectory_detail();
        }
    }
    pub(crate) fn schedule_trajectory_redraw(&self) {
        if self
            .trajectory_view
            .as_ref()
            .is_some_and(|v| v.live && v.hits.is_none())
        {
            self.frame_requester.schedule_frame_in(REFRESH);
        }
    }
    pub(crate) fn paste_trajectory_search(&mut self, text: &str) {
        if let Some(v) = self.trajectory_view.as_mut().filter(|v| v.searching) {
            let remaining = 256usize.saturating_sub(v.search.chars().count());
            v.search
                .extend(text.chars().filter(|c| !c.is_control()).take(remaining));
        }
    }
    pub(crate) async fn handle_trajectory_view_key(&mut self, key: KeyEvent) -> Result<bool> {
        let zh = self.is_zh_language();
        let Some(v) = self.trajectory_view.as_mut() else {
            return Ok(false);
        };
        if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c')) {
            self.trajectory_view = None;
            return Ok(true);
        }
        if v.help {
            if matches!(key.code, KeyCode::Esc | KeyCode::Char('?')) {
                v.help = false;
            }
            return Ok(true);
        }
        if v.searching {
            match key.code {
                KeyCode::Esc => {
                    v.searching = false;
                    v.search.clear();
                    v.hits = None;
                    v.reconcile();
                }
                KeyCode::Enter if v.pending.is_none() => {
                    v.searching = false;
                    self.request_trajectory_search();
                }
                KeyCode::Backspace => {
                    v.search.pop();
                }
                KeyCode::Char(c)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                        && v.search.chars().count() < 256 =>
                {
                    v.search.push(c);
                }
                _ => {}
            }
            return Ok(true);
        }
        if matches!(key.code, KeyCode::Esc | KeyCode::Char('q') | KeyCode::Left) {
            if v.detail_focus {
                v.detail_focus = false;
            } else if v.hits.is_some() {
                v.hits = None;
                v.search.clear();
                v.selected = 0;
                v.detail_key = None;
            } else if v.root.is_some() && v.pending.is_none() {
                v.root = None;
                v.detail_key = None;
                self.request_trajectory_page(Direction::Latest);
            } else {
                self.trajectory_view = None;
            }
            return Ok(true);
        }
        match key.code {
            KeyCode::Char('?') => v.help = true,
            KeyCode::Char('/') | KeyCode::F(3) => v.searching = true,
            KeyCode::Char('d') => v.duration = !v.duration,
            KeyCode::Char('x') => v.tokens = !v.tokens,
            KeyCode::Char('+') | KeyCode::Char('=') => v.zoom = v.zoom.saturating_add(1).min(8),
            KeyCode::Char('-') => v.zoom = v.zoom.saturating_sub(1),
            KeyCode::Char('0') => v.zoom = 0,
            KeyCode::Char('t') => {
                v.fold_turns = !v.fold_turns;
                v.reconcile();
            }
            KeyCode::Char('c') if key.modifiers.is_empty() => {
                v.fold_calls = !v.fold_calls;
                v.reconcile();
            }
            KeyCode::Char(' ') => {
                if let Some(root) = v.row().map(|r| r.root_turn_id.clone()) {
                    if !v.folded.remove(&root) {
                        v.folded.insert(root);
                    }
                    v.reconcile();
                }
            }
            KeyCode::Tab | KeyCode::BackTab => {
                let count = view::tabs(v).len();
                v.tab = (v.tab
                    + if key.code == KeyCode::BackTab || key.modifiers.contains(KeyModifiers::SHIFT)
                    {
                        count - 1
                    } else {
                        1
                    })
                    % count;
                v.detail_scroll = 0;
            }
            KeyCode::Char('i') | KeyCode::Right => v.detail_focus = !v.detail_focus,
            KeyCode::Char('f') if key.modifiers.is_empty() => {
                v.live = !v.live;
            }
            KeyCode::Char('y') => {
                let content = view::detail_text(v, zh);
                self.publish_clipboard_text(&content);
            }
            KeyCode::Char('h') if v.detail_focus => v.detail_x = v.detail_x.saturating_sub(20),
            KeyCode::Char('l') if v.detail_focus => {
                v.detail_x = v
                    .detail_x
                    .saturating_add(20)
                    .min(crate::trajectory::DETAIL_BYTES)
            }
            KeyCode::Char('e') if v.pending.is_none() => {
                let session = v.session.clone();
                let runtime = self.runtime.clone();
                let path = runtime
                    .launch_dir
                    .join(format!("trajectory-{}.jsonl", uuid::Uuid::new_v4()));
                self.trajectory_job(async move {
                    Ok(Reply::Export(
                        crate::trajectory::export(&runtime, session, path).await?,
                    ))
                });
            }
            KeyCode::Char('r') if v.pending.is_none() => {
                v.detail_key = None;
                v.error = None;
                if v.hits.is_some() {
                    self.request_trajectory_search();
                } else if v.newer || !v.live {
                    self.request_trajectory_page(Direction::Reload);
                } else {
                    self.request_trajectory_page(Direction::Refresh);
                }
            }
            KeyCode::Enter if v.pending.is_none() => {
                if let Some(row) = v.row().cloned() {
                    if row.overview_only || v.root.is_none() && !row.root_turn_id.is_empty() {
                        v.return_anchor = (row.created_seq > 0).then_some(row.created_seq);
                        v.live = false;
                        v.root = Some(row.root_turn_id.clone());
                        v.detail_key = None;
                        v.hits = None;
                        v.search.clear();
                        self.request_trajectory_page(Direction::Latest);
                    } else {
                        v.detail_focus = !v.detail_focus;
                    }
                }
            }
            KeyCode::Up | KeyCode::Char('k') => self.navigate_trajectory(-1),
            KeyCode::Down | KeyCode::Char('j') => self.navigate_trajectory(1),
            KeyCode::PageUp => self.navigate_trajectory(-8),
            KeyCode::PageDown => self.navigate_trajectory(8),
            KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.navigate_trajectory(-8)
            }
            KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.navigate_trajectory(8)
            }
            KeyCode::Home => {
                if v.detail_focus {
                    v.detail_scroll = 0;
                } else {
                    v.selected = v.visible().first().copied().unwrap_or(0);
                    self.navigate_trajectory(-1);
                }
            }
            KeyCode::End => {
                if v.detail_focus {
                    v.detail_scroll = v
                        .detail_height
                        .saturating_sub(v.detail_area.height.saturating_sub(3) as usize);
                } else {
                    v.selected = v.visible().last().copied().unwrap_or(0);
                    self.navigate_trajectory(1);
                }
            }
            _ => {}
        }
        self.request_trajectory_detail();
        Ok(true)
    }
    fn navigate_trajectory(&mut self, delta: isize) {
        let Some(v) = self.trajectory_view.as_mut() else {
            return;
        };
        if v.detail_focus {
            v.scroll_detail(delta);
            return;
        }
        v.live = false;
        let old = v.selected;
        v.move_by(delta);
        let visible = v.visible();
        let direction = if delta < 0 && visible.first() == Some(&v.selected) && v.older {
            Some(Direction::Older)
        } else if delta > 0 && visible.last() == Some(&v.selected) && v.newer {
            Some(Direction::Newer)
        } else {
            None
        };
        if v.hits.is_none() {
            if let Some(direction) = direction {
                self.request_trajectory_page(direction);
            }
        }
        if self
            .trajectory_view
            .as_ref()
            .is_some_and(|v| old != v.selected)
        {
            self.request_trajectory_detail();
        }
    }
    pub(crate) fn trajectory_mouse(&mut self, event: MouseEvent) {
        if self.active_approval.is_some()
            || self.active_remote_approval.is_some()
            || self.active_inquiry_panel.is_some()
        {
            return;
        }
        let Some(v) = self.trajectory_view.as_mut() else {
            return;
        };
        let point = ratatui::layout::Position::new(event.column, event.row);
        match event.kind {
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let delta = if event.kind == MouseEventKind::ScrollUp {
                    -3
                } else {
                    3
                };
                if v.detail_area.contains(point) {
                    v.scroll_detail(delta);
                } else {
                    v.detail_focus = false;
                    self.navigate_trajectory(delta);
                }
            }
            MouseEventKind::Down(MouseButton::Left) if v.ledger_area.contains(point) => {
                let offset = event.row.saturating_sub(v.ledger_area.y + 2) as usize;
                if let Some(i) = v.visible().get(v.ledger_offset + offset) {
                    v.selected = *i;
                    v.detail_scroll = 0;
                    v.live = false;
                }
                self.request_trajectory_detail();
            }
            MouseEventKind::Down(MouseButton::Left) if v.timeline_area.contains(point) => {
                let width = v.timeline_area.width.saturating_sub(8) as usize;
                let x = event.column.saturating_sub(v.timeline_area.x + 8) as usize;
                let value = v.timeline_range.0
                    + x as f64 / width.max(1) as f64 * (v.timeline_range.1 - v.timeline_range.0);
                if let Some((i, _)) = v.active_rows().iter().enumerate().min_by(|(_, a), (_, b)| {
                    (row_span(a, v.duration).0 - value)
                        .abs()
                        .total_cmp(&(row_span(b, v.duration).0 - value).abs())
                }) {
                    v.selected = i;
                    v.live = false;
                }
                self.request_trajectory_detail();
            }
            MouseEventKind::Down(MouseButton::Left) if v.tokenbars_area.contains(point) => {
                // 点击消耗带定位对应步骤：列号跳过左侧标签后查绘制期留下的命中表。
                let x = event
                    .column
                    .saturating_sub(v.tokenbars_area.x + view::TOKENBARS_LABEL_COLS)
                    as usize;
                if event.column >= v.tokenbars_area.x + view::TOKENBARS_LABEL_COLS {
                    if let Some(Some(i)) = v.tokenbar_hits.get(x).copied() {
                        v.selected = i;
                        v.detail_scroll = 0;
                        v.live = false;
                        self.request_trajectory_detail();
                    }
                }
            }
            MouseEventKind::Down(MouseButton::Left) if v.detail_area.contains(point) => {
                v.detail_focus = true
            }
            _ => {}
        }
    }
}
