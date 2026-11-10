//! Client self-update distribution.
//!
//! Two sources publish the same `manifest.json` + package pairs:
//! - the logged-in cloud channel (`api::updates`): the server keeps releases
//!   below `{storage.data_root}/releases/`, downloads run authenticated;
//! - public GitHub releases: the release carries `manifest.json` as an asset,
//!   downloads go through `browser_download_url` with no session.
//!
//! Both produce the [`UpdateAsset`] record consumed by the desktop façade;
//! integrity always rests on the manifest's sha256, never on the transport.
//! The dotted-version comparison shared by both sides lives here too.

use super::service::{error_message, CloudService};
use super::{CloudSessionExpired, CloudTransportError};
use anyhow::{anyhow, bail, Result};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::cmp::Ordering as CmpOrdering;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::time::Duration;
use tokio::io::AsyncWriteExt;

/// Server routes, mirrored by `api::updates`. Kept here so the client and the
/// server cannot drift apart silently.
pub const UPDATES_LATEST_PATH: &str = "/wunder/cloud/updates/latest";
pub const UPDATES_DOWNLOAD_PATH: &str = "/wunder/cloud/updates/download";

/// A stalled download must produce an error, not hang forever: no body chunk
/// for this long aborts the transfer (already-written bytes stay on disk as a
/// `.part` file the caller cleans up).
const DOWNLOAD_CHUNK_IDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// One downloadable package inside a release: bound to exactly one
/// client/os/arch triple. `version` and `published_at` are flattened copies of
/// the owning release entry so a client-facing projection stays a single value.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UpdateAsset {
    #[serde(default)]
    pub client: String,
    #[serde(default)]
    pub os: String,
    #[serde(default)]
    pub arch: String,
    /// File name inside the release directory; never contains a path separator.
    #[serde(default)]
    pub file: String,
    /// Lowercase hex sha256 of the package, the download integrity anchor.
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub mandatory: bool,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub published_at: Option<i64>,
    /// Absolute download URL for sources that publish one (GitHub releases).
    /// Empty means "use the authenticated server endpoint with `file`".
    #[serde(default)]
    pub url: String,
}

/// One entry of the on-disk `manifest.json` in the release directory.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ReleaseEntry {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub published_at: Option<i64>,
    #[serde(default)]
    pub assets: Vec<UpdateAsset>,
}

/// Root shape of the on-disk `manifest.json`. Operators publish a release by
/// dropping the package next to this file; no server restart is required.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ReleaseManifest {
    #[serde(default)]
    pub releases: Vec<ReleaseEntry>,
}

impl ReleaseManifest {
    /// Newest asset matching the client/os/arch triple, or `None` when the
    /// manifest has nothing for this platform. Version comparison is numeric
    /// dotted, so "0.2.10" correctly beats "0.2.9".
    pub fn select_asset(&self, client: &str, os: &str, arch: &str) -> Option<UpdateAsset> {
        let mut best: Option<UpdateAsset> = None;
        for entry in &self.releases {
            for asset in &entry.assets {
                if asset.client != client || asset.os != os || asset.arch != arch {
                    continue;
                }
                let mut flattened = asset.clone();
                flattened.version = entry.version.clone();
                flattened.published_at = entry.published_at;
                let replace = match &best {
                    None => true,
                    Some(current) => {
                        compare_release_versions(&flattened.version, &current.version)
                            == CmpOrdering::Greater
                    }
                };
                if replace {
                    best = Some(flattened);
                }
            }
        }
        best
    }

    /// Whether `file` names an asset this manifest publishes. The download
    /// endpoint only serves files it can vouch for.
    pub fn contains_file(&self, file: &str) -> bool {
        self.releases
            .iter()
            .any(|entry| entry.assets.iter().any(|asset| asset.file == file))
    }
}

