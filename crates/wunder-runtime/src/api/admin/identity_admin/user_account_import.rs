use crate::api::admin::{
    build_unit_map, ensure_unit_scope, error_response, normalize_optional_id, normalize_user_email,
    normalize_user_roles, normalize_user_status, resolve_admin_actor,
};
use crate::core::blocking;
use crate::i18n;
use crate::state::AppState;
use crate::storage::OrgUnitRecord;
use crate::user_store::UserStore;
use anyhow::{anyhow, Result as AnyhowResult};
use axum::extract::{Multipart, State};
use axum::http::{HeaderMap as AxumHeaderMap, StatusCode};
use axum::response::Response;
use bytes::Bytes;
use calamine::{open_workbook_auto, Data, DataType, Reader};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use uuid::Uuid;

pub(super) const MAX_USER_ACCOUNT_IMPORT_BYTES: usize = 8 * 1024 * 1024;
const MAX_IMPORT_ROWS: usize = 1000;

#[derive(Debug, Clone)]
struct ImportUserRow {
    row_number: usize,
    username: String,
    password: String,
    email: Option<String>,
    unit_id: Option<String>,
    roles: Vec<String>,
    status: String,
}

#[derive(Debug)]
struct UploadedExcel {
    filename: String,
    bytes: Bytes,
}

pub(super) async fn import_user_accounts(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
    multipart: Multipart,
) -> std::result::Result<Value, Response> {
    let units = state
        .user_store
        .list_org_units()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let actor = resolve_admin_actor(&state, &headers, true, &units)?;
    let upload = parse_upload(multipart).await?;
    let temp_path = persist_upload(&upload).await?;
    let parse_path = temp_path.clone();
    let rows = blocking::run_fs("admin.user_accounts.import.parse_excel", move || {
        parse_excel_rows(&parse_path)
    })
    .await
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()));
    let _ = fs::remove_file(&temp_path).await;
    let rows = rows?;
    if rows.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "excel has no importable user rows".to_string(),
        ));
    }

    let unit_map = build_unit_map(&units);
    let unit_lookup = build_unit_lookup(&units);
    let mut created = Vec::new();
    let mut failed = Vec::new();
    let mut seen_usernames = HashSet::new();
    let mut seen_emails = HashSet::new();

    for mut row in rows {
        if row.username.trim().is_empty() || row.password.trim().is_empty() {
            failed.push(import_failure(
                row.row_number,
                row.username,
                "username and password are required",
            ));
            continue;
        }
        let Some(user_id) = UserStore::normalize_user_id(&row.username) else {
            failed.push(import_failure(
                row.row_number,
                row.username,
                "invalid username",
            ));
            continue;
        };
        row.username = user_id;
        if !seen_usernames.insert(row.username.clone()) {
            failed.push(import_failure(
                row.row_number,
                row.username,
                "duplicate username in excel",
            ));
            continue;
        }
        if let Some(email) = row.email.as_ref() {
            if !seen_emails.insert(email.to_ascii_lowercase()) {
                failed.push(import_failure(
                    row.row_number,
                    row.username,
                    "duplicate email in excel",
                ));
                continue;
            }
        }

        let resolved_unit_id = match resolve_import_unit_id(row.unit_id.as_deref(), &unit_lookup) {
            Ok(unit_id) => unit_id,
            Err(err) => {
                failed.push(import_failure(
                    row.row_number,
                    row.username,
                    &err.to_string(),
                ));
                continue;
            }
        };
        if actor.scope_unit_ids.is_some() {
            let Some(unit_id) = resolved_unit_id.as_deref() else {
                failed.push(import_failure(
                    row.row_number,
                    row.username,
                    &i18n::t("error.org_unit_required"),
                ));
                continue;
            };
            if let Err(response) = ensure_unit_scope(&actor, Some(unit_id)) {
                drop(response);
                failed.push(import_failure(
                    row.row_number,
                    row.username,
                    &i18n::t("error.permission_denied"),
                ));
                continue;
            }
        }

        let resolved_unit = resolved_unit_id
            .as_ref()
            .and_then(|unit_id| unit_map.get(unit_id));
        match state.user_store.create_user(
            &row.username,
            row.email.clone(),
            &row.password,
            None,
            resolved_unit_id,
            row.roles.clone(),
            &row.status,
            false,
        ) {
            Ok(record) => {
                if let Err(err) = crate::services::user_agent_presets::ensure_user_agent_bootstrap(
                    &state, &record,
                )
                .await
                {
                    tracing::warn!(
                        "failed to bootstrap user agents after admin batch user create for {}: {err}",
                        record.user_id
                    );
                }
                created.push(json!({
                    "row": row.row_number,
                    "user": UserStore::to_profile_with_unit(&record, resolved_unit),
                }));
            }
            Err(err) => failed.push(import_failure(
                row.row_number,
                row.username,
                &err.to_string(),
            )),
        }
    }

    Ok(json!({
        "data": {
            "filename": upload.filename,
            "total_rows": created.len() + failed.len(),
            "created": created.len(),
            "failed": failed.len(),
            "skipped": failed.len(),
            "items": created,
            "errors": failed,
        }
    }))
}

