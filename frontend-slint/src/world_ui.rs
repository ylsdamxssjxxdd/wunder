use crate::{DesktopPage, MainWindow, WorldMessageCard};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::sync::Arc;
use std::time::Duration;
use wunder_desktop::{NativeDesktop, WorldMessageTracker};

const LIVE_TICK: Duration = Duration::from_millis(250);
const LIVE_DRAIN_MAX: usize = 64;

/// Process-wide dedupe and paging cursors for world conversations. Static
/// because the shared event feed outlives any single page switch.
struct WorldUiState {
    tracker: WorldMessageTracker,
    oldest: std::sync::Mutex<std::collections::HashMap<String, i64>>,
}

impl WorldUiState {
    fn observe_oldest(&self, conversation: &str, message_id: i64) {
        if message_id <= 0 {
            return;
        }
        let mut guard = self.oldest.lock().unwrap_or_else(|e| e.into_inner());
        let entry = guard
            .entry(conversation.trim().to_string())
            .or_insert(message_id);
        if message_id < *entry {
            *entry = message_id;
        }
    }

    fn oldest(&self, conversation: &str) -> Option<i64> {
        let guard = self.oldest.lock().unwrap_or_else(|e| e.into_inner());
        guard.get(conversation.trim()).copied().filter(|id| *id > 0)
    }
}

fn world_state() -> &'static WorldUiState {
    static STATE: std::sync::OnceLock<WorldUiState> = std::sync::OnceLock::new();
    STATE.get_or_init(|| WorldUiState {
        tracker: WorldMessageTracker::default(),
        oldest: std::sync::Mutex::new(std::collections::HashMap::new()),
    })
}