/// Numeric dotted-version comparison ("0.2.10" > "0.2.9"). Build metadata
/// ("+build.7") is stripped; a pre-release suffix ("-rc1") is stripped for the
/// numeric core and then applied as a semver-style tiebreaker — a suffixed
/// version ranks below its bare release. Equal shapes fall back to the full
/// strings so a suffix can never silently downgrade an equal-versioned
/// package.
pub fn compare_release_versions(left: &str, right: &str) -> CmpOrdering {
    let left = normalize_version(left);
    let right = normalize_version(right);
    let numeric_core = |version: &str| -> Vec<u64> {
        let core = version.split(['-', '+']).next().unwrap_or(version);
        core.split('.')
            .map(|part| part.trim().parse::<u64>().unwrap_or(0))
            .collect()
    };
    let (left_parts, right_parts) = (numeric_core(&left), numeric_core(&right));
    for index in 0..left_parts.len().max(right_parts.len()) {
        let left_value = left_parts.get(index).copied().unwrap_or(0);
        let right_value = right_parts.get(index).copied().unwrap_or(0);
        if left_value != right_value {
            return left_value.cmp(&right_value);
        }
    }
    match (left.contains('-'), right.contains('-')) {
        (true, false) => CmpOrdering::Less,
        (false, true) => CmpOrdering::Greater,
        _ => left.cmp(&right),
    }
}

/// Trim whitespace and a leading `v`/`V` so GitHub tags ("v0.2.7") and
/// manifest versions ("0.2.7") compare equal.
pub fn normalize_version(version: &str) -> String {
    let trimmed = version.trim();
    match trimmed.strip_prefix(['v', 'V']) {
        Some(rest)
            if !rest.is_empty() && rest.chars().next().is_some_and(|ch| ch.is_ascii_digit()) =>
        {
            rest.to_string()
        }
        _ => trimmed.to_string(),
    }
}

/// True when `candidate` is strictly newer than `current`.
pub fn is_newer_version(candidate: &str, current: &str) -> bool {
    compare_release_versions(candidate.trim(), current.trim()) == CmpOrdering::Greater
}

impl CloudService {
    /// Ask the logged-in server for the newest release asset matching
    /// client/os/arch. `Ok(None)` covers both "nothing published" and a server
    /// without the update surface (404), so an older fleet server degrades to
    /// "no updates available" instead of an error.
    pub async fn fetch_latest_update(
        &self,
        client: &str,
        os_name: &str,
        arch: &str,
    ) -> Result<Option<UpdateAsset>> {
        let response = self
            .authed_send(|session| {
                // Query params are built by hand: client/os/arch are process
                // constants and file names pass `update_file_name_is_safe`, so
                // no percent-encoding can be needed. `RequestBuilder::query`
                // sits behind a reqwest 0.13 feature the win7 pin does not
                // enable.
                self.http.get(format!(
                    "{session_server}{UPDATES_LATEST_PATH}?client={client}&os={os_name}&arch={arch}",
                    session_server = session.server
                ))
            })
            .await?;
        let status = response.status();
        if status.as_u16() == 404 {
            return Ok(None);
        }
        if !status.is_success() {
            let body = read_update_json(response).await;
            bail!("update lookup failed: {status} {}", error_message(&body));
        }
        let body = read_update_json(response).await;
        let payload = body.get("data").cloned().unwrap_or(serde_json::Value::Null);
        if payload.is_null() {
            return Ok(None);
        }
        let asset: UpdateAsset = serde_json::from_value(payload)
            .map_err(|error| anyhow!("malformed update manifest: {error}"))?;
        Ok(Some(asset))
    }

    /// Stream one release asset to `dest` (typically a `.part` staging path),
    /// hashing while writing so the integrity check costs no second pass.
    /// Returns the byte count and the lowercase hex sha256 of what actually
    /// landed on disk; the caller compares against the manifest value.
    pub async fn download_update_file(
        &self,
        file: &str,
        dest: &Path,
        progress: &AtomicU64,
    ) -> Result<(u64, String)> {
        let mut response = self.send_update_download(file).await?;
        if response.status().as_u16() == 401 {
            // Single-flight silent refresh, then exactly one retry — the same
            // contract as `authed_send`, kept explicit because this request
            // runs without a total timeout.
            let stale = self
                .session()
                .map(|session| session.token)
                .unwrap_or_default();
            self.recover_auth(&stale).await?;
            response = self.send_update_download(file).await?;
        }
        let status = response.status();
        if !status.is_success() {
            let body = read_update_json(response).await;
            bail!("update download failed: {status} {}", error_message(&body));
        }
        stream_download_response(response, dest, progress).await
    }

