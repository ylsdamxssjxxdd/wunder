use super::{build_model_tool_success, context::ToolContext};
use crate::i18n;
use crate::path_utils::{normalize_existing_path, normalize_path_for_compare};
use crate::skills::SkillSpec;
use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use serde_yaml::Value as YamlValue;
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub(crate) const SKILL_ROOT_PLACEHOLDER: &str = "{{SKILL_ROOT}}";
const DEFAULT_READ_LINE_LIMIT: usize = 160;
const MAX_READ_LINE_LIMIT: usize = 800;
const MAX_READ_BYTES: usize = 64 * 1024;
/// First-page budget for `load`: wide enough that a normal SKILL.md is fully
/// delivered in the same call as its resource index, so the model never pays a
/// separate manifest round-trip before it can start the workflow.
const DEFAULT_LOAD_LINE_LIMIT: usize = MAX_READ_LINE_LIMIT;
const DEFAULT_LOAD_BYTES: usize = 48 * 1024;
const MAX_MANIFEST_ITEMS: usize = 64;
const DEFAULT_READ_BYTES: usize = 16 * 1024;
const MAX_SECTION_SCAN_BYTES: usize = 256 * 1024;
const MAX_MANIFEST_SCAN_ITEMS: usize = 4096;
const MAX_MANIFEST_CHARS: usize = 1600;
const MAX_FRAGMENT_BYTES: usize = 4096;
const MAX_CURSOR_BYTES: usize = 4096;

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
    bytes_limit: Option<usize>,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    manifest_cursor: Option<String>,
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
            .unwrap_or("load")
            .to_ascii_lowercase()
    };
    match action.as_str() {
        // Default path: SKILL.md body and the resource index in a single call.
        "load" => load_skill(&spec, payload),
        "manifest" => Ok(build_model_tool_success(
            "skill_manifest",
            "completed",
            format!("Skill {} manifest loaded.", spec.name),
            build_skill_manifest(&spec, payload.manifest_cursor.as_deref())?,
        )),
        "read" => read_skill_resource(&spec, &payload),
        // Explicit compatibility path for callers that still request full.
        "full" => {
            let mut read_payload = payload;
            read_payload.path = Some("SKILL.md".to_string());
            read_payload.line_limit = Some(MAX_READ_LINE_LIMIT);
            read_payload.full = false;
            read_skill_resource(&spec, &read_payload)
        }
        other => Err(anyhow!(format!(
            "unsupported skill action `{other}`; use load, read or manifest"
        ))),
    }
}

/// One-shot skill load: bounded `SKILL.md` window plus the compact resource
/// index, so a single tool call is enough to start the workflow.
fn load_skill(spec: &SkillSpec, mut payload: SkillCallArgs) -> Result<Value> {
    payload.full = false;
    if payload
        .path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none()
    {
        payload.path = Some("SKILL.md".to_string());
    }
    payload.line_limit.get_or_insert(DEFAULT_LOAD_LINE_LIMIT);
    payload.bytes_limit.get_or_insert(DEFAULT_LOAD_BYTES);
    let mut result = read_skill_resource(spec, &payload)?;
    merge_skill_index(&mut result, &spec.name, &build_skill_manifest(spec, None)?);
    if let Some(object) = result.as_object_mut() {
        object.insert("action".to_string(), json!("skill_load"));
        object.insert(
            "summary".to_string(),
            json!(format!("Skill {} loaded.", spec.name)),
        );
    }
    Ok(result)
}

/// Fold the manifest's discovery fields into an already-read resource payload.
/// Index paging keeps a separate cursor key so it cannot be mistaken for the
/// content cursor of the resource window.
fn merge_skill_index(result: &mut Value, spec_name: &str, manifest: &Value) {
    let Some(data) = result.get_mut("data").and_then(Value::as_object_mut) else {
        return;
    };
    data.insert("name".to_string(), json!(spec_name));
    for key in ["summary", "version", "entrypoints", "resource_index"] {
        if let Some(value) = manifest.get(key) {
            data.insert(key.to_string(), value.clone());
        }
    }
    if manifest.get("index_scan_limited") == Some(&Value::Bool(true)) {
        data.insert("index_scan_limited".to_string(), Value::Bool(true));
    }
    if let Some(cursor) = manifest.get("next_cursor").and_then(Value::as_str) {
        data.insert("index_next_cursor".to_string(), json!(cursor));
    }
}