pub fn install(app: &MainWindow, api: Arc<NativeDesktop>) {
    let weak = app.as_weak();
    let contact_api = api.clone();
    app.on_refresh_world(move |groups| {
        let Some(app) = weak.upgrade() else { return };
        refresh_lists(&app, &contact_api, groups);
    });
    let weak = app.as_weak();
    let open_api = api.clone();
    app.on_open_world_contact(move |index| {
        let Some(app) = weak.upgrade() else { return };
        if app.get_page() == DesktopPage::Groups {
            let Some(group) = app.get_world_groups().row_data(index.max(0) as usize) else {
                return;
            };
            open_group_conversation(
                &app,
                &open_api,
                &group.group_id,
                &group.conversation_id,
                &group.name,
            );
            return;
        }
        let Some(contact) = app.get_world_contacts().row_data(index.max(0) as usize) else {
            return;
        };
        open_direct_conversation(&app, &open_api, &contact.user_id, &contact.username);
    });
    let weak = app.as_weak();
    let send_api = api.clone();
    app.on_send_world_message(move |content| {
        let Some(app) = weak.upgrade() else { return };
        let conversation = app.get_world_active_conversation().to_string();
        if conversation.is_empty() || content.trim().is_empty() || app.get_world_loading() {
            return;
        }
        app.set_world_loading(true);
        let weak = app.as_weak();
        let api = send_api.clone();
        let content = content.to_string();
        std::thread::spawn(move || {
            let result = api.send_world_message(&conversation, content.trim());
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_world_loading(false);
                match result {
                    Ok(item) => {
                        world_state()
                            .tracker
                            .observe(&conversation, item.id);
                        let mut messages = current_messages(&app);
                        messages.push(WorldMessageCard {
                            content: item.content.into(),
                            mine: item.mine,
                            sender: item.sender.into(),
                        });
                        app.set_world_messages(ModelRc::new(VecModel::from(messages)));
                        let groups = app.get_page() == DesktopPage::Groups;
                        app.invoke_refresh_world(groups);
                    }
                    Err(error) => app.set_status(format!("发送消息失败：{error}").into()),
                }
            });
        });
    });
    let weak = app.as_weak();
    let creator_api = api.clone();
    app.on_open_world_group_creator(move || {
        let Some(app) = weak.upgrade() else { return };
        app.set_world_group_creator_open(true);
        app.set_world_group_picks(ModelRc::default());
        if app.get_world_loading() {
            return;
        }
        app.set_world_loading(true);
        let weak = app.as_weak();
        let api = creator_api.clone();
        std::thread::spawn(move || {
            let result = api.list_world_contacts("", 0);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_world_loading(false);
                match result {
                    Ok((contacts, _)) => {
                        app.set_world_group_picks(ModelRc::new(VecModel::from(
                            contacts
                                .into_iter()
                                .map(|contact| crate::WorldPickRow {
                                    user_id: contact.user_id.into(),
                                    username: contact.username.into(),
                                    selected: false,
                                })
                                .collect::<Vec<_>>(),
                        )));
                    }
                    Err(error) => app.set_status(format!("无法读取联系人：{error}").into()),
                }
            });
        });
    });
    let weak = app.as_weak();
    app.on_close_world_group_creator(move || {
        let Some(app) = weak.upgrade() else { return };
        app.set_world_group_creator_open(false);
        app.set_world_group_picks(ModelRc::default());
    });
    let weak = app.as_weak();
    app.on_toggle_world_group_member(move |user_id| {
        let Some(app) = weak.upgrade() else { return };
        let picks: Vec<crate::WorldPickRow> = app.get_world_group_picks().iter().collect();
        if picks.is_empty() {
            return;
        }
        let user_id = user_id.to_string();
        let mut updated: Vec<crate::WorldPickRow> = Vec::with_capacity(picks.len());
        let mut toggled = false;
        for mut pick in picks {
            if pick.user_id == user_id {
                pick.selected = !pick.selected;
                toggled = true;
            }
            updated.push(pick);
        }
        if toggled {
            app.set_world_group_picks(ModelRc::new(VecModel::from(updated)));
        }
    });
    let weak = app.as_weak();
    let group_api = api.clone();
    app.on_create_world_group(move |name| {
        let Some(app) = weak.upgrade() else { return };
        if app.get_world_loading() || name.trim().is_empty() {
            return;
        }
        let members: Vec<String> = app
            .get_world_group_picks()
            .iter()
            .filter(|pick| pick.selected)
            .map(|pick| pick.user_id.to_string())
            .collect();
        if members.is_empty() {
            app.set_status("请至少选择 1 名初始成员".into());
            return;
        }
        app.set_world_loading(true);
        let weak = app.as_weak();
        let api = group_api.clone();
        let name = name.to_string();
        std::thread::spawn(move || {
            let result = api.create_world_group(&name, &members);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_world_loading(false);
                match result {
                    Ok(_) => {
                        app.set_world_group_creator_open(false);
                        app.set_world_group_picks(ModelRc::default());
                        app.set_world_offset(0);
                        app.invoke_refresh_world(true);
                        app.set_status("群组已创建".into());
                    }
                    Err(error) => app.set_status(format!("无法创建群组：{error}").into()),
                }
            });
        });
    });
    let weak = app.as_weak();
    app.on_edit_world_announcement(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_world_active_group().is_empty() {
            return;
        }
        app.set_world_announcement_editing(true);
    });
    let weak = app.as_weak();
    app.on_cancel_world_announcement(move || {
        let Some(app) = weak.upgrade() else { return };
        app.set_world_announcement_editing(false);
    });
    let weak = app.as_weak();
    let announce_api = api.clone();
    app.on_save_world_announcement(move |text| {
        let Some(app) = weak.upgrade() else { return };
        let group = app.get_world_active_group().to_string();
        if group.is_empty() || app.get_world_loading() {
            return;
        }
        app.set_world_loading(true);
        let weak = app.as_weak();
        let api = announce_api.clone();
        let text = text.to_string();
        std::thread::spawn(move || {
            let result = api
                .update_world_group_announcement(&group, &text)
                .and_then(|_| api.get_world_group_detail(&group));
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_world_loading(false);
                match result {
                    Ok(detail) => {
                        app.set_world_announcement(detail.announcement.into());
                        app.set_world_announcement_editing(false);
                        app.set_status("公告已更新".into());
                    }
                    Err(error) => app.set_status(format!("公告保存失败：{error}").into()),
                }
            });
        });
    });
    let weak = app.as_weak();
    let earlier_api = api.clone();
    app.on_load_earlier_world_messages(move || {
        let Some(app) = weak.upgrade() else { return };
        let conversation = app.get_world_active_conversation().to_string();
        let Some(oldest) = world_state().oldest(&conversation) else {
            return;
        };
        if app.get_world_loading() {
            return;
        }
        app.set_world_loading(true);
        let weak = app.as_weak();
        let api = earlier_api.clone();
        std::thread::spawn(move || {
            let result = api
                .list_world_messages(&conversation, Some(oldest))
                .and_then(|page| {
                    let oldest = page.first().map(|item| item.id).unwrap_or(0);
                    api.has_older_world_messages(&conversation, oldest)
                        .map(|has_older| (page, oldest, has_older))
                });
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_world_loading(false);
                match result {
                    Ok((page, oldest, has_older)) => {
                        // Storage pages newest-first; the older window belongs
                        // above the already-rendered messages.
                        let mut messages: Vec<WorldMessageCard> = page
                            .iter()
                            .rev()
                            .map(|item| WorldMessageCard {
                                content: item.content.clone().into(),
                                mine: item.mine,
                                sender: item.sender.clone().into(),
                            })
                            .collect();
                        messages.extend(current_messages(&app));
                        app.set_world_messages(ModelRc::new(VecModel::from(messages)));
                        world_state().observe_oldest(&conversation, oldest);
                        app.set_world_has_earlier(has_older);
                    }
                    Err(error) => app.set_status(format!("无法加载更早消息：{error}").into()),
                }
            });
        });
    });
    install_live_feed(app, &api);
}