    /// Raw authenticated GET for one release asset. Network failures feed the
    /// connection state machine through the shared transport error path.
    async fn send_update_download(&self, file: &str) -> Result<reqwest::Response> {
        if !update_file_name_is_safe(file) {
            bail!("invalid update file name");
        }
        let Some(session) = self.session() else {
            return Err(anyhow!(CloudSessionExpired));
        };
        let attempt = self
            .http
            .get(format!(
                "{session_server}{UPDATES_DOWNLOAD_PATH}?file={file}",
                session_server = session.server
            ))
            .bearer_auth(&session.token)
            .header("x-wunder-device-id", &session.device_id)
            .send()
            .await;
        match attempt {
            Ok(response) => Ok(response),
            Err(error) => {
                let summary = super::network_error_summary(&error);
                Err(anyhow!(CloudTransportError(summary)))
            }
        }
    }
}

/// Release file names are operator-published package names: a single safe
/// path component. Anything that could escape the release directory is
/// rejected before it reaches the filesystem.
pub fn update_file_name_is_safe(file: &str) -> bool {
    !file.is_empty()
        && !file.starts_with('.')
        && !file.contains('/')
        && !file.contains('\\')
        && !file.contains("..")
        && file
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_' | '(' | ')'))
}

async fn read_update_json(response: reqwest::Response) -> serde_json::Value {
    match response.text().await {
        Ok(text) => serde_json::from_str(&text).unwrap_or(serde_json::Value::Null),
        Err(_) => serde_json::Value::Null,
    }
}

// ---------------------------------------------------------------------------
// Public GitHub releases source
// ---------------------------------------------------------------------------

/// Unauthenticated GitHub REST endpoint for the newest stable release
/// (prereleases and drafts are excluded server-side).
pub const GITHUB_RELEASES_LATEST: &str = "https://api.github.com/repos";

fn github_repo_is_valid(repo: &str) -> bool {
    !repo.is_empty()
        && repo
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '.' | '_' | '/'))
}

/// Parsed subset of the GitHub release payload we rely on.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct GitHubRelease {
    #[serde(default)]
    pub tag_name: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub html_url: String,
    #[serde(default)]
    pub published_at: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub assets: Vec<GitHubReleaseAsset>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct GitHubReleaseAsset {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub browser_download_url: String,
    #[serde(default)]
    pub size: u64,
    /// GitHub reports `sha256:<hex>` for release assets since 2024.
    #[serde(default)]
    pub digest: String,
}

/// One Android APK offered by a public release. The Android shell installs it
/// through the system package installer; the desktop updater never sees this
/// type because its packages are executable files, not APKs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AndroidRelease {
    pub version: String,
    pub notes: String,
    pub url: String,
    /// Server-side release file name; the authenticated cloud download uses
    /// this instead of a public URL.
    pub file: String,
    pub sha256: String,
    pub size: u64,
    pub published_at_ms: i64,
    pub html_url: String,
}

impl AndroidRelease {
    /// Flatten a cloud manifest asset into the Android update projection.
    pub fn from_update_asset(asset: UpdateAsset) -> Self {
        Self {
            version: asset.version,
            notes: asset.notes,
            url: asset.url,
            file: asset.file,
            sha256: asset.sha256,
            size: asset.size,
            published_at_ms: asset.published_at.unwrap_or(0).saturating_mul(1000),
            html_url: String::new(),
        }
    }
}

impl GitHubRelease {
    fn asset_url(&self, name: &str) -> Option<String> {
        self.assets
            .iter()
            .find(|asset| asset.name == name)
            .map(|asset| asset.browser_download_url.clone())
    }

    fn manifest_url(&self) -> Option<String> {
        self.asset_url("manifest.json")
    }

    fn published_ms(&self) -> i64 {
        let timestamp = if self.published_at.is_empty() {
            &self.created_at
        } else {
            &self.published_at
        };
        chrono::DateTime::parse_from_rfc3339(timestamp)
            .map(|value| value.timestamp_millis())
            .unwrap_or(0)
    }
}

