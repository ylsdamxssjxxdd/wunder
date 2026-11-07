//! Admin-facing release publishing for client self-updates.
//!
//! Clients fetch releases from `{storage.data_root}/releases/` through
//! `api::updates`. Until now an operator had to shell onto the server and drop
//! packages plus `manifest.json` by hand; these admin endpoints let the bridge
//! upload packages and publish the manifest with the same on-disk semantics —
//! 落盘即生效, no restart, no database. Integrity stays anchored on the
//! manifest sha256: it is recomputed from disk on every publish, so the
//! manifest can only ever vouch for bytes that actually exist.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::{DefaultBodyLimit, Multipart, Query as AxumQuery, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::api::errors::error_response_with_detail;
use crate::api::updates::releases_dir;
use crate::services::cloud::update::{normalize_version, update_file_name_is_safe, ReleaseManifest};
use crate::state::AppState;

/// Uploads stream straight to disk; the cap only bounds one package, and the
/// largest release piece (a supplement pack with a bundled toolchain) stays an
/// order of magnitude below it.
const MAX_PACKAGE_UPLOAD_BYTES: usize = 2 * 1024 * 1024 * 1024;
const COPY_CHUNK_BYTES: usize = 64 * 1024;

const CODE_UPDATE_UNAVAILABLE: &str = "UPDATE_UNAVAILABLE";
const CODE_UPDATE_FILE_INVALID: &str = "UPDATE_FILE_INVALID";
const CODE_UPDATE_FILE_REFERENCED: &str = "UPDATE_FILE_REFERENCED";

pub(super) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/wunder/admin/updates", axum::routing::get(get_updates))
        .route(
            "/wunder/admin/updates/packages",
            post(upload_package)
                .delete(delete_package)
                .layer(DefaultBodyLimit::max(MAX_PACKAGE_UPLOAD_BYTES)),
        )
        .route("/wunder/admin/updates/manifest", post(publish_manifest))
}

#[derive(Deserialize)]
struct FileQuery {
    file: String,
}

/// client/os/arch travel unencoded inside the cloud query string, so they are
/// restricted to plain tokens.
fn platform_token_is_safe(token: &str) -> bool {
    !token.is_empty()
        && token
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
}

fn release_version_is_valid(version: &str) -> bool {
    let normalized = normalize_version(version);
    !normalized.is_empty()
        && normalized
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '+'))
}

/// Package names the manifest already vouches for. Replacing bytes under a
/// published sha256 would silently break every in-flight download, so upload
/// and delete both refuse them.
fn referenced_files(manifest: &ReleaseManifest) -> HashSet<String> {
    manifest
        .releases
        .iter()
        .flat_map(|entry| entry.assets.iter())
        .map(|asset| asset.file.clone())
        .collect()
}

/// Upload-time name gate on top of the shared path-safety check: the manifest
/// file itself, part-file siblings and names the published manifest already
/// vouches for are refused.
fn upload_rejection(name: &str, referenced: &HashSet<String>) -> Option<&'static str> {
    if name == "manifest.json" || name.ends_with(".part") {
        return Some("this name is reserved");
    }
    if !update_file_name_is_safe(name) {
        return Some("invalid package file name");
    }
    if referenced.contains(name) {
        return Some("package is referenced by the published manifest; retire it first");
    }
    None
}

/// Tolerant read for admin operations: a missing or malformed manifest reads
/// as "nothing published" so the bridge can repair it by republishing.
async fn read_manifest_tolerant(dir: &Path) -> ReleaseManifest {
    match tokio::fs::read_to_string(dir.join("manifest.json")).await {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => ReleaseManifest::default(),
    }
}

/// `GET /wunder/admin/updates` — the published manifest plus the package files
/// on disk, so the bridge renders exactly what clients will receive. An
/// unconfigured or empty release directory degrades to an empty catalog.
async fn get_updates(State(state): State<Arc<AppState>>) -> Response {
    let Some(dir) = releases_dir(&state).await else {
        return Json(json!({ "releases_dir": "", "releases": [], "files": [] })).into_response();
    };
    let manifest = read_manifest_tolerant(&dir).await;
    let referenced = referenced_files(&manifest);
    let mut files: Vec<Value> = Vec::new();
    if let Ok(mut entries) = tokio::fs::read_dir(&dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if name == "manifest.json" || name.ends_with(".part") {
                continue;
            }
            if !update_file_name_is_safe(name) {
                continue;
            }
            let size = tokio::fs::metadata(&path)
                .await
                .map(|meta| meta.len())
                .unwrap_or(0);
            files.push(json!({
                "file": name,
                "size": size,
                "referenced": referenced.contains(name),
            }));
        }
    }
    files.sort_by(|left, right| {
        left["file"]
            .as_str()
            .unwrap_or_default()
            .cmp(right["file"].as_str().unwrap_or_default())
    });
    Json(json!({
        "releases_dir": dir.to_string_lossy(),
        "releases": manifest.releases,
        "files": files,
    }))
    .into_response()
}