fn current_messages(app: &MainWindow) -> Vec<WorldMessageCard> {
    app.get_world_messages().iter().collect()
}

fn refresh_lists(app: &MainWindow, api: &Arc<NativeDesktop>, groups: bool) {
    if app.get_world_loading() {
        return;
    }
    app.set_world_loading(true);
    let offset = app.get_world_offset() as i64;
    let weak = app.as_weak();
    let api = api.clone();
    std::thread::spawn(move || {
        let result = if groups {
            api.list_world_groups(offset)
                .map(|(items, total)| (Vec::new(), items, total))
        } else {
            api.list_world_contacts("", offset)
                .map(|(items, total)| (items, Vec::new(), total))
        };
        let _ = weak.upgrade_in_event_loop(move |app| {
            app.set_world_loading(false);
            match result {
                Ok((contacts, groups, total)) => {
                    app.set_world_total(total.min(i32::MAX as i64) as i32);
                    app.set_world_contacts(ModelRc::new(VecModel::from(
                        contacts
                            .into_iter()
                            .map(|item| crate::WorldContactCard {
                                user_id: item.user_id.into(),
                                username: item.username.into(),
                                online: item.online,
                                preview: item.preview.into(),
                                unread: item.unread.min(i32::MAX as i64) as i32,
                            })
                            .collect::<Vec<_>>(),
                    )));
                    app.set_world_groups(ModelRc::new(VecModel::from(
                        groups
                            .into_iter()
                            .map(|item| crate::WorldGroupCard {
                                group_id: item.group_id.into(),
                                conversation_id: item.conversation_id.into(),
                                name: item.name.into(),
                                members: item.members.min(i32::MAX as i64) as i32,
                                preview: item.preview.into(),
                                unread: item.unread.min(i32::MAX as i64) as i32,
                            })
                            .collect::<Vec<_>>(),
                    )));
                }
                Err(error) => app.set_status(format!("无法读取用户世界：{error}").into()),
            }
        });
    });
}

fn open_direct_conversation(
    app: &MainWindow,
    api: &Arc<NativeDesktop>,
    peer: &str,
    username: &str,
) {
    app.set_world_loading(true);
    app.set_world_active_group("".into());
    app.set_world_announcement("".into());
    app.set_world_announcement_editing(false);
    let weak = app.as_weak();
    let api = api.clone();
    let peer = peer.to_string();
    let username = username.to_string();
    std::thread::spawn(move || {
        let result = api
            .create_world_direct_conversation(&peer)
            .and_then(|conversation| {
                api.list_world_messages(&conversation, None)
                    .map(|messages| (conversation, messages))
            });
        let _ = weak.upgrade_in_event_loop(move |app| {
            app.set_world_loading(false);
            match result {
                Ok((conversation, messages)) => {
                    app.set_world_active_title(username.into());
                    app.set_world_active_conversation(conversation.clone().into());
                    apply_message_snapshot(&app, &api, &conversation, messages);
                }
                Err(error) => app.set_status(format!("无法打开单聊：{error}").into()),
            }
        });
    });
}

fn open_group_conversation(
    app: &MainWindow,
    api: &Arc<NativeDesktop>,
    group_id: &str,
    conversation_id: &str,
    title: &str,
) {
    if conversation_id.is_empty() {
        return;
    }
    app.set_world_loading(true);
    app.set_world_active_group(group_id.into());
    app.set_world_announcement("".into());
    app.set_world_announcement_editing(false);
    let weak = app.as_weak();
    let api = api.clone();
    let conversation = conversation_id.to_string();
    let title = title.to_string();
    let group = group_id.to_string();
    std::thread::spawn(move || {
        let detail = api.get_world_group_detail(&group);
        let messages = api.list_world_messages(&conversation, None);
        let _ = weak.upgrade_in_event_loop(move |app| {
            app.set_world_loading(false);
            match messages {
                Ok(messages) => {
                    app.set_world_active_title(title.into());
                    app.set_world_active_conversation(conversation.clone().into());
                    apply_message_snapshot(&app, &api, &conversation, messages);
                }
                Err(error) => app.set_status(format!("无法加载群组消息：{error}").into()),
            }
            match detail {
                Ok(detail) => app.set_world_announcement(detail.announcement.into()),
                Err(error) => app.set_status(format!("无法读取群公告：{error}").into()),
            }
        });
    });
}

