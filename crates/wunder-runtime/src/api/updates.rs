//! Client update distribution over the cloud surface.
//!
//! Operators publish a release by dropping the package plus `manifest.json`
//! into `{storage.data_root}/releases/` — no restart, no database. Logged-in
//! local clients (desktop/cli) resolve the newest asset for their
//! client/os/arch triple and stream the package through the same bearer-token
//! surface as the rest of the cloud API. Download paths are restricted to
//! manifest-listed assets, so this endpoint can never serve an arbitrary file
//! from the server disk.

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Query as AxumQuery, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio_util::io::ReaderStream;

use crate::api::cloud::{authenticate_cloud, require_cloud_enabled};
use crate::api::errors::error_response_with_detail;
use crate::services::cloud::update::{update_file_name_is_safe, ReleaseManifest, UpdateAsset};
use crate::state::AppState;

const CODE_UPDATE_UNAVAILABLE: &str = "UPDATE_UNAVAILABLE";
const CODE_UPDATE_FILE_INVALID: &str = "UPDATE_FILE_INVALID";

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/wunder/cloud/updates/latest", get(latest_update))
        .route("/wunder/cloud/updates/download", get(download_update))
}

#[derive(Deserialize)]
struct UpdateQuery {
    client: String,
    os: String,
    arch: String,
}

#[derive(Deserialize)]
struct DownloadQuery {
    file: String,
}

/// Client-facing projection of one asset: the manifest record flattened with
/// its owning release version and publish time.
#[derive(Serialize)]
struct UpdateOffering {
    version: String,
    published_at: Option<i64>,
    file: String,
    sha256: String,
    size: u64,
    notes: String,
    mandatory: bool,
}

impl UpdateOffering {
    fn from_asset(asset: UpdateAsset) -> Self {
        Self {
            version: asset.version,
            published_at: asset.published_at,
            file: asset.file,
            sha256: asset.sha256,
            size: asset.size,
            notes: asset.notes,
            mandatory: asset.mandatory,
        }
    }
}

/// Release directory below the configured storage data root. Missing
/// configuration or a missing directory degrades to "no releases published".
pub(crate) async fn releases_dir(state: &AppState) -> Option<PathBuf> {
    let config = state.config_store.get().await;
    let root = config.storage.data_root.trim();
    if root.is_empty() {
        return None;
    }
    Some(PathBuf::from(root).join("releases"))
}

pub(crate) async fn load_manifest(state: &AppState) -> Result<ReleaseManifest, Response> {
    let Some(dir) = releases_dir(state).await else {
        return Err(unavailable_error());
    };
    let path = dir.join("manifest.json");
    let text = match tokio::fs::read_to_string(&path).await {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(unavailable_error());
        }
        Err(error) => {
            return Err(error_response_with_detail(
                StatusCode::INTERNAL_SERVER_ERROR,
                Some(CODE_UPDATE_UNAVAILABLE),
                format!("release manifest is unreadable: {error}"),
                None,
                None,
            ));
        }
    };
    serde_json::from_str(&text).map_err(|error| {
        error_response_with_detail(
            StatusCode::INTERNAL_SERVER_ERROR,
            Some(CODE_UPDATE_UNAVAILABLE),
            format!("release manifest is malformed: {error}"),
            None,
            None,
        )
    })
}

fn unavailable_error() -> Response {
    error_response_with_detail(
        StatusCode::NOT_FOUND,
        Some(CODE_UPDATE_UNAVAILABLE),
        "no release catalog is published on this server",
        None,
        None,
    )
}

/// `GET /wunder/cloud/updates/latest?client=&os=&arch=` — the newest asset
/// for the requesting platform, or `{"data": null}` when nothing matches.
async fn latest_update(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumQuery(query): AxumQuery<UpdateQuery>,
) -> Response {
    if let Err(response) = require_cloud_enabled(&state).await {
        return response;
    }
    if let Err(response) = authenticate_cloud(&state, &headers).await {
        return response;
    }
    let manifest = match load_manifest(&state).await {
        Ok(manifest) => manifest,
        Err(response) => return response,
    };
    let offering = manifest
        .select_asset(&query.client, &query.os, &query.arch)
        .map(UpdateOffering::from_asset);
    (
        StatusCode::OK,
        Json(json!({ "ok": true, "data": offering })),
    )
        .into_response()
}

/// `GET /wunder/cloud/updates/download?file=` — stream one manifest-listed
/// release package. The sha256 travels in a response header so the client can
/// verify the transfer without a second metadata round-trip.
async fn download_update(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumQuery(query): AxumQuery<DownloadQuery>,
) -> Response {
    if let Err(response) = require_cloud_enabled(&state).await {
        return response;
    }
    if let Err(response) = authenticate_cloud(&state, &headers).await {
        return response;
    }
    if !update_file_name_is_safe(&query.file) {
        return error_response_with_detail(
            StatusCode::BAD_REQUEST,
            Some(CODE_UPDATE_FILE_INVALID),
            "invalid release file name",
            None,
            None,
        );
    }
    let manifest = match load_manifest(&state).await {
        Ok(manifest) => manifest,
        Err(response) => return response,
    };
    if !manifest.contains_file(&query.file) {
        return error_response_with_detail(
            StatusCode::NOT_FOUND,
            Some(CODE_UPDATE_FILE_INVALID),
            "release file is not listed in the manifest",
            None,
            None,
        );
    }
    let Some(dir) = releases_dir(&state).await else {
        return unavailable_error();
    };
    let path = dir.join(&query.file);
    let file = match tokio::fs::File::open(&path).await {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return error_response_with_detail(
                StatusCode::NOT_FOUND,
                Some(CODE_UPDATE_FILE_INVALID),
                "release file is listed but missing from the release directory",
                None,
                None,
            );
        }
        Err(error) => {
            return error_response_with_detail(
                StatusCode::INTERNAL_SERVER_ERROR,
                Some(CODE_UPDATE_UNAVAILABLE),
                format!("release file is unreadable: {error}"),
                None,
                None,
            );
        }
    };
    let size = file.metadata().await.ok().map(|meta| meta.len());
    let sha256 = manifest
        .releases
        .iter()
        .flat_map(|entry| entry.assets.iter())
        .find(|asset| asset.file == query.file)
        .map(|asset| asset.sha256.clone())
        .unwrap_or_default();
    let stream = ReaderStream::with_capacity(file, 64 * 1024);
    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header("x-wunder-sha256", sha256);
    if let Some(size) = size {
        builder = builder.header(header::CONTENT_LENGTH, size.to_string());
    }
    builder
        .body(Body::from_stream(stream))
        .unwrap_or_else(|error| {
            error_response_with_detail(
                StatusCode::INTERNAL_SERVER_ERROR,
                Some(CODE_UPDATE_UNAVAILABLE),
                format!("release stream failed: {error}"),
                None,
                None,
            )
        })
}
