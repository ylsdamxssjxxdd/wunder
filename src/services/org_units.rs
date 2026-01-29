use crate::storage::OrgUnitRecord;
use crate::user_store::UserStore;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use uuid::Uuid;

const ORG_UNIT_SEED_PATH: &str = "config/org_units.json";
const MAX_ORG_UNIT_LEVEL: i32 = 4;

#[derive(Debug, Deserialize)]
struct OrgUnitSeed {
    name: String,
    #[serde(default)]
    children: Vec<OrgUnitSeed>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OrgUnitNode {
    pub unit_id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub level: i32,
    pub path: String,
    pub path_name: String,
    pub sort_order: i64,
    pub leader_ids: Vec<String>,
    pub children: Vec<OrgUnitNode>,
}

pub fn seed_org_units_if_empty(user_store: &UserStore) -> Result<usize> {
    let existing = user_store.list_org_units()?;
    if !existing.is_empty() {
        return Ok(0);
    }
    let seeds = load_seed_units(Path::new(ORG_UNIT_SEED_PATH))?;
    let now = now_ts();
    let mut records = Vec::new();
    for (index, seed) in seeds.iter().enumerate() {
        build_records(seed, None, &[], &[], index as i64, 1, now, &mut records)?;
    }
    for record in &records {
        user_store.upsert_org_unit(record)?;
    }
    Ok(records.len())
}

pub fn build_unit_tree(units: &[OrgUnitRecord]) -> Vec<OrgUnitNode> {
    let mut record_map: HashMap<String, OrgUnitRecord> = HashMap::new();
    let mut children_map: HashMap<Option<String>, Vec<String>> = HashMap::new();
    for unit in units {
        record_map.insert(unit.unit_id.clone(), unit.clone());
        children_map
            .entry(unit.parent_id.clone())
            .or_default()
            .push(unit.unit_id.clone());
    }
    for (_, children) in children_map.iter_mut() {
        children.sort_by(|left, right| {
            let left_record = record_map.get(left);
            let right_record = record_map.get(right);
            match (left_record, right_record) {
                (Some(left), Some(right)) => left
                    .sort_order
                    .cmp(&right.sort_order)
                    .then_with(|| left.name.cmp(&right.name)),
                _ => left.cmp(right),
            }
        });
    }

    let roots = children_map.get(&None).cloned().unwrap_or_default();
    roots
        .into_iter()
        .filter_map(|unit_id| build_node(&unit_id, &record_map, &children_map))
        .collect()
}

pub fn resolve_leader_root_ids(user_id: &str, units: &[OrgUnitRecord]) -> Vec<String> {
    let cleaned = user_id.trim();
    if cleaned.is_empty() {
        return Vec::new();
    }
    units
        .iter()
        .filter(|unit| unit.leader_ids.iter().any(|id| id == cleaned))
        .map(|unit| unit.unit_id.clone())
        .collect()
}

pub fn collect_descendant_unit_ids(
    units: &[OrgUnitRecord],
    root_ids: &[String],
) -> HashSet<String> {
    if root_ids.is_empty() {
        return HashSet::new();
    }
    let mut path_map = HashMap::new();
    for unit in units {
        path_map.insert(unit.unit_id.clone(), unit.path.clone());
    }
    let mut root_paths = Vec::new();
    for root_id in root_ids {
        if let Some(path) = path_map.get(root_id) {
            root_paths.push(path.clone());
        }
    }
    let mut output = HashSet::new();
    for unit in units {
        for root_path in &root_paths {
            if unit.path == *root_path || unit.path.starts_with(&format!("{root_path}/")) {
                output.insert(unit.unit_id.clone());
                break;
            }
        }
    }
    output
}

pub fn resolve_default_root_unit(units: &[OrgUnitRecord]) -> Option<OrgUnitRecord> {
    let mut roots: Vec<OrgUnitRecord> = units
        .iter()
        .filter(|unit| unit.parent_id.is_none())
        .cloned()
        .collect();
    roots.sort_by(|left, right| {
        left.sort_order
            .cmp(&right.sort_order)
            .then_with(|| left.name.cmp(&right.name))
    });
    roots.into_iter().next()
}

pub fn resolve_units_by_ids(
    units: &[OrgUnitRecord],
    target_ids: &HashSet<String>,
) -> HashMap<String, OrgUnitRecord> {
    units
        .iter()
        .filter(|unit| target_ids.contains(&unit.unit_id))
        .map(|unit| (unit.unit_id.clone(), unit.clone()))
        .collect()
}

fn build_node(
    unit_id: &str,
    record_map: &HashMap<String, OrgUnitRecord>,
    children_map: &HashMap<Option<String>, Vec<String>>,
) -> Option<OrgUnitNode> {
    let record = record_map.get(unit_id)?;
    let children_ids = children_map
        .get(&Some(unit_id.to_string()))
        .cloned()
        .unwrap_or_default();
    let children = children_ids
        .into_iter()
        .filter_map(|child_id| build_node(&child_id, record_map, children_map))
        .collect();
    Some(OrgUnitNode {
        unit_id: record.unit_id.clone(),
        parent_id: record.parent_id.clone(),
        name: record.name.clone(),
        level: record.level,
        path: record.path.clone(),
        path_name: record.path_name.clone(),
        sort_order: record.sort_order,
        leader_ids: record.leader_ids.clone(),
        children,
    })
}

fn load_seed_units(path: &Path) -> Result<Vec<OrgUnitSeed>> {
    let content = std::fs::read_to_string(path)?;
    let seeds: Vec<OrgUnitSeed> =
        serde_json::from_str(&content).map_err(|err| anyhow!(err.to_string()))?;
    Ok(seeds)
}

fn build_records(
    seed: &OrgUnitSeed,
    parent_id: Option<String>,
    parent_path_ids: &[String],
    parent_path_names: &[String],
    sort_order: i64,
    level: i32,
    now: f64,
    output: &mut Vec<OrgUnitRecord>,
) -> Result<()> {
    let name = seed.name.trim();
    if name.is_empty() {
        return Err(anyhow!("org unit name is empty"));
    }
    if level > MAX_ORG_UNIT_LEVEL {
        return Err(anyhow!("org unit level exceeds {MAX_ORG_UNIT_LEVEL}"));
    }
    let mut path_names = parent_path_names.to_vec();
    path_names.push(name.to_string());
    let unit_id = build_unit_id(&path_names);
    let mut path_ids = parent_path_ids.to_vec();
    path_ids.push(unit_id.clone());
    let path = path_ids.join("/");
    let path_name = path_names.join(" / ");
    output.push(OrgUnitRecord {
        unit_id: unit_id.clone(),
        parent_id: parent_id.clone(),
        name: name.to_string(),
        level,
        path,
        path_name,
        sort_order,
        leader_ids: Vec::new(),
        created_at: now,
        updated_at: now,
    });
    for (index, child) in seed.children.iter().enumerate() {
        build_records(
            child,
            Some(unit_id.clone()),
            &path_ids,
            &path_names,
            index as i64,
            level + 1,
            now,
            output,
        )?;
    }
    Ok(())
}

fn build_unit_id(path_names: &[String]) -> String {
    let text = path_names.join("/");
    let uuid = Uuid::new_v5(&Uuid::NAMESPACE_URL, text.as_bytes());
    format!("unit_{}", uuid.simple())
}

fn now_ts() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000.0
}
