use super::{build_model_tool_success, context::ToolContext};
use crate::config::Config;
use crate::i18n;
use crate::path_utils::{normalize_existing_path, normalize_path_for_compare};
use crate::skills::SkillSpec;
use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use serde_yaml::Value as YamlValue;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub(crate) const SKILL_ROOT_PLACEHOLDER: &str = "{{SKILL_ROOT}}";
const DEFAULT_READ_LINE_LIMIT: usize = 160;
const MAX_READ_LINE_LIMIT: usize = 400;
const MAX_READ_BYTES: usize = 64 * 1024;
const MAX_MANIFEST_ITEMS: usize = 64;

pub(crate) fn render_skill_markdown_for_model(raw: &str, skill_root: &str) -> String {
    if raw.contains(SKILL_ROOT_PLACEHOLDER) {
        raw.replace(SKILL_ROOT_PLACEHOLDER, skill_root)
    } else {
        raw.to_string()
    }
}

pub(crate) fn parse_skill_name_candidates(raw_name: &str) -> Vec<String> {
    let trimmed = raw_name.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let mut names = vec![trimmed.to_string()];
    if let Some((_, suffix)) = trimmed.split_once('@') {
        let suffix = suffix.trim();
        if !suffix.is_empty() && !names.iter().any(|item| item == suffix) {
            names.push(suffix.to_string());
        }
    }
    names
}

#[derive(Debug, Deserialize)]
struct SkillCallArgs {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    skill_name: Option<String>,
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    section: Option<String>,
    #[serde(default)]
    start_line: Option<usize>,
    #[serde(default)]
    line_limit: Option<usize>,
    #[serde(default)]
    full: bool,
}

pub(crate) async fn execute_skill_call(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: SkillCallArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let raw_name = payload
        .name
        .as_deref()
        .or(payload.skill_name.as_deref())
        .unwrap_or_default()
        .trim()
        .to_string();
    if raw_name.is_empty() {
        return Err(anyhow!(i18n::t("tool.skill_call.name_required")));
    }

    let mut selected: Option<SkillSpec> = None;
    if let Some(bindings) = context.user_tool_bindings {
        if let Some(spec) = bindings
            .skill_specs
            .iter()
            .find(|spec| spec.name == raw_name)
        {
            selected = Some(spec.clone());
        } else {
            let suffix = format!("@{raw_name}");
            let matches: Vec<SkillSpec> = bindings
                .skill_specs
                .iter()
                .filter(|spec| spec.name.ends_with(&suffix))
                .cloned()
                .collect();
            if matches.len() == 1 {
                selected = Some(matches[0].clone());
            } else if matches.len() > 1 {
                let candidates = matches
                    .iter()
                    .map(|spec| spec.name.clone())
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(anyhow!(i18n::t_with_params(
                    "tool.skill_call.ambiguous",
                    &HashMap::from([
                        ("name".to_string(), raw_name.clone()),
                        ("candidates".to_string(), candidates),
                    ]),
                )));
            }
        }
    }
    if selected.is_none() {
        selected = context.skills.get(&raw_name);
    }
    if selected.is_none() {
        selected = resolve_skill_call_spec_from_roots(context, &raw_name);
    }

    let Some(spec) = selected else {
        return Err(anyhow!(i18n::t_with_params(
            "tool.skill_call.not_found",
            &HashMap::from([("name".to_string(), raw_name)]),
        )));
    };

    let action = if payload.full {
        "full".to_string()
    } else {
        payload
            .action
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("manifest")
            .to_ascii_lowercase()
    };
    match action.as_str() {
        "manifest" => Ok(build_model_tool_success(
            "skill_manifest",
            "completed",
            format!("Skill {} manifest loaded.", spec.name),
            build_skill_manifest(&spec),
        )),
        "read" => read_skill_resource(&spec, &payload),
        // Explicit compatibility path for callers that still request full.
        "full" => {
            let mut read_payload = payload;
            read_payload.path = Some("SKILL.md".to_string());
            read_payload.line_limit = Some(MAX_READ_LINE_LIMIT);
            read_skill_resource(&spec, &read_payload)
        }
        other => Err(anyhow!(format!(
            "unsupported skill action `{other}`; use manifest or read"
        ))),
    }
}

