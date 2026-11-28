use super::{format_k, part_detail_rows, part_rows, percent, summary, tiles, DEFAULT_MAP_BOX};
use crate::MobileWindow;
use slint::{ComponentHandle, ModelRc, VecModel};
use std::rc::Rc;
use wunder_desktop::context_map::{ItemRecord, PartRecord, Record};

pub fn open(ui: &MobileWindow, page: &str) {
    let record = Rc::new(sample(page == "context-empty"));
    let fields = summary(&record);
    ui.set_context_window_has_data(record.has_data());
    ui.set_context_window_measured(record.measured);
    ui.set_context_window_used_text(fields.used_text.into());
    ui.set_context_window_capacity_text(fields.capacity_text.into());
    ui.set_context_window_free_text(fields.free_text.into());
    ui.set_context_window_pct_text(fields.pct_text.into());
    ui.set_context_window_model_label(fields.model_label.into());
    ui.set_context_window_turns_text(fields.turns_text.into());
    ui.set_context_window_parts(ModelRc::new(VecModel::from(part_rows(&record))));
    ui.set_context_window_tiles(ModelRc::new(VecModel::from(tiles(
        &record,
        DEFAULT_MAP_BOX.0,
        DEFAULT_MAP_BOX.1,
    ))));

    let weak = ui.as_weak();
    let sample = Rc::clone(&record);
    ui.on_context_map_resized(move |width, height| {
        if width > 0.0 && height > 0.0 {
            if let Some(ui) = weak.upgrade() {
                ui.set_context_window_tiles(ModelRc::new(VecModel::from(tiles(
                    &sample,
                    width.into(),
                    height.into(),
                ))));
            }
        }
    });
    let weak = ui.as_weak();
    let sample = Rc::clone(&record);
    ui.on_context_legend_open(move |slot| {
        if let Some(ui) = weak.upgrade() {
            show_part(&ui, &sample, slot);
        }
    });
    let weak = ui.as_weak();
    let sample = Rc::clone(&record);
    ui.on_context_tile_open(move |slot, tag, name, count| {
        if let Some(ui) = weak.upgrade() {
            if let Some(item) = sample.part(slot).and_then(|part| {
                part.items.iter().find(|item| {
                    item.tag == tag.as_str()
                        && item.name == name.as_str()
                        && item.count == i64::from(count)
                })
            }) {
                show_item(&ui, &sample, slot, item);
            }
        }
    });
    let weak = ui.as_weak();
    ui.on_context_detail_row_toggle(move |row, _, _, _, _| {
        if let Some(ui) = weak.upgrade() {
            if ui.get_context_detail_row_open() == row {
                ui.set_context_detail_row_open(-1);
                ui.set_context_detail_row_text("".into());
            } else {
                ui.set_context_detail_row_open(row);
                ui.set_context_detail_row_text(r#"{ "sample": true }"#.into());
            }
        }
    });
    let weak = ui.as_weak();
    ui.on_context_detail_dismissed(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_context_detail_open(false);
        }
    });
    let weak = ui.as_weak();
    ui.on_close_context_window(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_context_detail_open(false);
            ui.set_context_window_open(false);
        }
    });
    ui.set_context_window_open(true);
    match page {
        "context-part-detail" => show_part(ui, &record, 2),
        "context-item-detail" => show_item(ui, &record, 2, &record.part(2).unwrap().items[0]),
        _ => {}
    }
}

fn show_part(ui: &MobileWindow, record: &Record, slot: i32) {
    let Some(part) = record.part(slot) else {
        return;
    };
    ui.set_context_detail_kind("part".into());
    ui.set_context_detail_slot(slot);
    ui.set_context_detail_value(format_k(part.tokens).into());
    ui.set_context_detail_pct(percent(part.tokens, record.tokens).into());
    ui.set_context_detail_rows(ModelRc::new(VecModel::from(part_detail_rows(record, slot))));
    ui.set_context_detail_row_open(-1);
    ui.set_context_detail_open(true);
}

fn show_item(ui: &MobileWindow, record: &Record, slot: i32, item: &ItemRecord) {
    ui.set_context_detail_kind("item".into());
    ui.set_context_detail_slot(slot);
    ui.set_context_detail_tag(item.tag.as_str().into());
    ui.set_context_detail_name(item.name.as_str().into());
    ui.set_context_detail_count(item.count as i32);
    ui.set_context_detail_value(format_k(item.tokens).into());
    ui.set_context_detail_pct(percent(item.tokens, record.tokens).into());
    ui.set_context_detail_rows(ModelRc::default());
    ui.set_context_detail_text(r#"{ "sample": true }"#.into());
    ui.set_context_detail_open(true);
}

fn sample(empty: bool) -> Record {
    let mut record = Record {
        window: 128_000,
        model: "Demo model".into(),
        ..Default::default()
    };
    if empty {
        return record;
    }
    let item = |tag: &str, name: &str, count, tokens| ItemRecord {
        tag: tag.into(),
        name: name.into(),
        count,
        tokens,
    };
    let part = |slot, items: Vec<ItemRecord>| PartRecord {
        slot,
        tokens: items.iter().map(|item| item.tokens).sum(),
        items,
    };
    record.measured = true;
    record.turns = 6;
    record.parts = vec![
        part(
            1,
            vec![item("prompt", "", 1, 2_100), item("reminder", "", 1, 220)],
        ),
        part(
            2,
            vec![
                item("tool", "sample_reader", 1, 2_800),
                item("tool", "sample_writer", 1, 900),
                item("tool", "sample_filter", 1, 100),
                item("tool", "sample_sort", 1, 90),
            ],
        ),
        part(3, vec![item("image", "", 1, 450)]),
        part(4, vec![item("result", "sample_reader", 2, 1_800)]),
        part(
            5,
            vec![item("turn", "", 5, 1_700), item("turn", "", 6, 2_100)],
        ),
    ];
    record.tokens = record.parts.iter().map(|part| part.tokens).sum();
    record
}

#[cfg(test)]
mod tests;