fn build_skill_manifest(spec: &SkillSpec, cursor: Option<&str>) -> Result<Value> {
    let version = parse_frontmatter_value(&spec.frontmatter, "version");
    let entrypoints = ["run.py", "skill.py", "main.py"]
        .iter()
        .filter(|name| spec.root.join(name).is_file())
        .map(|name| (*name).to_string())
        .collect::<Vec<_>>();
    let decoded_cursor = match cursor {
        Some(value) if value.len() <= 2048 => String::from_utf8(
            URL_SAFE_NO_PAD
                .decode(value)
                .map_err(|_| anyhow!("invalid manifest cursor"))?,
        )
        .map_err(|_| anyhow!("invalid manifest cursor"))
        .map(Some)?,
        Some(_) => return Err(anyhow!("manifest cursor is too long")),
        None => None,
    };
    let (mut items, more, scan_limited) =
        build_skill_tree_page(&spec.root, decoded_cursor.as_deref());
    let mut manifest = json!({
        "name": truncate_skill_text(&spec.name, 96),
        "summary": truncate_skill_text(&spec.description, 200),
        "entrypoints": entrypoints,
        "resource_index": [],
        "index_scan_limited": scan_limited,
        "read": "action=read; path/section, then cursor",
    });
    if let Some(version) = version {
        manifest["version"] = Value::String(truncate_skill_text(&version, 32));
    }
    let mut shortened = more;
    loop {
        manifest["resource_index"] = json!(items);
        if shortened {
            if let Some(last) = items.last() {
                manifest["next_cursor"] = Value::String(URL_SAFE_NO_PAD.encode(last.as_bytes()));
            }
        } else if !scan_limited {
            if let Some(object) = manifest.as_object_mut() {
                object.remove("next_cursor");
            }
        }
        let length = serde_json::to_string(&manifest)
            .map(|text| text.chars().count())
            .unwrap_or(0);
        if length <= MAX_MANIFEST_CHARS || items.is_empty() {
            break;
        }
        items.pop();
        shortened = true;
    }
    while serde_json::to_string(&manifest)
        .map(|text| text.chars().count())
        .unwrap_or(0)
        > MAX_MANIFEST_CHARS
    {
        if let Some(summary) = manifest["summary"].as_str() {
            let next = truncate_skill_text(summary, summary.chars().count().saturating_sub(32));
            if next != summary {
                manifest["summary"] = Value::String(next);
                continue;
            }
        }
        if let Some(name) = manifest["name"].as_str() {
            let next = truncate_skill_text(name, name.chars().count().saturating_sub(16));
            if next != name {
                manifest["name"] = Value::String(next);
                continue;
            }
        }
        if manifest
            .as_object_mut()
            .is_some_and(|value| value.remove("version").is_some())
        {
            continue;
        }
        break;
    }
    Ok(manifest)
}

#[derive(Debug, Serialize, Deserialize)]
struct SkillReadCursor {
    path: String,
    binding: String,
    offset: u64,
    line: usize,
    line_start: bool,
    skip_lines: usize,
    section: Option<String>,
    found: bool,
    level: usize,
}