async fn parse_upload(mut multipart: Multipart) -> std::result::Result<UploadedExcel, Response> {
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    {
        let name = field.name().unwrap_or_default().to_string();
        if name != "file" && name != "excel" && name != "upload" {
            continue;
        }
        let filename = field
            .file_name()
            .map(str::to_string)
            .unwrap_or_else(|| "users.xlsx".to_string());
        if !is_excel_filename(&filename) {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                "only .xlsx, .xls, .xlsm, .xlsb, and .ods files are supported".to_string(),
            ));
        }
        let bytes = field
            .bytes()
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        if bytes.is_empty() {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                "uploaded file is empty".to_string(),
            ));
        }
        return Ok(UploadedExcel { filename, bytes });
    }
    Err(error_response(
        StatusCode::BAD_REQUEST,
        "please upload an excel file".to_string(),
    ))
}

async fn persist_upload(upload: &UploadedExcel) -> std::result::Result<PathBuf, Response> {
    let mut path = std::env::temp_dir();
    let extension = Path::new(&upload.filename)
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_else(|| "xlsx".to_string());
    path.push(format!(
        "wunder-user-import-{}.{}",
        Uuid::new_v4().simple(),
        extension
    ));
    fs::write(&path, &upload.bytes)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(path)
}

fn is_excel_filename(filename: &str) -> bool {
    let extension = Path::new(filename)
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    matches!(extension.as_str(), "xlsx" | "xls" | "xlsm" | "xlsb" | "ods")
}

fn parse_excel_rows(path: &Path) -> AnyhowResult<Vec<ImportUserRow>> {
    let mut workbook = open_workbook_auto(path)?;
    let sheet_name = workbook
        .sheet_names()
        .first()
        .cloned()
        .ok_or_else(|| anyhow!("excel workbook has no sheet"))?;
    let range = workbook.worksheet_range(&sheet_name)?;
    let mut rows = range.rows();
    let header_row = rows
        .next()
        .ok_or_else(|| anyhow!("excel is missing header row"))?;
    let headers = build_header_map(header_row);
    let username_index = required_header(
        &headers,
        &[
            "username",
            "user_name",
            "\u{7528}\u{6237}\u{540d}",
            "\u{8d26}\u{53f7}",
        ],
    )?;
    let password_index = required_header(&headers, &["password", "\u{5bc6}\u{7801}"])?;
    let email_index = optional_header(
        &headers,
        &["email", "mail", "\u{90ae}\u{7bb1}", "\u{90ae}\u{4ef6}"],
    );
    let unit_index = optional_header(
        &headers,
        &[
            "unit_id",
            "unit",
            "org_unit",
            "\u{5355}\u{4f4d}id",
            "\u{5355}\u{4f4d}",
        ],
    );
    let status_index = optional_header(&headers, &["status", "\u{72b6}\u{6001}"]);
    let roles_index = optional_header(
        &headers,
        &["roles", "role", "\u{89d2}\u{8272}", "\u{6743}\u{9650}"],
    );

    let mut output = Vec::new();
    for (row_offset, row) in rows.enumerate() {
        if output.len() >= MAX_IMPORT_ROWS {
            return Err(anyhow!(
                "single import supports at most {MAX_IMPORT_ROWS} rows"
            ));
        }
        if row.iter().all(|cell| cell_to_string(cell).is_empty()) {
            continue;
        }
        let row_number = row_offset + 2;
        let username = cell_at(row, username_index);
        let password = cell_at(row, password_index);
        if username.is_empty() || password.is_empty() {
            output.push(ImportUserRow {
                row_number,
                username,
                password,
                email: None,
                unit_id: None,
                roles: vec!["user".to_string()],
                status: "active".to_string(),
            });
            continue;
        }
        let email = email_index
            .map(|index| cell_at(row, index))
            .and_then(|value| normalize_user_email(Some(value)));
        let unit_id =
            unit_index.and_then(|index| normalize_optional_id(Some(&cell_at(row, index))));
        let roles = roles_index
            .map(|index| split_roles(&cell_at(row, index)))
            .unwrap_or_else(|| vec!["user".to_string()]);
        let status = status_index
            .map(|index| normalize_user_status(Some(&cell_at(row, index))))
            .unwrap_or_else(|| "active".to_string());
        output.push(ImportUserRow {
            row_number,
            username,
            password,
            email,
            unit_id,
            roles,
            status,
        });
    }
    Ok(output)
}