/// Parse a release payload and pick the newest manifest asset for the
/// platform, attaching the release's absolute download URL. The manifest is
/// the same file the cloud channel serves, so sha256 verification and version
/// selection are shared one-for-one with that path.
pub fn select_github_asset(
    manifest_text: &str,
    release: &GitHubRelease,
    client: &str,
    os: &str,
    arch: &str,
) -> Result<Option<UpdateAsset>> {
    let manifest: ReleaseManifest = serde_json::from_str(manifest_text)
        .map_err(|error| anyhow!("malformed update manifest: {error}"))?;
    let mut asset = match manifest.select_asset(client, os, arch) {
        Some(asset) => asset,
        None => return Ok(None),
    };
    asset.url = release.asset_url(&asset.file).ok_or_else(|| {
        anyhow!(
            "release is missing the manifest-listed package {}",
            asset.file
        )
    })?;
    if asset.version.is_empty() {
        asset.version = normalize_version(&release.tag_name);
    }
    Ok(Some(asset))
}

/// Pick the newest Android APK across a list of GitHub releases. The APK's own
/// file name (`wunder-android-<version>-<arch>.apk`) is the version source
/// because the nightly release uses a moving tag that is not a version.
/// Releases without a matching asset, drafts and assets without a download URL
/// are ignored.
pub fn select_github_android_release(
    releases: &[GitHubRelease],
    arch_label: &str,
) -> Option<AndroidRelease> {
    let suffix = format!("-{arch_label}.apk");
    let mut best: Option<AndroidRelease> = None;
    for release in releases.iter().filter(|release| !release.draft) {
        for asset in &release.assets {
            let Some(version) = asset
                .name
                .strip_prefix("wunder-android-")
                .and_then(|rest| rest.strip_suffix(&suffix))
                .map(str::trim)
                .filter(|version| !version.is_empty())
            else {
                continue;
            };
            if asset.browser_download_url.is_empty() {
                continue;
            }
            let digest = asset.digest.trim();
            let sha256 = digest
                .strip_prefix("sha256:")
                .unwrap_or(digest)
                .trim()
                .to_ascii_lowercase();
            let candidate = AndroidRelease {
                version: version.to_string(),
                notes: release.body.clone(),
                url: asset.browser_download_url.clone(),
                file: asset.name.clone(),
                sha256,
                size: asset.size,
                published_at_ms: release.published_ms(),
                html_url: release.html_url.clone(),
            };
            let replace = match &best {
                None => true,
                Some(current) => {
                    match compare_release_versions(&candidate.version, &current.version) {
                        CmpOrdering::Greater => true,
                        CmpOrdering::Equal => candidate.published_at_ms > current.published_at_ms,
                        CmpOrdering::Less => false,
                    }
                }
            };
            if replace {
                best = Some(candidate);
            }
        }
    }
    best
}

/// A source-agnostic HTTP client for public update sources. Owns its pooled
/// client; one instance per process is plenty (checks and downloads are rare).
pub struct UpdateSourceClient {
    http: reqwest::Client,
}

impl Default for UpdateSourceClient {
    fn default() -> Self {
        Self::new()
    }
}

