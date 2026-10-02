use super::NativeDesktop;
use anyhow::{anyhow, Result};
use serde_json::Value;
use wunder_server::user_plaza::{
    get_item, import_item, list_items, ListUserPlazaItemsQuery,
};

/// Desktop plaza only carries normal user assets. `hive_pack` items are swarm
/// assets: they never appear in the list and importing one is refused, no
/// matter what the storage contains.
const PLAZA_DESKTOP_KINDS: [&str; 2] = ["worker_card", "skill_pack"];
/// Plan cap for directory-style listings; the plaza list stays bounded too.
const PLAZA_LIST_LIMIT: usize = 100;

#[derive(Clone, Debug)]
pub struct PlazaItemCard {
    pub item_id: String,
    pub kind: String,
    pub title: String,
    pub summary: String,
    pub owner_username: String,
    pub mine: bool,
    /// "current" | "outdated" | "source_missing"
    pub freshness_status: String,
    pub artifact_filename: String,
    pub artifact_size_text: String,
    pub tags: String,
    pub updated_at: String,
}

#[derive(Clone, Debug)]
pub struct PlazaImportOutcome {
    pub kind: String,
    pub title: String,
    pub message: String,
    pub imported_agent_id: String,
}

impl NativeDesktop {
    /// Lists plaza assets visible to the current user, newest first, capped at
    /// 100 entries. `kind` accepts only the desktop kinds.
    pub fn list_plaza_items(&self, kind: Option<&str>) -> Result<Vec<PlazaItemCard>> {
        let kind = match kind.map(str::trim).filter(|value| !value.is_empty()) {
            Some(value) => {
                if !PLAZA_DESKTOP_KINDS.contains(&value) {
                    return Err(anyhow!("桌面广场不提供该类型的资产"));
                }
                Some(value.to_string())
            }
            None => None,
        };
        let items = self.runtime.block_on(list_items(
            self.state(),
            self.user_id(),
            &ListUserPlazaItemsQuery {
                mine_only: false,
                kind,
            },
        ))?;
        let mut cards: Vec<(f64, PlazaItemCard)> = items
            .into_iter()
            .filter(|item| {
                PLAZA_DESKTOP_KINDS.contains(&item.get("kind").and_then(Value::as_str).unwrap_or(""))
            })
            .map(plaza_card_from_item)
            .collect();
        cards.sort_by(|left, right| right.0.total_cmp(&left.0));
        Ok(cards
            .into_iter()
            .map(|(_, card)| card)
            .take(PLAZA_LIST_LIMIT)
            .collect())
    }

    /// Imports one plaza item through the shared service. The kind guard keeps
    /// swarm packs out of the desktop even when their ids are known.
    pub fn import_plaza_item(&self, item_id: &str) -> Result<PlazaImportOutcome> {
        let cleaned = item_id.trim();
        if cleaned.is_empty() {
            return Err(anyhow!("广场资产不存在"));
        }
        let record = self
            .runtime
            .block_on(get_item(self.state(), cleaned))?
            .ok_or_else(|| anyhow!("广场资产不存在"))?;
        if !PLAZA_DESKTOP_KINDS.contains(&record.kind.as_str()) {
            return Err(anyhow!("桌面广场不提供该类型的资产"));
        }
        let user = self.channel_user()?;
        let outcome = self
            .runtime
            .block_on(import_item(self.state(), &user, cleaned))?;
        Ok(PlazaImportOutcome {
            kind: outcome.kind,
            title: outcome.title,
            message: outcome.message,
            imported_agent_id: outcome.imported_agent_id.unwrap_or_default(),
        })
    }
}

fn plaza_card_from_item(item: Value) -> (f64, PlazaItemCard) {
    let updated_ts = item.get("updated_at").and_then(Value::as_f64).unwrap_or(0.0);
    let tags: Vec<String> = item
        .get("tags")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let card = PlazaItemCard {
        artifact_size_text: format_size(item
            .get("artifact_size_bytes")
            .and_then(Value::as_u64)
            .unwrap_or(0)),
        updated_at: super::channels::format_ts(updated_ts),
        tags: tags.join(" · "),
        item_id: text_at(&item, "item_id"),
        kind: text_at(&item, "kind"),
        title: {
            let title = text_at(&item, "title");
            if title.is_empty() { text_at(&item, "source_key") } else { title }
        },
        summary: text_at(&item, "summary"),
        owner_username: text_at(&item, "owner_username"),
        mine: item.get("mine").and_then(Value::as_bool).unwrap_or(false),
        freshness_status: text_at(&item, "freshness_status"),
        artifact_filename: text_at(&item, "artifact_filename"),
    };
    (updated_ts, card)
}

fn text_at(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let kb = bytes as f64 / 1024.0;
    if kb < 1024.0 {
        return format!("{kb:.1} KB");
    }
    format!("{:.1} MB", kb / 1024.0)
}
