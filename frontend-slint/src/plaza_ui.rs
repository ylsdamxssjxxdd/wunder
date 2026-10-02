use crate::{MainWindow, PlazaItemCard};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::sync::Arc;
use wunder_desktop::NativeDesktop;

pub fn install(app: &MainWindow, api: Arc<NativeDesktop>) {
    let weak = app.as_weak();
    let list_api = api.clone();
    app.on_refresh_plaza(move || {
        let Some(app) = weak.upgrade() else { return };
        refresh_plaza(&app, &list_api);
    });
    let weak = app.as_weak();
    app.on_select_plaza_item(move |index| {
        let Some(app) = weak.upgrade() else { return };
        app.set_selected_plaza_item(index);
    });
    let weak = app.as_weak();
    app.on_request_plaza_import(move || {
        let Some(app) = weak.upgrade() else { return };
        if selected_item(&app).is_some() {
            app.set_plaza_import_open(true);
        }
    });
    let weak = app.as_weak();
    app.on_cancel_plaza_import(move || {
        let Some(app) = weak.upgrade() else { return };
        app.set_plaza_import_open(false);
    });
    let weak = app.as_weak();
    let import_api = api.clone();
    app.on_confirm_plaza_import(move || {
        let Some(app) = weak.upgrade() else { return };
        let Some(item) = selected_item(&app) else { return };
        app.set_plaza_import_open(false);
        let api = import_api.clone();
        let weak = app.as_weak();
        let item_id = item.item_id.to_string();
        let title = item.title.to_string();
        std::thread::spawn(move || {
            let result = api.import_plaza_item(&item_id);
            let _ = weak.upgrade_in_event_loop(move |app| match result {
                Ok(outcome) => {
                    app.set_status(format!("已导入「{}」：{}", title, outcome.message).into());
                    refresh_plaza(&app, &api);
                }
                Err(error) => app.set_status(format!("导入失败：{error}").into()),
            });
        });
    });
}

fn selected_item(app: &MainWindow) -> Option<PlazaItemCard> {
    let selected = app.get_selected_plaza_item();
    if selected < 0 {
        return None;
    }
    app.get_plaza_items().iter().nth(selected as usize)
}

fn refresh_plaza(app: &MainWindow, api: &Arc<NativeDesktop>) {
    if app.get_plaza_loading() {
        return;
    }
    app.set_plaza_loading(true);
    let weak = app.as_weak();
    let api = api.clone();
    std::thread::spawn(move || {
        let result = api.list_plaza_items(None);
        let _ = weak.upgrade_in_event_loop(move |app| {
            app.set_plaza_loading(false);
            match result {
                Ok(items) => {
                    app.set_plaza_items(ModelRc::new(VecModel::from(
                        items
                            .into_iter()
                            .map(|item| PlazaItemCard {
                                item_id: item.item_id.into(),
                                kind: item.kind.into(),
                                title: item.title.into(),
                                summary: item.summary.into(),
                                owner: item.owner_username.into(),
                                mine: item.mine,
                                freshness: item.freshness_status.into(),
                                artifact: item.artifact_filename.into(),
                                size: item.artifact_size_text.into(),
                                tags: item.tags.into(),
                                updated_at: item.updated_at.into(),
                            })
                            .collect::<Vec<_>>(),
                    )));
                }
                Err(error) => app.set_status(format!("无法读取广场资产：{error}").into()),
            }
        });
    });
}