fn read_skill_resource(spec: &SkillSpec, payload: &SkillCallArgs) -> Result<Value> {
    if payload.cursor.is_some()
        && (payload.start_line.is_some() || payload.section.is_some() || payload.full)
    {
        return Err(anyhow!(
            "cursor cannot be combined with start_line, section or full"
        ));
    }
    let mut state = if let Some(encoded) = payload.cursor.as_deref() {
        if encoded.len() > MAX_CURSOR_BYTES {
            return Err(anyhow!("skill read cursor is too long"));
        }
        let bytes = URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| anyhow!("invalid skill read cursor"))?;
        let state: SkillReadCursor =
            serde_json::from_slice(&bytes).map_err(|_| anyhow!("invalid skill read cursor"))?;
        if state.path.len() > 1024
            || state
                .section
                .as_ref()
                .is_some_and(|value| value.len() > 512)
            || state.level > 6
            || state.line == 0
            || state.binding.len() != 64
        {
            return Err(anyhow!("invalid skill read cursor"));
        }
        state
    } else {
        let section = payload
            .section
            .as_deref()
            .map(str::trim)
            .map(|value| value.trim_start_matches('#').trim())
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        if section.as_ref().is_some_and(|value| value.len() > 512) {
            return Err(anyhow!("section must not exceed 512 UTF-8 bytes"));
        }
        SkillReadCursor {
            path: payload
                .path
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("SKILL.md")
                .to_string(),
            binding: String::new(),
            offset: 0,
            line: 1,
            line_start: true,
            skip_lines: payload.start_line.unwrap_or(1).max(1).saturating_sub(1),
            found: section.is_none(),
            section,
            level: 0,
        }
    };
    if state.path.len() > 1024 {
        return Err(anyhow!("skill resource path is too long"));
    }
    if payload.cursor.is_some()
        && payload
            .path
            .as_ref()
            .is_some_and(|path| path.trim() != state.path)
    {
        return Err(anyhow!(
            "cursor belongs to a different resource; reuse its path"
        ));
    }
    let path = resolve_skill_resource_path(&spec.root, &state.path)?;
    let file = File::open(&path).map_err(|_| anyhow!("skill resource could not be opened"))?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(anyhow!("skill resource must be a regular file"));
    }
    let mut hasher = Sha256::new();
    hasher.update(path.as_os_str().to_string_lossy().as_bytes());
    hasher.update(metadata.len().to_le_bytes());
    if let Some(modified) = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
    {
        hasher.update(modified.as_nanos().to_le_bytes());
    }
    let binding = format!("{:x}", hasher.finalize());
    if !state.binding.is_empty() && state.binding != binding {
        return Err(anyhow!(
            "skill resource changed; restart the read without cursor"
        ));
    }
    state.binding = binding;
    if state.offset > metadata.len() {
        return Err(anyhow!("skill read cursor is past the resource end"));
    }
    let bytes_limit = payload
        .bytes_limit
        .unwrap_or(DEFAULT_READ_BYTES)
        .clamp(4, MAX_READ_BYTES);
    let line_limit = payload
        .line_limit
        .unwrap_or(DEFAULT_READ_LINE_LIMIT)
        .clamp(1, MAX_READ_LINE_LIMIT);
    let mut reader = BufReader::with_capacity(8192, file);
    reader.seek(SeekFrom::Start(state.offset))?;
    let root = ".";
    let mut content = String::with_capacity(bytes_limit.min(8192));
    let mut scanned = 0usize;
    let mut returned_lines = 0usize;
    let mut first_line = None;
    let mut section_ended = false;
    while state.offset < metadata.len() && scanned < MAX_SECTION_SCAN_BYTES {
        if content.len() >= bytes_limit || returned_lines >= line_limit {
            break;
        }
        let cap = MAX_FRAGMENT_BYTES.min(MAX_SECTION_SCAN_BYTES - scanned);
        let raw = read_skill_fragment(&mut reader, cap.max(4))?;
        if raw.is_empty() {
            break;
        }
        scanned = scanned.saturating_add(raw.len());
        let ends_line = raw.ends_with('\n');
        if state.line_start {
            if let Some((level, title)) = markdown_heading(&raw) {
                if !state.found
                    && state
                        .section
                        .as_ref()
                        .is_some_and(|wanted| title.eq_ignore_ascii_case(wanted))
                {
                    state.found = true;
                    state.level = level;
                    state.line = 1;
                } else if state.found && state.section.is_some() && level <= state.level {
                    section_ended = true;
                    break;
                }
            }
        }
        if !state.found || state.skip_lines > 0 {
            state.offset = state.offset.saturating_add(raw.len() as u64);
            state.line_start = ends_line;
            if ends_line && state.found {
                state.skip_lines = state.skip_lines.saturating_sub(1);
                state.line = state.line.saturating_add(1);
            }
            continue;
        }
        let (rendered, consumed) = render_skill_fragment(
            &raw,
            &root,
            bytes_limit - content.len(),
            state.offset.saturating_add(raw.len() as u64) < metadata.len() && !ends_line,
        );
        if consumed == 0 {
            if content.is_empty() {
                return Err(anyhow!(
                    "bytes_limit is too small for a skill-root expansion; increase bytes_limit"
                ));
            }
            break;
        }
        first_line.get_or_insert(state.line);
        content.push_str(&rendered);
        state.offset = state.offset.saturating_add(consumed as u64);
        if consumed < raw.len() {
            reader.seek_relative(-((raw.len() - consumed) as i64))?;
            state.line_start = false;
            break;
        }
        state.line_start = ends_line;
        if ends_line {
            returned_lines += 1;
            state.line = state.line.saturating_add(1);
        }
    }
    let pending = !section_ended && state.offset < metadata.len();
    if !state.found && !pending {
        return Err(anyhow!("requested skill section was not found"));
    }
    let section_scan_pending = !state.found && pending;
    let next_cursor = if pending {
        Some(URL_SAFE_NO_PAD.encode(serde_json::to_vec(&state)?))
    } else {
        None
    };
    let start_line = first_line.unwrap_or(state.line);
    let end_line = if content.ends_with('\n') {
        state.line.saturating_sub(1)
    } else {
        state.line
    };
    Ok(build_model_tool_success(
        "skill_read",
        "completed",
        "Skill resource window loaded.",
        json!({
            "name": truncate_skill_text(&spec.name, 96),
            "path": state.path,
            "content": content,
            "start_line": start_line,
            "end_line": end_line,
            "truncated": pending,
            "section_scan_pending": section_scan_pending,
            "next_cursor": next_cursor,
        }),
    ))
}