fn build_skill_manifest(spec: &SkillSpec) -> Value {
    let version = parse_frontmatter_value(&spec.frontmatter, "version");
    let entrypoints = ["run.py", "skill.py", "main.py"]
        .iter()
        .filter(|name| spec.root.join(name).is_file())
        .map(|name| (*name).to_string())
        .collect::<Vec<_>>();
    let resource_index = build_skill_tree(&spec.root)
        .into_iter()
        .take(MAX_MANIFEST_ITEMS)
        .collect::<Vec<_>>();
    let mut manifest = json!({
        "name": spec.name,
        "summary": truncate_skill_text(&spec.description, 320),
        "entrypoints": entrypoints,
        "resource_index": resource_index,
        "read": "action=read; path or section selects a bounded resource",
    });
    if let Some(version) = version {
        manifest["version"] = Value::String(version);
    }
    manifest
}

fn read_skill_resource(spec: &SkillSpec, payload: &SkillCallArgs) -> Result<Value> {
    let requested = payload
        .path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .unwrap_or("SKILL.md");
    let path = resolve_skill_resource_path(&spec.root, requested)?;
    let raw = std::fs::read_to_string(&path).map_err(|err| {
        anyhow!(i18n::t_with_params(
            "tool.skill_call.read_failed",
            &HashMap::from([("detail".to_string(), err.to_string())]),
        ))
    })?;
    let selected = if let Some(section) = payload
        .section
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        select_skill_section(&raw, section)
            .ok_or_else(|| anyhow!(format!("skill section `{section}` was not found")))?
    } else {
        raw
    };
    let root = absolute_path_string(&spec.root);
    let rendered = render_skill_markdown_for_model(&selected, &root);
    let start_line = payload.start_line.unwrap_or(1).max(1);
    let line_limit = payload
        .line_limit
        .unwrap_or(DEFAULT_READ_LINE_LIMIT)
        .clamp(1, MAX_READ_LINE_LIMIT);
    let (content, end_line, truncated) = take_skill_lines(&rendered, start_line, line_limit);
    Ok(build_model_tool_success(
        "skill_read",
        "completed",
        format!("Read {} from skill {}.", requested, spec.name),
        json!({
            "name": spec.name,
            "path": relative_skill_path(&spec.root, &path),
            "content": content,
            "start_line": start_line,
            "end_line": end_line,
            "truncated": truncated,
        }),
    ))
}

fn resolve_skill_resource_path(root: &Path, requested: &str) -> Result<PathBuf> {
    let relative = requested.replace('\\', "/");
    let relative = relative.strip_prefix("./").unwrap_or(&relative);
    let relative_path = Path::new(relative);
    if relative.is_empty()
        || relative_path.is_absolute()
        || relative_path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(anyhow!(
            "skill resource path must stay inside the skill root"
        ));
    }
    let candidate = root.join(relative_path);
    let canonical_root = root
        .canonicalize()
        .map_err(|err| anyhow!(format!("skill root unavailable: {err}")))?;
    let canonical = candidate
        .canonicalize()
        .map_err(|err| anyhow!(format!("skill resource not found: {err}")))?;
    if !canonical.starts_with(&canonical_root) {
        return Err(anyhow!(
            "skill resource path must stay inside the skill root"
        ));
    }
    Ok(canonical)
}