/// Project a freshly loaded message page: record dedupe cursors, render the
/// snapshot, expose the "load earlier" affordance, then clear unread state.
fn apply_message_snapshot(
    app: &MainWindow,
    api: &Arc<NativeDesktop>,
    conversation: &str,
    messages: Vec<wunder_desktop::WorldMessage>,
) {
    let state = world_state();
    let oldest = messages.first().map(|item| item.id).unwrap_or(0);
    for item in &messages {
        state.tracker.observe(conversation, item.id);
    }
    state.observe_oldest(conversation, oldest);
    app.set_world_messages(ModelRc::new(VecModel::from(
        messages
            .into_iter()
            .map(|item| WorldMessageCard {
                content: item.content.into(),
                mine: item.mine,
                sender: item.sender.into(),
            })
            .collect::<Vec<_>>(),
    )));
    let conversation = conversation.to_string();
    let api = api.clone();
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let has_older = api
            .has_older_world_messages(&conversation, oldest)
            .unwrap_or(false);
        let _ = api.mark_world_read(&conversation, None);
        let _ = weak.upgrade_in_event_loop(move |app| {
            app.set_world_has_earlier(has_older);
            app.invoke_refresh_world(app.get_page() == DesktopPage::Groups);
        });
    });
}

/// Bridge the shared runtime broadcast feed to the UI thread. The feed lives
/// for the whole process; a 250 ms timer drains at most 64 events per tick so
/// one tick can never stall rendering. Overflow recovers through snapshots.
fn install_live_feed(app: &MainWindow, api: &Arc<NativeDesktop>) {
    let feed = match api.start_world_event_feed() {
        Ok(feed) => feed,
        Err(error) => {
            app.set_status(format!("实时消息订阅失败：{error}").into());
            return;
        }
    };
    let weak = app.as_weak();
    let api = api.clone();
    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::Repeated, LIVE_TICK, move || {
        let Some(app) = weak.upgrade() else { return };
        let overflow = feed.take_overflowed();
        let events = feed.drain(LIVE_DRAIN_MAX);
        if !overflow && events.is_empty() {
            return;
        }
        let active = app.get_world_active_conversation().to_string();
        let mut live_cards: Vec<WorldMessageCard> = Vec::new();
        let mut touched_active = false;
        let mut touched_background = false;
        for event in events {
            if event.conversation_id != active {
                touched_background = true;
                continue;
            }
            let Some(message) = event.message else { continue };
            let state = world_state();
            if state.tracker.seen(&event.conversation_id, message.id) {
                continue;
            }
            state.tracker.observe(&event.conversation_id, message.id);
            live_cards.push(WorldMessageCard {
                content: message.content.into(),
                mine: message.mine,
                sender: message.sender.into(),
            });
            touched_active = true;
        }
        if overflow && !active.is_empty() {
            reload_active_conversation(&app, &api, &active);
        }
        if touched_active {
            let mut messages = current_messages(&app);
            messages.extend(live_cards);
            app.set_world_messages(ModelRc::new(VecModel::from(messages)));
            let api = api.clone();
            let weak = weak.clone();
            std::thread::spawn(move || {
                let _ = api.mark_world_read(&active, None);
                let _ = weak.upgrade_in_event_loop(move |app| {
                    app.invoke_refresh_world(app.get_page() == DesktopPage::Groups);
                });
            });
        }
        if overflow || touched_background {
            app.invoke_refresh_world(app.get_page() == DesktopPage::Groups);
        }
    });
    // The timer must outlive this function; the feed runs for the process.
    std::mem::forget(timer);
}

/// Snapshot recovery used after an event feed overflow: reload the open
/// conversation from storage instead of trusting partial incremental updates.
fn reload_active_conversation(app: &MainWindow, api: &Arc<NativeDesktop>, conversation: &str) {
    let title = app.get_world_active_title().to_string();
    let group = app.get_world_active_group().to_string();
    let weak = app.as_weak();
    let api = api.clone();
    let conversation = conversation.to_string();
    std::thread::spawn(move || {
        let messages = api.list_world_messages(&conversation, None);
        let detail = (!group.is_empty()).then(|| api.get_world_group_detail(&group));
        let _ = weak.upgrade_in_event_loop(move |app| {
            match messages {
                Ok(messages) => {
                    app.set_world_active_title(title.into());
                    apply_message_snapshot(&app, &api, &conversation, messages);
                }
                Err(error) => app.set_status(format!("无法恢复会话：{error}").into()),
            }
            if let Some(Ok(detail)) = detail {
                app.set_world_announcement(detail.announcement.into());
            }
        });
    });
}