fn build_header_map(row: &[Data]) -> HashMap<String, usize> {
    row.iter()
        .enumerate()
        .filter_map(|(index, cell)| {
            let key = normalize_header(&cell_to_string(cell));
            if key.is_empty() {
                None
            } else {
                Some((key, index))
            }
        })
        .collect()
}

fn required_header(headers: &HashMap<String, usize>, names: &[&str]) -> AnyhowResult<usize> {
    optional_header(headers, names)
        .ok_or_else(|| anyhow!("excel is missing required header: {}", names.join("/")))
}

fn optional_header(headers: &HashMap<String, usize>, names: &[&str]) -> Option<usize> {
    names
        .iter()
        .map(|name| normalize_header(name))
        .find_map(|name| headers.get(&name).copied())
}

fn normalize_header(value: &str) -> String {
    value
        .trim()
        .trim_start_matches('\u{feff}')
        .to_ascii_lowercase()
        .replace([' ', '-', '/', '\\'], "_")
}

fn cell_at(row: &[Data], index: usize) -> String {
    row.get(index).map(cell_to_string).unwrap_or_default()
}

fn cell_to_string(cell: &Data) -> String {
    match cell {
        Data::Empty => String::new(),
        Data::String(value) => value.trim().to_string(),
        Data::Float(value) if value.fract() == 0.0 => format!("{value:.0}"),
        Data::Float(value) => value.to_string(),
        Data::Int(value) => value.to_string(),
        Data::Bool(value) => value.to_string(),
        Data::Error(value) => value.to_string(),
        _ => cell.as_string().unwrap_or_default().trim().to_string(),
    }
}

fn split_roles(raw: &str) -> Vec<String> {
    normalize_user_roles(
        raw.split([',', ';', '\u{ff0c}', '\u{ff1b}'])
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .collect(),
    )
}

fn build_unit_lookup(units: &[OrgUnitRecord]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for unit in units {
        for value in [&unit.unit_id, &unit.name, &unit.path_name] {
            let key = value.trim();
            if !key.is_empty() {
                map.entry(key.to_string())
                    .or_insert_with(|| unit.unit_id.clone());
            }
        }
    }
    map
}

fn resolve_import_unit_id(
    raw: Option<&str>,
    unit_lookup: &HashMap<String, String>,
) -> AnyhowResult<Option<String>> {
    let cleaned = raw.map(str::trim).filter(|value| !value.is_empty());
    let Some(cleaned) = cleaned else {
        return Ok(None);
    };
    unit_lookup
        .get(cleaned)
        .cloned()
        .map(Some)
        .ok_or_else(|| anyhow!("unit not found: {cleaned}"))
}

fn import_failure(row: usize, username: String, message: &str) -> Value {
    json!({
        "row": row,
        "username": username,
        "message": message,
    })
}