fn relative_skill_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn parse_frontmatter_value(frontmatter: &str, key: &str) -> Option<String> {
    let value: YamlValue = serde_yaml::from_str(frontmatter).ok()?;
    value
        .get(key)
        .and_then(YamlValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn truncate_skill_text(value: &str, max_chars: usize) -> String {
    let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if value.chars().count() <= max_chars {
        return value;
    }
    let mut result = value
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    result.push('…');
    result
}

fn select_skill_section(raw: &str, section: &str) -> Option<String> {
    let wanted = section.trim().trim_start_matches('#').trim();
    if wanted.is_empty() {
        return None;
    }
    let lines = raw.lines().collect::<Vec<_>>();
    let start = lines.iter().position(|line| {
        let heading = line.trim().trim_start_matches('#').trim();
        !heading.is_empty() && heading.eq_ignore_ascii_case(wanted)
    })?;
    let level = lines[start].chars().take_while(|ch| *ch == '#').count();
    let end = lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find(|(_, line)| {
            let trimmed = line.trim();
            let next_level = trimmed.chars().take_while(|ch| *ch == '#').count();
            next_level > 0 && next_level <= level
        })
        .map(|(index, _)| index)
        .unwrap_or(lines.len());
    Some(lines[start..end].join("\n"))
}

fn take_skill_lines(raw: &str, start_line: usize, line_limit: usize) -> (String, usize, bool) {
    let lines = raw.lines().collect::<Vec<_>>();
    let start = start_line.saturating_sub(1).min(lines.len());
    let end = (start + line_limit).min(lines.len());
    let mut content = lines[start..end].join("\n");
    let mut truncated = end < lines.len();
    if content.len() > MAX_READ_BYTES {
        let mut bytes = content.into_bytes();
        bytes.truncate(MAX_READ_BYTES);
        content = String::from_utf8_lossy(&bytes).into_owned();
        truncated = true;
    }
    (content, end, truncated)
}

fn resolve_skill_call_spec_from_roots(
    context: &ToolContext<'_>,
    raw_name: &str,
) -> Option<SkillSpec> {
    let requested_owner = raw_name
        .split_once('@')
        .map(|(owner_id, _)| owner_id.trim().to_string())
        .filter(|owner_id| !owner_id.is_empty());
    let mut roots = Vec::new();
    let mut seen_roots: HashSet<PathBuf> = HashSet::new();
    let add_root = |root: PathBuf, roots: &mut Vec<PathBuf>, seen_roots: &mut HashSet<PathBuf>| {
        if !root.exists() || !root.is_dir() {
            return;
        }
        let key = normalize_path_for_compare(&normalize_existing_path(&root));
        if seen_roots.insert(key) {
            roots.push(root);
        }
    };

    if let Some(store) = context.user_tool_store {
        if let Some(owner_id) = requested_owner.as_deref() {
            add_root(store.get_skill_root(owner_id), &mut roots, &mut seen_roots);
        } else {
            add_root(
                store.get_skill_root(context.user_id),
                &mut roots,
                &mut seen_roots,
            );
            if let Some(bindings) = context.user_tool_bindings {
                for owner_id in bindings.skill_sources.keys() {
                    add_root(store.get_skill_root(owner_id), &mut roots, &mut seen_roots);
                }
            }
        }
    }
    if let Some(bindings) = context.user_tool_bindings {
        if let Some(owner_id) = requested_owner.as_deref() {
            if let Some(source) = bindings.skill_sources.get(owner_id) {
                add_root(source.root.clone(), &mut roots, &mut seen_roots);
            }
        } else {
            for source in bindings.skill_sources.values() {
                add_root(source.root.clone(), &mut roots, &mut seen_roots);
            }
        }
    }

    for root in roots {
        if let Some(spec) = find_skill_spec_in_root_by_name(&root, raw_name) {
            return Some(spec);
        }
    }
    None
}

fn find_skill_spec_in_root_by_name(root: &Path, raw_name: &str) -> Option<SkillSpec> {
    let candidates = parse_skill_name_candidates(raw_name);
    if candidates.is_empty() || !root.exists() || !root.is_dir() {
        return None;
    }

    let mut scan_config = Config::default();
    scan_config.skills.paths = vec![root.to_string_lossy().to_string()];
    let registry = crate::skills::load_skills(&scan_config, false, false, false);

    let mut matches = Vec::new();
    for candidate in candidates {
        matches.extend(
            registry
                .list_specs()
                .into_iter()
                .filter(|spec| spec.name == candidate),
        );
    }
    matches.sort_by(|left, right| left.name.cmp(&right.name));
    matches.dedup_by(|left, right| left.path == right.path);
    matches.into_iter().next()
}

fn build_skill_tree(root: &Path) -> Vec<String> {
    let mut items = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return items;
    };
    for entry in entries.filter_map(|entry| entry.ok()) {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.is_empty() {
            continue;
        }
        let mut display = name.replace('\\', "/");
        if entry
            .file_type()
            .map(|file_type| file_type.is_dir())
            .unwrap_or(false)
        {
            display.push('/');
        }
        items.push(display);
    }
    items.sort();
    items
}

fn absolute_path_string(path: &Path) -> String {
    let normalized = normalize_existing_path(path);
    let mut text = normalized.to_string_lossy().to_string();
    if cfg!(windows) {
        if let Some(stripped) = text.strip_prefix(r"\\?\") {
            text = stripped.to_string();
        }
    }
    text.replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::lsp::LspManager;
    use crate::services::tools::ToolContext;
    use crate::skills::SkillRegistry;
    use crate::storage::SqliteStorage;
    use crate::user_tools::UserToolStore;
    use crate::workspace::WorkspaceManager;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tempfile::tempdir;

    #[test]
    fn replaces_skill_root_placeholder() {
        let raw = format!(
            "run: {SKILL_ROOT_PLACEHOLDER}/scripts/tool.py --input {SKILL_ROOT_PLACEHOLDER}/data.json"
        );
        let rendered = render_skill_markdown_for_model(&raw, "C:/tmp/skills/demo");
        assert_eq!(
            rendered,
            "run: C:/tmp/skills/demo/scripts/tool.py --input C:/tmp/skills/demo/data.json"
        );
    }

    #[test]
    fn keeps_content_without_placeholder() {
        let raw = "no placeholder";
        let rendered = render_skill_markdown_for_model(raw, "C:/tmp/skills/demo");
        assert_eq!(rendered, raw);
    }

    #[test]
    fn parses_skill_name_candidates_with_owner_alias() {
        assert_eq!(
            parse_skill_name_candidates("alice@planner"),
            vec!["alice@planner".to_string(), "planner".to_string()]
        );
    }

    #[test]
    fn parses_skill_name_candidates_with_plain_name() {
        assert_eq!(
            parse_skill_name_candidates("planner"),
            vec!["planner".to_string()]
        );
    }

    #[test]
    fn build_skill_tree_only_lists_root_children() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::write(root.join("SKILL.md"), "# Demo").expect("write skill");
        std::fs::create_dir_all(root.join("references").join("nested")).expect("create references");
        std::fs::write(root.join("references").join("guide.md"), "# Guide").expect("write guide");
        std::fs::write(
            root.join("references").join("nested").join("deep.md"),
            "# Deep",
        )
        .expect("write nested guide");
        std::fs::create_dir_all(root.join("scripts")).expect("create scripts");
        std::fs::write(root.join("scripts").join("tool.py"), "print('ok')").expect("write script");

        assert_eq!(
            build_skill_tree(root),
            vec![
                "SKILL.md".to_string(),
                "references/".to_string(),
                "scripts/".to_string()
            ]
        );
    }

    #[tokio::test]
    async fn skill_call_loads_fallback_user_skill_from_store_root() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("state.sqlite3");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        let workspace_root = dir.path().join("workspace");
        let workspace = Arc::new(WorkspaceManager::new(
            workspace_root.to_string_lossy().as_ref(),
            storage.clone(),
            0,
            &HashMap::new(),
        ));
        let config = Config::default();
        let store = UserToolStore::new(&config, workspace.clone()).expect("user store");
        let skill_root = store.get_skill_root("alice").join("draft-skill");
        std::fs::create_dir_all(&skill_root).expect("create skill dir");
        std::fs::write(
            skill_root.join("SKILL.md"),
            "---\nname: draft_skill\ndescription: draft description\n---\n# Draft\nUse {{SKILL_ROOT}}\n",
        )
        .expect("write skill");
        let skills = SkillRegistry::default();
        let http = reqwest::Client::new();
        let lsp_manager = LspManager::new(workspace.clone());
        let context = ToolContext {
            user_id: "alice",
            session_id: "sess_skill",
            workspace_id: "alice",
            agent_id: None,
            user_round: None,
            model_round: None,
            is_admin: false,
            storage,
            orchestrator: None,
            monitor: None,
            workspace,
            lsp_manager,
            config: &config,
            skills: &skills,
            gateway: None,
            user_world: None,
            cron_wake_signal: None,
            user_tool_manager: None,
            user_tool_bindings: None,
            user_tool_store: Some(&store),
            request_config_overrides: None,
            allow_roots: None,
            read_roots: None,
            command_sessions: None,
            event_emitter: None,
            http: &http,
        };

        let result = execute_skill_call(&context, &json!({ "name": "draft_skill" }))
            .await
            .expect("skill call should succeed");

        assert_eq!(result["ok"], true);
        assert_eq!(result["action"], "skill_manifest");
        assert_eq!(result["data"]["name"], "draft_skill");
        assert_eq!(result["data"]["summary"], "draft description");
        assert_eq!(result["data"]["resource_index"], json!(["SKILL.md"]));
        assert!(result["data"].get("content").is_none());
    }

    #[test]
    fn fallback_skill_lookup_honors_explicit_owner_alias() {
        let dir = tempdir().expect("tempdir");
        let alice_root = dir.path().join("alice-skills");
        let bob_root = dir.path().join("bob-skills");
        let alice_skill = alice_root.join("planner");
        let bob_skill = bob_root.join("planner");
        std::fs::create_dir_all(&alice_skill).expect("create alice skill dir");
        std::fs::create_dir_all(&bob_skill).expect("create bob skill dir");
        std::fs::write(
            alice_skill.join("SKILL.md"),
            "---\nname: planner\ndescription: alice planner\n---\n# Alice\n",
        )
        .expect("write alice skill");
        std::fs::write(
            bob_skill.join("SKILL.md"),
            "---\nname: bob@planner\ndescription: bob planner\n---\n# Bob\n",
        )
        .expect("write bob skill");

        let selected = find_skill_spec_in_root_by_name(&bob_root, "bob@planner")
            .expect("bob owner skill should be selected");
        assert_eq!(selected.name, "bob@planner");

        let selected_plain = find_skill_spec_in_root_by_name(&alice_root, "planner")
            .expect("plain skill should be selected from requested root");
        assert_eq!(selected_plain.description, "alice planner");
    }
}