/// Read one bounded physical-line fragment, without read_line's unbounded allocation.
fn read_skill_fragment(reader: &mut BufReader<File>, cap: usize) -> Result<String> {
    let mut bytes = Vec::with_capacity(cap.min(MAX_FRAGMENT_BYTES));
    while bytes.len() < cap {
        let buffer = reader.fill_buf()?;
        if buffer.is_empty() {
            break;
        }
        let remaining = cap - bytes.len();
        let length = buffer
            .iter()
            .take(remaining)
            .position(|byte| *byte == b'\n')
            .map(|index| index + 1)
            .unwrap_or_else(|| remaining.min(buffer.len()));
        bytes.extend_from_slice(&buffer[..length]);
        reader.consume(length);
        if bytes.last() == Some(&b'\n') {
            break;
        }
    }
    match std::str::from_utf8(&bytes) {
        Ok(_) => {}
        Err(error) if error.error_len().is_none() && error.valid_up_to() > 0 => {
            let tail = bytes.len() - error.valid_up_to();
            reader.seek_relative(-(tail as i64))?;
            bytes.truncate(error.valid_up_to());
        }
        Err(_) => return Err(anyhow!("skill resource is not valid UTF-8")),
    }
    String::from_utf8(bytes).map_err(|_| anyhow!("skill resource is not valid UTF-8"))
}

fn markdown_heading(raw: &str) -> Option<(usize, &str)> {
    let line = raw.trim_start_matches(' ');
    if raw.len() - line.len() > 3 {
        return None;
    }
    let level = line.bytes().take_while(|byte| *byte == b'#').count();
    if level == 0 || level > 6 {
        return None;
    }
    let title = &line[level..];
    if !title.chars().next().is_some_and(|ch| ch.is_whitespace()) {
        return None;
    }
    Some((level, title.trim().trim_end_matches('#').trim_end()))
}

/// Return source bytes consumed as well as rendered bytes, so pagination cannot skip text.
fn render_skill_fragment(raw: &str, root: &str, limit: usize, has_more: bool) -> (String, usize) {
    let mut result = String::with_capacity(limit.min(raw.len()));
    let mut consumed = 0;
    while consumed < raw.len() {
        let tail = &raw[consumed..];
        if has_more
            && tail.len() < SKILL_ROOT_PLACEHOLDER.len()
            && SKILL_ROOT_PLACEHOLDER.starts_with(tail)
        {
            break;
        }
        if tail.starts_with(SKILL_ROOT_PLACEHOLDER) {
            if result.len().saturating_add(root.len()) > limit {
                break;
            }
            result.push_str(root);
            consumed += SKILL_ROOT_PLACEHOLDER.len();
        } else if let Some(ch) = tail.chars().next() {
            if result.len().saturating_add(ch.len_utf8()) > limit {
                break;
            }
            result.push(ch);
            consumed += ch.len_utf8();
        } else {
            break;
        }
    }
    (result, consumed)
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

    crate::skills::find_skill_in_root_by_candidates(root, &candidates)
}