impl UpdateSourceClient {
    pub fn new() -> Self {
        let http = crate::http_client::builder()
            .connect_timeout(Duration::from_secs(10))
            .user_agent(concat!("wunder-updater/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_default();
        Self { http }
    }

    /// Newest asset for the platform from a GitHub repository's latest
    /// release. `Ok(None)` means the repo publishes nothing matching this
    /// platform. GitHub rejects requests without a User-Agent, which the
    /// shared client always sends.
    pub async fn fetch_github_latest(
        &self,
        repo: &str,
        client: &str,
        os_name: &str,
        arch: &str,
    ) -> Result<Option<UpdateAsset>> {
        if !github_repo_is_valid(repo) {
            bail!("GitHub 更新源仓库标识无效");
        }
        let release = self
            .http
            .get(format!("{GITHUB_RELEASES_LATEST}/{repo}/releases/latest"))
            .header("Accept", "application/vnd.github+json")
            .timeout(Duration::from_secs(20))
            .send()
            .await
            .map_err(|error| anyhow!("GitHub 更新源不可达：{error}"))?;
        let status = release.status();
        if status.as_u16() == 404 {
            return Ok(None);
        }
        if !status.is_success() {
            bail!("GitHub 更新源查询失败：{status}");
        }
        let release: GitHubRelease = release
            .json()
            .await
            .map_err(|error| anyhow!("GitHub 发布信息解析失败：{error}"))?;
        let Some(manifest_url) = release.manifest_url() else {
            bail!("GitHub 最新发布缺少 manifest.json 资产");
        };
        let manifest_text = self
            .http
            .get(&manifest_url)
            .timeout(Duration::from_secs(20))
            .send()
            .await
            .map_err(|error| anyhow!("更新清单下载失败：{error}"))?
            .error_for_status()
            .map_err(|error| anyhow!("更新清单下载失败：{error}"))?
            .text()
            .await
            .map_err(|error| anyhow!("更新清单下载失败：{error}"))?;
        select_github_asset(&manifest_text, &release, client, os_name, arch)
    }

    /// Newest Android APK across the repository's recent releases. Unlike the
    /// desktop source this does not need a `manifest.json`: the APK file name
    /// carries the version, and GitHub reports a sha256 digest per asset.
    /// Nightly releases use a moving tag, so the caller decides whether a
    /// same-version APK published after the installed build is an update.
    pub async fn fetch_github_android(
        &self,
        repo: &str,
        arch_label: &str,
    ) -> Result<Option<AndroidRelease>> {
        if !github_repo_is_valid(repo) {
            bail!("GitHub 更新源仓库标识无效");
        }
        let response = self
            .http
            .get(format!(
                "{GITHUB_RELEASES_LATEST}/{repo}/releases?per_page=20"
            ))
            .header("Accept", "application/vnd.github+json")
            .timeout(Duration::from_secs(20))
            .send()
            .await
            .map_err(|error| anyhow!("GitHub 更新源不可达：{error}"))?;
        let status = response.status();
        if status.as_u16() == 404 {
            return Ok(None);
        }
        if status.as_u16() == 403 {
            bail!("GitHub 更新源请求过于频繁，请稍后再试");
        }
        if !status.is_success() {
            bail!("GitHub 更新源查询失败：{status}");
        }
        let releases: Vec<GitHubRelease> = response
            .json()
            .await
            .map_err(|error| anyhow!("GitHub 发布信息解析失败：{error}"))?;
        Ok(select_github_android_release(&releases, arch_label))
    }

    /// Stream a release asset that publishes an absolute URL (no session;
    /// redirects to the release CDN are followed by the client).
    pub async fn download_asset(
        &self,
        asset: &UpdateAsset,
        dest: &Path,
        progress: &AtomicU64,
    ) -> Result<(u64, String)> {
        if asset.url.is_empty() {
            bail!("更新资产缺少下载地址");
        }
        let response = self
            .http
            .get(&asset.url)
            .send()
            .await
            .map_err(|error| anyhow!("更新包下载失败：{error}"))?;
        let status = response.status();
        if !status.is_success() {
            bail!("更新包下载失败：{status}");
        }
        stream_download_response(response, dest, progress).await
    }
}

/// Shared streaming body: write chunk-by-chunk to `dest`, hash on the fly and
/// report cumulative bytes. No total timeout (packages outlive the API
/// budget); a stalled connection surfaces via the per-chunk idle watchdog.
async fn stream_download_response(
    response: reqwest::Response,
    dest: &Path,
    progress: &AtomicU64,
) -> Result<(u64, String)> {
    let mut dest_file = tokio::fs::File::create(dest).await?;
    let mut hasher = Sha256::new();
    let mut written: u64 = 0;
    let mut stream = response.bytes_stream();
    loop {
        let next = match tokio::time::timeout(DOWNLOAD_CHUNK_IDLE_TIMEOUT, stream.next()).await {
            Ok(next) => next,
            Err(_) => {
                bail!(
                    "update download stalled for {}s",
                    DOWNLOAD_CHUNK_IDLE_TIMEOUT.as_secs()
                )
            }
        };
        let Some(chunk) = next else { break };
        let chunk = chunk.map_err(|error| anyhow!("update download interrupted: {error}"))?;
        hasher.update(&chunk);
        dest_file.write_all(&chunk).await?;
        written += chunk.len() as u64;
        progress.store(written, AtomicOrdering::Relaxed);
    }
    dest_file.flush().await?;
    Ok((written, format!("{:x}", hasher.finalize())))
}

#[cfg(test)]
mod tests {

    use super::*;

    fn manifest() -> ReleaseManifest {
        serde_json::from_str(
            r#"{
                "releases": [
                    {
                        "version": "0.2.6",
                        "published_at": 1790000000,
                        "assets": [
                            {"client": "desktop", "os": "windows", "arch": "x86",
                             "file": "desktop-0.2.6-win7-x86.exe", "sha256": "abc", "size": 10},
                            {"client": "cli", "os": "windows", "arch": "x86",
                             "file": "cli-0.2.6-win7-x86.exe", "sha256": "abd", "size": 5},
                            {"client": "supplement", "os": "windows", "arch": "x86",
                             "file": "supplement-0.2.6-win7-x86.zip", "sha256": "abf", "size": 900}
                        ]
                    },
                    {
                        "version": "0.2.10",
                        "assets": [
                            {"client": "desktop", "os": "windows", "arch": "x86",
                             "file": "desktop-0.2.10-win7-x86.exe", "sha256": "abe", "size": 12}
                        ]
                    }
                ]
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn dotted_versions_compare_numerically() {
        assert_eq!(
            compare_release_versions("0.2.10", "0.2.9"),
            CmpOrdering::Greater
        );
        assert_eq!(
            compare_release_versions("0.10.0", "0.9.9"),
            CmpOrdering::Greater
        );
        assert_eq!(
            compare_release_versions("0.2.6", "0.2.6"),
            CmpOrdering::Equal
        );
        assert_eq!(
            compare_release_versions("0.2.6-rc1", "0.2.6"),
            CmpOrdering::Less
        );
        assert_eq!(
            compare_release_versions("0.2.6", "0.2.6-rc1"),
            CmpOrdering::Greater
        );
        assert!(is_newer_version("0.3.0", "0.2.6"));
        assert!(!is_newer_version("0.2.6", "0.2.6"));
        assert!(!is_newer_version("0.2.5", "0.2.6"));
        // Malformed parts never panic; they compare as zero.
        assert!(is_newer_version("0.3.0", "garbage"));
    }

    #[test]
    fn github_tags_with_v_prefix_compare_like_manifest_versions() {
        assert!(is_newer_version("v0.2.7", "0.2.6"));
        assert!(!is_newer_version("0.2.6", "v0.2.7"));
        assert_eq!(
            compare_release_versions("v0.2.7", "0.2.7"),
            CmpOrdering::Equal
        );
        assert_eq!(normalize_version("  v0.2.7 "), "0.2.7");
        assert_eq!(normalize_version("version0.2.7"), "version0.2.7");
    }

    #[test]
    fn select_github_asset_maps_the_release_onto_the_manifest_record() {
        let release: GitHubRelease = serde_json::from_str(
            r#"{
                "tag_name": "v0.2.7",
                "assets": [
                    {"name": "manifest.json", "browser_download_url": "https://example.invalid/m/manifest.json", "size": 300},
                    {"name": "desktop-0.2.7-win7-x86.exe", "browser_download_url": "https://example.invalid/m/pkg.exe", "size": 99}
                ]
            }"#,
        )
        .unwrap();
        let manifest_text = r#"{
            "releases": [
                {"version": "0.2.7", "assets": [
                    {"client": "desktop", "os": "windows", "arch": "x86",
                     "file": "desktop-0.2.7-win7-x86.exe", "sha256": "abc", "size": 99}
                ]}
            ]
        }"#;
        let asset = select_github_asset(manifest_text, &release, "desktop", "windows", "x86")
            .unwrap()
            .expect("asset for desktop/windows/x86");
        assert_eq!(asset.url, "https://example.invalid/m/pkg.exe");
        assert_eq!(asset.sha256, "abc");
        assert!(
            asset.version == "0.2.7" || asset.version.is_empty(),
            "manifest version wins when present"
        );
        // A package listed in the manifest but missing from the release is a
        // publishing error, not an empty result.
        let broken = select_github_asset(
            r#"{"releases":[{"version":"0.2.7","assets":[{"client":"desktop","os":"windows","arch":"x86","file":"missing.exe","sha256":"x","size":1}]}]}"#,
            &release,
            "desktop",
            "windows",
            "x86",
        )
        .unwrap_err();
        assert!(format!("{broken:#}").contains("missing.exe"));
        assert!(
            select_github_asset(manifest_text, &release, "cli", "windows", "x86")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn select_asset_picks_newest_flattened_entry() {
        let manifest = manifest();
        let asset = manifest
            .select_asset("desktop", "windows", "x86")
            .expect("asset for desktop/windows/x86");
        assert_eq!(asset.version, "0.2.10");
        assert_eq!(asset.file, "desktop-0.2.10-win7-x86.exe");
        assert_eq!(asset.published_at, None);
        assert!(manifest.select_asset("desktop", "linux", "x86").is_none());
        assert!(manifest
            .select_asset("cli", "windows", "x86")
            .is_some_and(|asset| asset.version == "0.2.6"));
    }

    #[test]
    fn one_manifest_serves_every_update_target() {
        let manifest = manifest();
        for (client, file) in [
            ("desktop", "desktop-0.2.10-win7-x86.exe"),
            ("cli", "cli-0.2.6-win7-x86.exe"),
            ("supplement", "supplement-0.2.6-win7-x86.zip"),
        ] {
            let asset = manifest
                .select_asset(client, "windows", "x86")
                .unwrap_or_else(|| panic!("asset for {client}"));
            assert_eq!(asset.file, file);
        }
        // Unknown targets select nothing, mirroring the endpoint's 404 path.
        assert!(manifest.select_asset("agent", "windows", "x86").is_none());
    }

    #[test]
    fn file_names_must_stay_inside_the_release_directory() {
        assert!(update_file_name_is_safe("desktop-0.2.6-win7-x86.exe"));
        assert!(update_file_name_is_safe("wunder_0.2.6(1).exe"));
        assert!(!update_file_name_is_safe(""));
        assert!(!update_file_name_is_safe(".hidden.exe"));
        assert!(!update_file_name_is_safe("a/b.exe"));
        assert!(!update_file_name_is_safe("a\\b.exe"));
        assert!(!update_file_name_is_safe("..\\secrets.yaml"));
        assert!(!update_file_name_is_safe("config/../wunder.yaml"));
        assert!(!update_file_name_is_safe("pkg exe.exe"));
    }

    #[test]
    fn android_release_reads_the_asset_digest_and_version_from_the_apk_name() {
        let releases: Vec<GitHubRelease> = serde_json::from_str(
            r#"[
                {
                    "tag_name": "nightly",
                    "body": "nightly notes",
                    "html_url": "https://example.invalid/releases/nightly",
                    "published_at": "2026-11-08T01:02:03Z",
                    "prerelease": true,
                    "assets": [
                        {"name": "wunder-android-0.4.0-arm64.apk",
                         "browser_download_url": "https://example.invalid/a-0.4.0.apk",
                         "size": 10, "digest": "sha256:aaaaaaaa"},
                        {"name": "wunder-android-0.4.1-arm64.apk",
                         "browser_download_url": "https://example.invalid/a-0.4.1.apk",
                         "size": 99, "digest": "sha256:bbbbbbbb"},
                        {"name": "wunder-android-0.4.2-x64.apk",
                         "browser_download_url": "https://example.invalid/a-x64.apk",
                         "size": 5}
                    ]
                },
                {
                    "tag_name": "android-runtime",
                    "assets": [
                        {"name": "wunder-android-runtime-arm64.tar.gz",
                         "browser_download_url": "https://example.invalid/runtime.tar.gz",
                         "size": 20}
                    ]
                }
            ]"#,
        )
        .unwrap();
        let release = select_github_android_release(&releases, "arm64").expect("Android release");
        assert_eq!(release.version, "0.4.1");
        assert_eq!(release.url, "https://example.invalid/a-0.4.1.apk");
        assert_eq!(release.sha256, "bbbbbbbb");
        assert_eq!(release.size, 99);
        assert_eq!(release.notes, "nightly notes");
        assert!(release.published_at_ms > 0);
        assert!(select_github_android_release(&releases, "x64").is_some());
        assert!(select_github_android_release(&releases, "arm").is_none());
    }

    #[test]
    fn android_release_ignores_drafts() {
        let releases: Vec<GitHubRelease> = serde_json::from_str(
            r#"[{
                "tag_name": "draft",
                "draft": true,
                "assets": [{
                    "name": "wunder-android-0.9.0-arm64.apk",
                    "browser_download_url": "https://example.invalid/draft.apk",
                    "size": 1
                }]
            }]"#,
        )
        .unwrap();
        assert!(select_github_android_release(&releases, "arm64").is_none());
    }
}
