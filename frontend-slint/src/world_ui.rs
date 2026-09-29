use crate::{MainWindow, WorldContactCard, WorldGroupCard, WorldMessageCard};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::sync::Arc;
use wunder_desktop::NativeDesktop;

pub fn install(app: &MainWindow, api: Arc<NativeDesktop>) {
    let weak = app.as_weak();
    let contact_api = api.clone();
    let open_api = api.clone();
    app.on_refresh_world(move |groups| {
        let Some(app) = weak.upgrade() else { return };
        if app.get_world_loading() {
            return;
        }
        app.set_world_loading(true);
        let offset = app.get_world_offset() as i64;
        let weak = app.as_weak();
        let api = contact_api.clone();
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
                                .map(|item| WorldContactCard {
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
                                .map(|item| WorldGroupCard {
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
    });
    let weak = app.as_weak();
    app.on_open_world_contact(move |index| {
        let Some(app) = weak.upgrade() else { return };
        if app.get_page() == crate::DesktopPage::Groups {
            let Some(group) = app.get_world_groups().row_data(index.max(0) as usize) else {
                return;
            };
            let conversation = group.conversation_id.to_string();
            let title = group.name.to_string();
            if conversation.is_empty() {
                return;
            }
            app.set_world_loading(true);
            let weak = app.as_weak();
            let api = open_api.clone();
            std::thread::spawn(move || {
                let result = api.list_world_messages(&conversation);
                let _ = weak.upgrade_in_event_loop(move |app| {
                    app.set_world_loading(false);
                    match result {
                        Ok(messages) => {
                            app.set_world_active_title(title.into());
                            app.set_world_active_conversation(conversation.into());
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
                        }
                        Err(error) => app.set_status(format!("无法加载群组消息：{error}").into()),
                    }
                });
            });
            return;
        }
        let Some(contact) = app.get_world_contacts().row_data(index.max(0) as usize) else {
            return;
        };
        let peer = contact.user_id.to_string();
        let username = contact.username.to_string();
        app.set_world_loading(true);
        let weak = app.as_weak();
        let api = open_api.clone();
        std::thread::spawn(move || {
            let result = api.create_world_direct_conversation(&peer);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_world_loading(false);
                match result {
                    Ok(conversation) => {
                        app.set_world_active_title(username.into());
                        app.set_world_active_conversation(conversation.clone().into());
                        app.set_status("正在加载消息…".into());
                        let messages = api.list_world_messages(&conversation);
                        match messages {
                            Ok(messages) => app.set_world_messages(ModelRc::new(VecModel::from(
                                messages
                                    .into_iter()
                                    .map(|item| WorldMessageCard {
                                        content: item.content.into(),
                                        mine: item.mine,
                                        sender: item.sender.into(),
                                    })
                                    .collect::<Vec<_>>(),
                            ))),
                            Err(error) => app.set_status(format!("无法加载消息：{error}").into()),
                        }
                    }
                    Err(error) => app.set_status(format!("无法打开单聊：{error}").into()),
                }
            });
        });
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
        std::thread::spawn(move || {
            let result = api.send_world_message(&conversation, content.trim());
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_world_loading(false);
                match result {
                    Ok(item) => {
                        let mut messages = app.get_world_messages().iter().collect::<Vec<_>>();
                        messages.push(WorldMessageCard {
                            content: item.content.into(),
                            mine: item.mine,
                            sender: item.sender.into(),
                        });
                        app.set_world_messages(ModelRc::new(VecModel::from(messages)));
                    }
                    Err(error) => app.set_status(format!("发送消息失败：{error}").into()),
                }
            });
        });
    });
    let weak = app.as_weak();
    let group_api = api.clone();
    app.on_create_world_group(move |name| {
        let Some(app) = weak.upgrade() else { return };
        if app.get_world_loading() || name.trim().is_empty() {
            return;
        }
        app.set_world_loading(true);
        let weak = app.as_weak();
        let api = group_api.clone();
        let name = name.to_string();
        std::thread::spawn(move || {
            let result = api.create_world_group(&name, &[]);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_world_loading(false);
                match result {
                    Ok(_) => {
                        app.set_world_offset(0);
                        app.invoke_refresh_world(true);
                        app.set_status("群组已创建".into());
                    }
                    Err(error) => app.set_status(format!("无法创建群组：{error}").into()),
                }
            });
        });
    });
}