async fn require_releases_dir(state: &AppState) -> Result<PathBuf, Response> {
    let Some(dir) = releases_dir(state).await else {
        return Err(unavailable("storage.data_root is not configured"));
    };
    tokio::fs::create_dir_all(&dir).await.map_err(|error| {
        unavailable(format!("release directory is unavailable: {error}"))
    })?;
    Ok(dir)
}

/// `POST /wunder/admin/updates/packages` — multipart upload of one package
/// file. The body streams to `<releases>/<name>` through a `.part` sibling
/// while hashing, so a multi-hundred-megabyte package never sits in memory.
/// Fields must arrive as `name` first, then `package`.
async fn upload_package(State(state): State<Arc<AppState>>, mut multipart: Multipart) -> Response {
    let dir = match require_releases_dir(&state).await {
        Ok(dir) => dir,
        Err(response) => return response,
    };
    let published = read_manifest_tolerant(&dir).await;
    let mut name: Option<String> = None;
    while let Some(mut field) = match multipart.next_field().await {
        Ok(Some(field)) => Some(field),
        Ok(None) => None,
        Err(error) => return invalid_file(format!("multipart upload failed: {error}")),
    } {
        let field_name = field.name().unwrap_or_default().to_string();
        if field_name == "name" {
            let value = match field.text().await {
                Ok(value) => value,
                Err(error) => return invalid_file(format!("multipart upload failed: {error}")),
            };
            name = Some(value.trim().to_string());
            continue;
        }
        if field_name != "package" {
            // Unknown field: drain its bytes so the request stays consumable.
            loop {
                match field.chunk().await {
                    Ok(Some(_)) => continue,
                    Ok(None) => break,
                    Err(error) => {
                        return invalid_file(format!("multipart upload failed: {error}"))
                    }
                }
            }
            continue;
        }
        let Some(package_name) = name.as_ref() else {
            return invalid_file("the name field must come before the package field");
        };
        if let Some(reason) = upload_rejection(package_name, &referenced_files(&published)) {
            return invalid_file(reason);
        }
        let part_path = dir.join(format!("{package_name}.part"));
        let final_path = dir.join(package_name);
        let mut file = match tokio::fs::File::create(&part_path).await {
            Ok(file) => file,
            Err(error) => return unavailable(format!("release file cannot be written: {error}")),
        };
        let mut hasher = Sha256::new();
        let mut written: u64 = 0;
        let mut failure: Option<Response> = None;
        loop {
            match field.chunk().await {
                Ok(Some(chunk)) => {
                    hasher.update(&chunk);
                    written += chunk.len() as u64;
                    if let Err(error) = file.write_all(&chunk).await {
                        failure =
                            Some(unavailable(format!("release file write failed: {error}")));
                        break;
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    failure = Some(invalid_file(format!("multipart upload failed: {error}")));
                    break;
                }
            }
        }
        if let Some(response) = failure {
            let _ = tokio::fs::remove_file(&part_path).await;
            return response;
        }
        if written == 0 {
            let _ = tokio::fs::remove_file(&part_path).await;
            return invalid_file("no package bytes were uploaded");
        }
        let flush_failure = file.flush().await.err().or(file.sync_all().await.err());
        drop(file);
        if let Some(error) = flush_failure {
            let _ = tokio::fs::remove_file(&part_path).await;
            return unavailable(format!("release file cannot be flushed: {error}"));
        }
        let _ = tokio::fs::remove_file(&final_path).await;
        if let Err(error) = tokio::fs::rename(&part_path, &final_path).await {
            let _ = tokio::fs::remove_file(&part_path).await;
            return unavailable(format!("release file cannot be placed: {error}"));
        }
        return Json(json!({
            "file": package_name,
            "sha256": format!("{:x}", hasher.finalize()),
            "size": written,
        }))
        .into_response();
    }
    invalid_file("the package field is missing")
}

fn invalid_file(message: impl Into<String>) -> Response {
    error_response_with_detail(
        StatusCode::BAD_REQUEST,
        Some(CODE_UPDATE_FILE_INVALID),
        message.into(),
        None,
        None,
    )
}

fn unavailable(message: impl Into<String>) -> Response {
    error_response_with_detail(
        StatusCode::INTERNAL_SERVER_ERROR,
        Some(CODE_UPDATE_UNAVAILABLE),
        message.into(),
        None,
        None,
    )
}

/// `POST /wunder/admin/updates/manifest` — publish the whole release catalog.
/// Every asset's sha256 and size are recomputed from the package on disk, so a
/// manifest can only reference bytes that actually exist; `published_at`
/// defaults to now for entries that omit it.
async fn publish_manifest(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ReleaseManifest>,
) -> Response {
    let dir = match require_releases_dir(&state).await {
        Ok(dir) => dir,
        Err(response) => return response,
    };
    let mut manifest = ReleaseManifest {
        releases: payload.releases,
    };
    if let Err(message) = verify_manifest_packages(&dir, &mut manifest).await {
        return invalid_file(message);
    }
    let body = match serde_json::to_vec_pretty(&manifest) {
        Ok(body) => body,
        Err(error) => return unavailable(format!("release manifest cannot be encoded: {error}")),
    };
    let tmp_path = dir.join(format!("manifest.json.new-{}", std::process::id()));
    let final_path = dir.join("manifest.json");
    if let Err(error) = tokio::fs::write(&tmp_path, &body).await {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return unavailable(format!("release manifest cannot be written: {error}"));
    }
    let _ = tokio::fs::remove_file(&final_path).await;
    if let Err(error) = tokio::fs::rename(&tmp_path, &final_path).await {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return unavailable(format!("release manifest cannot be placed: {error}"));
    }
    Json(json!({ "releases": manifest.releases })).into_response()
}

/// Validate every release entry against the packages on disk and fill each
/// asset's integrity fields from the actual bytes: sha256, size, and the
/// flattened version/published_at copies.
async fn verify_manifest_packages(
    dir: &Path,
    manifest: &mut ReleaseManifest,
) -> Result<(), String> {
    if manifest.releases.is_empty() {
        return Err("the release catalog is empty".to_string());
    }
    let now = chrono::Utc::now().timestamp();
    for entry in &mut manifest.releases {
        let version = normalize_version(&entry.version);
        if !release_version_is_valid(&version) {
            return Err(format!("invalid release version: {:?}", entry.version));
        }
        entry.version = version;
        if entry.published_at.is_none() {
            entry.published_at = Some(now);
        }
        if entry.assets.is_empty() {
            return Err(format!("release {} has no assets", entry.version));
        }
        for asset in &mut entry.assets {
            if !platform_token_is_safe(&asset.client)
                || !platform_token_is_safe(&asset.os)
                || !platform_token_is_safe(&asset.arch)
            {
                return Err(format!(
                    "release {} has an invalid client/os/arch token",
                    entry.version
                ));
            }
            if !update_file_name_is_safe(&asset.file) {
                return Err(format!(
                    "release {} has an invalid package file name",
                    entry.version
                ));
            }
            let (sha256, size) = hash_package(&dir.join(&asset.file)).await?;
            asset.sha256 = sha256;
            asset.size = size;
            asset.version = entry.version.clone();
            asset.published_at = entry.published_at;
            asset.url.clear();
        }
    }
    Ok(())
}

/// Stream one package to hash without holding it in memory. A missing or
/// unreadable package is a publish error, never a silent entry.
async fn hash_package(path: &Path) -> Result<(String, u64), String> {
    use tokio::io::AsyncReadExt;
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|error| format!("package {} is not on disk: {error}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut size: u64 = 0;
    let mut buffer = vec![0u8; COPY_CHUNK_BYTES];
    loop {
        let read = file
            .read(&mut buffer)
            .await
            .map_err(|error| format!("package {} is unreadable: {error}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        size += read as u64;
    }
    Ok((format!("{:x}", hasher.finalize()), size))
}

/// `DELETE /wunder/admin/updates/packages?file=` — remove one package file.
/// Referenced files are refused: retire them from the manifest first.
async fn delete_package(
    State(state): State<Arc<AppState>>,
    AxumQuery(query): AxumQuery<FileQuery>,
) -> Response {
    if !update_file_name_is_safe(&query.file) {
        return invalid_file("invalid package file name");
    }
    let Some(dir) = releases_dir(&state).await else {
        return unavailable("storage.data_root is not configured");
    };
    let manifest = read_manifest_tolerant(&dir).await;
    if referenced_files(&manifest).contains(&query.file) {
        return error_response_with_detail(
            StatusCode::CONFLICT,
            Some(CODE_UPDATE_FILE_REFERENCED),
            "package is referenced by the published manifest; retire it first",
            None,
            None,
        );
    }
    match tokio::fs::remove_file(dir.join(&query.file)).await {
        Ok(()) => Json(json!({ "deleted": query.file })).into_response(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => error_response_with_detail(
            StatusCode::NOT_FOUND,
            Some(CODE_UPDATE_FILE_INVALID),
            "package file is not on disk",
            None,
            None,
        ),
        Err(error) => unavailable(format!("package file cannot be removed: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::cloud::update::{ReleaseEntry, UpdateAsset};

    fn entry(version: &str, assets: Vec<UpdateAsset>) -> ReleaseEntry {
        ReleaseEntry {
            version: version.to_string(),
            published_at: None,
            assets,
        }
    }

    fn asset(client: &str, os: &str, arch: &str, file: &str) -> UpdateAsset {
        UpdateAsset {
            client: client.to_string(),
            os: os.to_string(),
            arch: arch.to_string(),
            file: file.to_string(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn publish_verifies_bytes_and_fills_integrity_fields() {
        let dir =
            std::env::temp_dir().join(format!("wunder-admin-upd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("desktop-0.3.0-win7-x86.exe"), b"package-bytes").unwrap();
        let mut manifest = ReleaseManifest {
            releases: vec![entry(
                "0.3.0",
                vec![asset("desktop", "windows", "x86", "desktop-0.3.0-win7-x86.exe")],
            )],
        };
        verify_manifest_packages(&dir, &mut manifest).await.unwrap();
        let published = &manifest.releases[0];
        assert!(published.published_at.is_some(), "publish time is filled");
        let store = &published.assets[0];
        assert_eq!(store.size, b"package-bytes".len() as u64);
        assert_eq!(
            store.sha256,
            format!("{:x}", Sha256::digest(b"package-bytes"))
        );
        assert_eq!(store.version, "0.3.0");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn publish_rejects_missing_files_and_bad_tokens() {
        let dir = std::env::temp_dir()
            .join(format!("wunder-admin-upd-miss-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut missing = ReleaseManifest {
            releases: vec![entry(
                "0.3.0",
                vec![asset("desktop", "windows", "x86", "missing.exe")],
            )],
        };
        assert!(verify_manifest_packages(&dir, &mut missing).await.is_err());
        let mut token = ReleaseManifest {
            releases: vec![entry(
                "0.3.0",
                vec![asset("desktop", "win dows", "x86", "a.exe")],
            )],
        };
        assert!(verify_manifest_packages(&dir, &mut token).await.is_err());
        let mut version = ReleaseManifest {
            releases: vec![entry("", vec![asset("desktop", "windows", "x86", "a.exe")])],
        };
        assert!(verify_manifest_packages(&dir, &mut version).await.is_err());
        let mut empty = ReleaseManifest { releases: vec![] };
        assert!(verify_manifest_packages(&dir, &mut empty).await.is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn upload_gate_refuses_reserved_referenced_and_unsafe_names() {
        let mut referenced = HashSet::new();
        referenced.insert("published.exe".to_string());
        assert_eq!(
            upload_rejection("manifest.json", &referenced),
            Some("this name is reserved")
        );
        assert_eq!(
            upload_rejection("pkg.part", &referenced),
            Some("this name is reserved")
        );
        assert_eq!(
            upload_rejection("published.exe", &referenced),
            Some("package is referenced by the published manifest; retire it first")
        );
        assert_eq!(
            upload_rejection("../evil", &referenced),
            Some("invalid package file name")
        );
        assert_eq!(upload_rejection("desktop-0.3.0-win7-x86.exe", &referenced), None);
    }

    #[test]
    fn platform_tokens_stay_plain_and_versions_semverish() {
        assert!(platform_token_is_safe("win7-x86"));
        assert!(!platform_token_is_safe(""));
        assert!(!platform_token_is_safe("win dows"));
        assert!(!platform_token_is_safe("../x"));
        assert!(release_version_is_valid("0.3.0"));
        assert!(release_version_is_valid("v0.3.0-rc1"));
        assert!(!release_version_is_valid(""));
        assert!(!release_version_is_valid("0.3.0 beta"));
    }
}