fn build_skill_tree_page(root: &Path, cursor: Option<&str>) -> (Vec<String>, bool, bool) {
    let mut items = BTreeSet::new();
    let mut scanned = 0;
    let Ok(entries) = std::fs::read_dir(root) else {
        return (Vec::new(), false, false);
    };
    let mut scan_limited = false;
    for entry in entries {
        if scanned >= MAX_MANIFEST_SCAN_ITEMS {
            scan_limited = true;
            break;
        }
        scanned += 1;
        let Ok(entry) = entry else {
            continue;
        };
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
        if cursor.is_some_and(|value| display.as_str() <= value) {
            continue;
        }
        items.insert(display);
    }
    let mut page = items
        .into_iter()
        .take(MAX_MANIFEST_ITEMS + 1)
        .collect::<Vec<_>>();
    let more = page.len() > MAX_MANIFEST_ITEMS;
    if more {
        page.pop();
    }
    // A hard scan cap means this page is only a bounded discovery view.  The
    // explicit flag prevents callers from treating a final page as a complete
    // directory listing.
    (page, more, scan_limited)
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
            build_skill_tree_page(root, None).0,
            vec![
                "SKILL.md".to_string(),
                "references/".to_string(),
                "scripts/".to_string()
            ]
        );
    }

    #[test]
    fn manifest_is_paged_and_bounded_for_large_resource_tree() {
        let dir = tempdir().expect("tempdir");
        for index in 0..100 {
            std::fs::write(
                dir.path()
                    .join(format!("resource-{index:03}-{}.md", "x".repeat(120))),
                "ok",
            )
            .expect("write resource");
        }
        let spec = SkillSpec {
            name: "demo".to_string(),
            description: "d".repeat(500),
            path: String::new(),
            input_schema: json!({}),
            frontmatter: String::new(),
            root: dir.path().to_path_buf(),
            entrypoint: None,
        };
        let first = build_skill_manifest(&spec, None).expect("manifest");
        assert!(serde_json::to_string(&first).expect("json").chars().count() <= MAX_MANIFEST_CHARS);
        assert!(first["resource_index"].as_array().expect("index").len() < MAX_MANIFEST_ITEMS);
        let cursor = first["next_cursor"].as_str().expect("next cursor");
        let second = build_skill_manifest(&spec, Some(cursor)).expect("second page");
        assert_ne!(first["resource_index"], second["resource_index"]);
    }

    #[test]
    fn bounded_fragment_preserves_utf8_and_rewinds_incomplete_codepoint() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("resource.md");
        let expected = format!("{}中tail", "a".repeat(MAX_FRAGMENT_BYTES - 1));
        std::fs::write(&path, &expected).expect("write resource");
        let mut reader = BufReader::with_capacity(64, File::open(path).expect("open resource"));
        let first = read_skill_fragment(&mut reader, MAX_FRAGMENT_BYTES).expect("first window");
        let second = read_skill_fragment(&mut reader, MAX_FRAGMENT_BYTES).expect("second window");
        assert!(first.is_char_boundary(first.len()));
        assert_eq!(format!("{first}{second}"), expected);
    }

    #[test]
    fn cursor_rendering_uses_relative_skill_root() {
        let (rendered, consumed) =
            render_skill_fragment("run {{SKILL_ROOT}}/tool.py", ".", MAX_READ_BYTES, false);
        assert_eq!(rendered, "run ./tool.py");
        assert_eq!(consumed, "run {{SKILL_ROOT}}/tool.py".len());
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
        assert_eq!(result["action"], "skill_load");
        assert_eq!(result["data"]["name"], "draft_skill");
        assert_eq!(result["data"]["summary"], "draft description");
        assert_eq!(result["data"]["resource_index"], json!(["SKILL.md"]));
        // The default call must already carry the SKILL.md body.
        assert!(result["data"]["content"]
            .as_str()
            .is_some_and(|content| content.contains("Use .")));
        assert_eq!(result["data"]["truncated"], false);
    }

    #[test]
    fn load_skill_returns_body_and_index_without_extra_call() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::write(
            root.join("SKILL.md"),
            "---\nname: demo\ndescription: demo skill\nversion: 1.2.0\n---\n# Demo\nRun {{SKILL_ROOT}}/scripts/run.py\n",
        )
        .expect("write skill");
        std::fs::create_dir_all(root.join("scripts")).expect("create scripts");
        let spec = SkillSpec {
            name: "demo".to_string(),
            description: "demo skill".to_string(),
            path: root.join("SKILL.md").to_string_lossy().to_string(),
            input_schema: json!({}),
            frontmatter: "name: demo\ndescription: demo skill\nversion: 1.2.0\n".to_string(),
            root: root.to_path_buf(),
            entrypoint: None,
        };

        let result = load_skill(&spec, SkillCallArgs {
            name: Some("demo".to_string()),
            skill_name: None,
            action: None,
            path: None,
            section: None,
            start_line: None,
            line_limit: None,
            bytes_limit: None,
            cursor: None,
            manifest_cursor: None,
            full: false,
        })
        .expect("load skill");

        assert_eq!(result["action"], "skill_load");
        assert_eq!(result["data"]["path"], "SKILL.md");
        assert_eq!(result["data"]["version"], "1.2.0");
        assert_eq!(result["data"]["resource_index"], json!(["SKILL.md", "scripts/"]));
        let content = result["data"]["content"].as_str().expect("content");
        assert!(content.contains("# Demo"));
        assert!(content.contains("./scripts/run.py"));
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
