//! Online self-update for the desktop installation.
//!
//! Flow: check (against the logged-in cloud server or public GitHub releases)
//! → download into a staging directory with on-the-fly sha256 → apply, which
//! dispatches per target: the desktop exe swaps itself by rename (Windows
//! allows renaming a running executable) and relaunches; the CLI binary
//! beside it rotates the same way but takes effect on its next launch; the
//! supplement archive feeds the existing import pipeline (extract + merge,
//! hot tool-env refresh, no restart). Every step is synchronous and blocking:
//! the UI layer owns threading, exactly like the settings and cloud façades.
//!
//! Targets share one manifest schema: each is just a `client` value
//! (`desktop` / `cli` / `supplement`) with its own os/arch assets.
//!
//! Recovery rules:
//! - the previous binary stays as `<exe>.old` until the next successful start
//!   removes it, so an interrupted rotation can be restored by hand;
//! - a downloaded package survives restarts (staging `meta.json`) and is
//!   discarded once the running version has caught up with it.

use super::NativeDesktop;
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use wunder_server::cloud::update::{is_newer_version, UpdateSourceClient};

/// Environment flag handed to the relaunched binary: it retries the
/// single-instance claim briefly while the old process is still exiting,
/// instead of immediately treating the handover as a duplicate launch.
pub const RELAUNCH_ENV: &str = "WUNDER_UPDATE_RELAUNCH";

/// This crate tracks the workspace version, which `sync_app_version.py`
/// keeps aligned with `config/app_version.json` — the same number the cloud
/// device registry receives at login.
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Update targets selectable in the settings card. Each maps to a `client`
/// value in the release manifest (`desktop` matches the cloud device
/// registry's client value); os/arch come from the build target constants.
pub const TARGET_DESKTOP: &str = "desktop";
pub const TARGET_CLI: &str = "cli";
pub const TARGET_SUPPLEMENT: &str = "supplement";

/// The CLI binary rides in the same program directory as the desktop exe on
/// every supported layout (portable root `bin/`).
fn cli_binary_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let name = if cfg!(windows) { "wunder-cli.exe" } else { "wunder-cli" };
    Some(dir.join(name))
}

/// `wunder-cli --version` prints `<bin> <version>`; parse the version-looking
/// token. Bounded wait: a hung child is killed at the deadline instead of
/// stalling the check worker.
fn probe_cli_version(path: &Path) -> Option<String> {
    use std::process::{Command, Stdio};
    #[allow(unused_mut)]
    let mut command = Command::new(path);
    command
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = command.spawn().ok()?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let output = loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let output = child.wait_with_output().ok()?;
                break status.success().then_some(output);
            }
            Ok(None) if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(25)),
            Err(_) => return None,
        }
    };
    parse_cli_version_output(&String::from_utf8_lossy(&output?.stdout))
}

/// Take the last whitespace-separated token that looks like a version
/// (`0.2.6`, `v0.2.7-rc1`); prerelease letters are allowed, anything not
/// starting with a number fails the probe into "unknown".
fn parse_cli_version_output(text: &str) -> Option<String> {
    text.split_whitespace()
        .rev()
        .find(|token| {
            let stripped = token.trim_start_matches(|c| c == 'v' || c == 'V');
            let mut chars = stripped.chars();
            chars.next().is_some_and(|c| c.is_ascii_digit())
                && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'))
        })
        .map(str::to_string)
}

/// The publishing convention stamps every supplement package with an
/// `opt/supplement.json` marker; it rides the regular import into the shared
/// home and is the local version source. Packs without it read as unknown.
fn read_supplement_version(wunder_home: &Path) -> Option<String> {
    let text = fs::read_to_string(wunder_home.join("opt/supplement.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value
        .get("version")?
        .as_str()
        .map(str::trim)
        .filter(|version| !version.is_empty())
        .map(str::to_string)
}

fn normalize_target(target: &str) -> &'static str {
    match target.trim() {
        TARGET_CLI => TARGET_CLI,
        TARGET_SUPPLEMENT => TARGET_SUPPLEMENT,
        _ => TARGET_DESKTOP,
    }
}

/// GitHub 更新源仓库（owner/name）。构建时通过 `WUNDER_UPDATE_GITHUB_REPO`
/// 注入；留空表示未配置，GitHub 源在检查时给出明确提示而不是静默失败。
pub const UPDATE_GITHUB_REPO: &str = match option_env!("WUNDER_UPDATE_GITHUB_REPO") {
    Some(repo) if !repo.is_empty() => repo,
    _ => "",
};

/// Public-source HTTP client, created on first use and shared thereafter.
static SOURCE_CLIENT: std::sync::OnceLock<UpdateSourceClient> = std::sync::OnceLock::new();

fn source_client() -> &'static UpdateSourceClient {
    SOURCE_CLIENT.get_or_init(UpdateSourceClient::new)
}

const STAGING_META: &str = "meta.json";
const STAGING_PART: &str = "package.part";
const COPY_BUFFER_BYTES: usize = 256 * 1024;

#[derive(Debug, Default)]
pub(super) struct UpdateState {
    /// `idle | checking | up_to_date | available | downloading | staged |
    /// applying | applied | failed`
    phase: String,
    /// Target of the current offering (`desktop` / `cli` / `supplement`).
    target: String,
    /// Locally detected version of the checked target; empty means unknown
    /// (no CLI installed, or a supplement pack without the version marker).
    local_version: String,
    latest_version: String,
    notes: String,
    mandatory: bool,
    /// Manifest metadata of the offering being downloaded.
    file: String,
    sha256: String,
    size: u64,
    /// Absolute download URL for public sources (GitHub); empty keeps the
    /// authenticated cloud endpoint. Decides the download transport together
    /// with the phase, so a check->download sequence never mixes sources.
    url: String,
    /// Shared with the download worker so `update_view` observes progress
    /// without holding the state lock.
    downloaded: Arc<AtomicU64>,
    error: String,
    /// Downloaded package that survives restarts until applied or superseded.
    staged_path: Option<PathBuf>,
    staged_target: String,
    staged_version: String,
    staged_sha256: Option<String>,
}

impl UpdateState {
    pub(super) fn new() -> Self {
        Self {
            phase: "idle".to_string(),
            target: TARGET_DESKTOP.to_string(),
            downloaded: Arc::new(AtomicU64::new(0)),
            ..Default::default()
        }
    }

    /// Record a check result. A staged package always wins over an equal or
    /// older server offering: what is already on disk is at least as good.
    /// Staged packages only count for their own target. A target whose local
    /// version is unknown (empty) always accepts the remote offering, because
    /// "already up to date" cannot be proven.
    fn finish_check(&mut self, outcome: Result<Option<wunder_server::cloud::update::UpdateAsset>>) {
        match outcome {
            Err(error) => {
                self.phase = "failed".into();
                self.error = format!("{error:#}");
            }
            Ok(latest) => {
                let staged_valid = self
                    .staged_path
                    .as_ref()
                    .is_some_and(|path| path.is_file())
                    && self.staged_target == self.target;
                let staged_newest = staged_valid
                    && !latest
                        .as_ref()
                        .is_some_and(|asset| is_newer_version(&asset.version, &self.staged_version));
                let candidate = latest
                    .clone()
                    .filter(|asset| {
                        self.local_version.is_empty()
                            || is_newer_version(&asset.version, &self.local_version)
                    });
                if staged_newest {
                    self.phase = "staged".into();
                } else if let Some(asset) = candidate {
                    self.phase = "available".into();
                    self.latest_version = asset.version.clone();
                    self.notes = asset.notes.clone();
                    self.mandatory = asset.mandatory;
                    self.file = asset.file.clone();
                    self.sha256 = asset.sha256.clone();
                    self.size = asset.size;
                    self.url = asset.url.clone();
                    self.downloaded.store(0, Ordering::Relaxed);
                } else {
                    self.phase = "up_to_date".into();
                    self.latest_version = latest.map(|asset| asset.version).unwrap_or_default();
                }
            }
        }
    }
}

/// Secret-free UI projection. Progress is preformatted so the Slint side never
/// formats numbers.
#[derive(Clone, Debug, Default)]
pub struct UpdateStateView {
    pub phase: String,
    pub current_version: String,
    /// Target the state machine currently talks about.
    pub target: String,
    /// Locally detected version of that target; empty when unknown.
    pub local_version: String,
    pub latest_version: String,
    pub notes: String,
    pub mandatory: bool,
    /// 0..1 while downloading; 0 when the total size is unknown.
    pub progress: f32,
    pub progress_text: String,
    pub error: String,
}

/// What the staging directory remembers across restarts.
#[derive(Serialize, Deserialize)]
struct StagingMeta {
    version: String,
    file: String,
    sha256: String,
    #[serde(default)]
    notes: String,
    #[serde(default)]
    mandatory: bool,
    /// Metas written before multi-target support were always desktop
    /// packages; the default keeps them restorable.
    #[serde(default)]
    target: String,
}

impl StagingMeta {
    fn target(&self) -> &str {
        if self.target.is_empty() {
            TARGET_DESKTOP
        } else {
            &self.target
        }
    }
}

fn staging_dir(wunder_home: &Path) -> PathBuf {
    wunder_home.join("updates").join("staging")
}

fn format_megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
}

impl NativeDesktop {
    /// Snapshot for UI binding. Cheap: one short mutex hold, no I/O.
    pub fn update_view(&self) -> UpdateStateView {
        let state = self.lock_update();
        let downloaded = state.downloaded.load(Ordering::Relaxed);
        let (progress, progress_text) = if state.phase == "downloading" && state.size > 0 {
            let ratio = (downloaded as f64 / state.size as f64).clamp(0.0, 1.0);
            (
                ratio as f32,
                format!(
                    "{} / {} ({:.0}%)",
                    format_megabytes(downloaded),
                    format_megabytes(state.size),
                    ratio * 100.0
                ),
            )
        } else if state.phase == "downloading" {
            (0.0, format_megabytes(downloaded))
        } else {
            (0.0, String::new())
        };
        let latest_version = if state.phase == "staged" && !state.staged_version.is_empty() {
            state.staged_version.clone()
        } else {
            state.latest_version.clone()
        };
        UpdateStateView {
            phase: state.phase.clone(),
            current_version: CURRENT_VERSION.to_string(),
            target: state.target.clone(),
            local_version: state.local_version.clone(),
            latest_version,
            notes: state.notes.clone(),
            mandatory: state.mandatory,
            progress,
            progress_text,
            error: state.error.clone(),
        }
    }

    /// Ask the selected source for the newest release of one target. Blocks;
    /// run on a worker thread. The cloud source requires a live session; the
    /// GitHub source is public but only exists when the binary was built with
    /// a repository configured. The target's local version is detected here:
    /// compiled-in for the desktop itself, `--version` probe for the CLI,
    /// version marker for the supplement pack.
    pub fn update_check(&self, source: &str, target: &str) -> Result<()> {
        let target = normalize_target(target);
        // Mark in-flight before any network work so a double click cannot
        // stack two checks; the UI poller observes "checking" right away.
        {
            let mut state = self.lock_update();
            if matches!(state.phase.as_str(), "checking" | "downloading" | "applying") {
                bail!("更新操作正在进行中");
            }
            state.phase = "checking".into();
            state.error.clear();
            state.target = target.into();
        }
        let local_version = match target {
            TARGET_CLI => cli_binary_path()
                .filter(|path| path.is_file())
                .and_then(|path| probe_cli_version(&path))
                .unwrap_or_default(),
            TARGET_SUPPLEMENT => {
                read_supplement_version(&self.desktop.wunder_home).unwrap_or_default()
            }
            _ => CURRENT_VERSION.to_string(),
        };
        let outcome = match source.trim() {
            "github" => {
                if UPDATE_GITHUB_REPO.is_empty() {
                    let mut state = self.lock_update();
                    state.phase = "failed".into();
                    state.error = "GitHub 更新源未配置，请使用云端账号检查更新".into();
                    bail!(state.error.clone());
                }
                self.runtime.block_on(source_client().fetch_github_latest(
                    UPDATE_GITHUB_REPO,
                    target,
                    std::env::consts::OS,
                    std::env::consts::ARCH,
                ))
            }
            _ => {
                if wunder_server::cloud::shared().session().is_none() {
                    let mut state = self.lock_update();
                    state.phase = "failed".into();
                    state.error = "请先登录云端账号".into();
                    bail!(state.error.clone());
                }
                self.runtime.block_on(
                    wunder_server::cloud::shared().fetch_latest_update(
                        target,
                        std::env::consts::OS,
                        std::env::consts::ARCH,
                    ),
                )
            }
        };
        let mut state = self.lock_update();
        state.local_version = local_version;
        state.finish_check(outcome);
        match state.phase.as_str() {
            "failed" => Err(anyhow!(state.error.clone())),
            _ => Ok(()),
        }
    }

    /// Download the offered package into staging, verifying sha256 while
    /// streaming. Blocks; run on a worker thread. Progress is observable
    /// through [`Self::update_view`] while this runs.
    pub fn update_download(&self) -> Result<()> {
        let (file, sha256, size, offering_version, notes, mandatory, url, target, progress) = {
            let mut state = self.lock_update();
            if state.phase != "available" {
                bail!("当前状态不允许下载（{}）", state.phase);
            }
            if state.file.is_empty() || state.sha256.is_empty() {
                bail!("更新清单缺少下载信息");
            }
            state.phase = "downloading".into();
            state.error.clear();
            state.downloaded.store(0, Ordering::Relaxed);
            (
                state.file.clone(),
                state.sha256.clone(),
                state.size,
                state.latest_version.clone(),
                state.notes.clone(),
                state.mandatory,
                state.url.clone(),
                state.target.clone(),
                state.downloaded.clone(),
            )
        };
        let staging = staging_dir(&self.desktop.wunder_home);
        let part = staging.join(STAGING_PART);
        // Two transports, one integrity contract: public sources stream from
        // the asset URL; the cloud channel keeps its authenticated endpoint
        // with 401 refresh. Both land in the same staging file.
        let download = self.runtime.block_on(async {
            fs::create_dir_all(&staging)?;
            if !url.is_empty() {
                source_client()
                    .download_asset(
                        &wunder_server::cloud::update::UpdateAsset {
                            file: file.clone(),
                            url,
                            ..Default::default()
                        },
                        &part,
                        &progress,
                    )
                    .await
            } else {
                wunder_server::cloud::shared()
                    .download_update_file(&file, &part, &progress)
                    .await
            }
        });
        let verified = match download {
            Ok((written, actual_sha)) if !actual_sha.eq_ignore_ascii_case(&sha256) => {
                let _ = written;
                let _ = fs::remove_file(&part);
                Err(anyhow!("更新包校验失败：内容与清单不符"))
            }
            Ok((written, _actual_sha)) if size > 0 && written != size => {
                let _ = fs::remove_file(&part);
                Err(anyhow!("更新包大小不符：预期 {size}，实际 {written}"))
            }
            Ok((_, actual_sha)) => Ok(actual_sha),
            Err(error) => {
                let _ = fs::remove_file(&part);
                Err(error)
            }
        };
        let actual_sha = match verified {
            Ok(actual_sha) => actual_sha,
            Err(error) => {
                let mut state = self.lock_update();
                state.phase = "failed".into();
                state.error = format!("{error:#}");
                return Err(anyhow!(state.error.clone()));
            }
        };
        let final_path = staging.join(&file);
        if let Err(error) = fs::rename(&part, &final_path) {
            let mut state = self.lock_update();
            state.phase = "failed".into();
            state.error = format!("无法落盘更新包：{error}");
            return Err(anyhow!(state.error.clone()));
        }
        let meta = StagingMeta {
            version: offering_version,
            file,
            sha256: actual_sha,
            notes,
            mandatory,
            target: target.clone(),
        };
        if let Err(error) = fs::write(staging.join(STAGING_META), serde_json::to_vec(&meta)?) {
            let mut state = self.lock_update();
            state.phase = "failed".into();
            state.error = format!("无法写入更新元数据：{error}");
            return Err(anyhow!(state.error.clone()));
        }
        let mut state = self.lock_update();
        state.staged_path = Some(final_path);
        state.staged_target = target;
        state.staged_version = meta.version;
        state.staged_sha256 = Some(meta.sha256);
        state.phase = "staged".into();
        Ok(())
    }

    /// Apply the staged package, dispatched per target: the desktop exe
    /// relaunches (caller exits), the CLI rotates beside it and takes effect
    /// on its next launch, the supplement feeds the import pipeline and takes
    /// effect immediately.
    pub fn update_apply(&self) -> Result<()> {
        let (staged_path, staged_sha, staged_target) = {
            let mut state = self.lock_update();
            let staged_ready = state
                .staged_path
                .as_ref()
                .is_some_and(|path| path.is_file());
            // `failed` stays applicable when a package is on disk: a failed
            // relaunch attempt must not brick the staged package behind a
            // restart.
            if !(matches!(state.phase.as_str(), "staged" | "failed") && staged_ready) {
                bail!("没有已下载完成的更新包");
            }
            state.phase = "applying".into();
            state.error.clear();
            (
                state.staged_path.clone().expect("checked above"),
                state.staged_sha256.clone().unwrap_or_default(),
                state.staged_target.clone(),
            )
        };
        let applied_note = match self.apply_staged_by_target(&staged_path, &staged_sha, &staged_target) {
            Ok(note) => note,
            Err(error) => {
                let mut state = self.lock_update();
                state.phase = "failed".into();
                state.error = format!("{error:#}");
                return Err(anyhow!(state.error.clone()));
            }
        };
        let mut state = self.lock_update();
        if staged_target == TARGET_DESKTOP {
            // The staged package is consumed by the relaunched binary's
            // `update_cleanup`; nothing to record — the caller exits now.
            return Ok(());
        }
        state.staged_path = None;
        state.staged_target.clear();
        state.staged_version.clear();
        state.staged_sha256 = None;
        let _ = fs::remove_file(&staged_path);
        let staging = staging_dir(&self.desktop.wunder_home);
        let _ = fs::remove_file(staging.join(STAGING_META));
        state.notes = applied_note.unwrap_or_default();
        state.phase = "applied".into();
        Ok(())
    }

    /// Per-target application of one staged package. `Ok(Some(summary))`
    /// carries a user-facing line for targets that finish in place.
    fn apply_staged_by_target(
        &self,
        staged: &Path,
        staged_sha: &str,
        target: &str,
    ) -> Result<Option<String>> {
        match target {
            TARGET_CLI => {
                self.apply_staged_cli(staged, staged_sha)?;
                Ok(None)
            }
            TARGET_SUPPLEMENT => Ok(Some(self.apply_staged_supplement(staged)?.summary_line())),
            _ => {
                self.apply_staged_package(staged, staged_sha)?;
                Ok(None)
            }
        }
    }

    /// Startup hygiene: drop rotation leftovers, and either restore a staged
    /// package newer than this binary or clear the staging directory.
    pub fn update_cleanup(&self) {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                if let Some(name) = exe.file_name() {
                    let _ = fs::remove_file(dir.join(format!("{}.old", name.to_string_lossy())));
                    let _ = fs::remove_file(dir.join(format!("{}.new", name.to_string_lossy())));
                }
                if let Some(name) = cli_binary_path().and_then(|cli| cli.file_name().map(|n| n.to_string_lossy().into_owned())) {
                    let _ = fs::remove_file(dir.join(format!("{name}.old")));
                    let _ = fs::remove_file(dir.join(format!("{name}.new")));
                }
            }
        }
        let staging = staging_dir(&self.desktop.wunder_home);
        let meta_path = staging.join(STAGING_META);
        let meta: Option<StagingMeta> = fs::read_to_string(&meta_path)
            .ok()
            .and_then(|text| serde_json::from_str::<StagingMeta>(&text).ok())
            .filter(|meta: &StagingMeta| staging_restores(meta, &staging.join(&meta.file)));
        let mut state = self.lock_update();
        match meta {
            Some(meta) => {
                let target = meta.target().to_string();
                state.staged_path = Some(staging.join(&meta.file));
                state.staged_target = target.clone();
                state.staged_version = meta.version.clone();
                state.staged_sha256 = Some(meta.sha256);
                state.target = target;
                state.latest_version = meta.version;
                state.notes = meta.notes;
                state.mandatory = meta.mandatory;
                state.phase = "staged".into();
            }
            None => {
                let _ = fs::remove_file(&meta_path);
                if let Ok(entries) = fs::read_dir(&staging) {
                    for entry in entries.flatten() {
                        if entry.path().is_file() {
                            let _ = fs::remove_file(entry.path());
                        }
                    }
                }
            }
        }
    }

    /// The actual rename rotation plus relaunch. Same-volume renames only:
    /// the staged package is first copied next to the exe (and re-hashed
    /// there) so the rotation never crosses a filesystem boundary.
    fn apply_staged_package(&self, staged: &Path, staged_sha: &str) -> Result<()> {
        let exe = std::env::current_exe().context("无法定位当前程序")?;
        let exe_dir = exe.parent().context("无法定位程序目录")?.to_path_buf();
        let exe_name = exe
            .file_name()
            .context("无法识别程序文件名")?
            .to_string_lossy()
            .into_owned();
        let new_path = exe_dir.join(format!("{exe_name}.new"));
        let old_path = exe_dir.join(format!("{exe_name}.old"));

        copy_and_verify(staged, &new_path, staged_sha).context("更新包落盘校验失败")?;
        if old_path.exists() {
            let _ = fs::remove_file(&old_path);
        }
        if let Err(error) = fs::rename(&exe, &old_path) {
            let _ = fs::remove_file(&new_path);
            bail!("无法重命名当前程序（可能被占用）：{error}");
        }
        if let Err(error) = fs::rename(&new_path, &exe) {
            let _ = fs::rename(&old_path, &exe);
            bail!("无法放置新版本程序：{error}");
        }
        let spawn = std::process::Command::new(&exe)
            .env(RELAUNCH_ENV, "1")
            .current_dir(&exe_dir)
            .spawn();
        if let Err(error) = spawn {
            let _ = fs::remove_file(&exe);
            let _ = fs::rename(&old_path, &exe);
            bail!("无法启动新版本程序：{error}");
        }
        // The staged package is consumed; the relaunched binary clears the
        // staging directory on its next startup (`update_cleanup`).
        Ok(())
    }

    /// Same-volume rotation for the CLI binary beside this executable; no
    /// relaunch: a running TUI keeps its old file open, the next launch picks
    /// up the new one. A missing CLI installs it fresh.
    fn apply_staged_cli(&self, staged: &Path, staged_sha: &str) -> Result<()> {
        let target = cli_binary_path().context("无法定位程序目录")?;
        let new_path = sibling_path(&target, ".new");
        let old_path = sibling_path(&target, ".old");
        copy_and_verify(staged, &new_path, staged_sha).context("更新包落盘校验失败")?;
        if target.is_file() {
            if old_path.exists() {
                let _ = fs::remove_file(&old_path);
            }
            if let Err(error) = fs::rename(&target, &old_path) {
                let _ = fs::remove_file(&new_path);
                bail!("无法重命名当前舵机（可能被占用）：{error}");
            }
        }
        if let Err(error) = fs::rename(&new_path, &target) {
            if !target.is_file() {
                let _ = fs::rename(&old_path, &target);
            }
            bail!("无法放置新版本舵机：{error}");
        }
        Ok(())
    }

    /// The supplement archive goes through the regular import pipeline:
    /// validated streaming extraction, overwrite-merge over the shared home
    /// and an in-process tool environment refresh — no restart needed. The
    /// staged file keeps its manifest name so the archive kind stays
    /// detectable.
    fn apply_staged_supplement(&self, staged: &Path) -> Result<super::SupplementImportReport> {
        self.import_supplement(&staged.to_string_lossy())
    }

    fn lock_update(&self) -> std::sync::MutexGuard<'_, UpdateState> {
        self.update
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// `<path><suffix>` next to the original (rotation helpers share this shape).
fn sibling_path(path: &Path, suffix: &str) -> PathBuf {
    PathBuf::from(format!("{}{suffix}", path.to_string_lossy()))
}

/// Stream-copy `from` to `to` while hashing; when `expected_sha` is non-empty
/// the copy is rejected (and removed) unless the digests match.
fn copy_and_verify(from: &Path, to: &Path, expected_sha: &str) -> Result<()> {
    let mut hasher = Sha256::new();
    let mut source = fs::File::open(from).with_context(|| format!("无法读取更新包 {from:?}"))?;
    let mut dest = fs::File::create(to)?;
    let mut buffer = vec![0u8; COPY_BUFFER_BYTES];
    loop {
        let read = source.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        dest.write_all(&buffer[..read])?;
    }
    dest.flush()?;
    drop(dest);
    let actual = format!("{:x}", hasher.finalize());
    if !expected_sha.is_empty() && !actual.eq_ignore_ascii_case(expected_sha) {
        let _ = fs::remove_file(to);
        bail!("内容与清单不符");
    }
    Ok(())
}

/// A staged package restores at startup only when its file survived and,
/// for desktop packages, it is still newer than this binary; cli and
/// supplement packages have no compile-time baseline and always restore.
fn staging_restores(meta: &StagingMeta, package: &Path) -> bool {
    package.is_file()
        && (meta.target() != TARGET_DESKTOP
            || is_newer_version(&meta.version, CURRENT_VERSION))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_fixture(path: &Path, payload: &[u8]) -> String {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).unwrap();
        }
        let mut file = fs::File::create(path).unwrap();
        file.write_all(payload).unwrap();
        let mut hasher = Sha256::new();
        hasher.update(payload);
        format!("{:x}", hasher.finalize())
    }

    #[test]
    fn copy_and_verify_accepts_matching_and_rejects_tampered_content() {
        let dir = std::env::temp_dir().join(format!("wunder-update-test-{}", std::process::id()));
        let source = dir.join("source.bin");
        let dest = dir.join("dest.bin");
        let sha = write_fixture(&source, b"package-bytes");
        copy_and_verify(&source, &dest, &sha).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"package-bytes");
        copy_and_verify(&source, &dest, "").unwrap();
        let error = copy_and_verify(&source, &dest, "deadbeef").unwrap_err();
        assert!(format!("{error:#}").contains("内容与清单不符"));
        assert!(!dest.exists(), "tampered copy must be removed");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn megabyte_formatting_stays_stable_for_small_and_large_counts() {
        assert_eq!(format_megabytes(0), "0.0 MB");
        assert_eq!(format_megabytes(512 * 1024), "0.5 MB");
        assert_eq!(format_megabytes(48 * 1024 * 1024), "48.0 MB");
    }

    #[test]
    fn cli_version_probe_parses_clap_output_and_rejects_noise() {
        assert_eq!(
            parse_cli_version_output("wunder-cli 0.2.6\n").as_deref(),
            Some("0.2.6")
        );
        assert_eq!(
            parse_cli_version_output("wunder-cli v0.2.7-rc1").as_deref(),
            Some("v0.2.7-rc1")
        );
        assert_eq!(parse_cli_version_output(""), None);
        assert_eq!(parse_cli_version_output("wunder-cli no-version-here"), None);
        assert_eq!(
            parse_cli_version_output("wunder-cli 0.2").as_deref(),
            Some("0.2"),
            "two-part versions still parse"
        );
    }

    #[test]
    fn staging_meta_defaults_legacy_packages_to_the_desktop_target() {
        let legacy: StagingMeta =
            serde_json::from_str(r#"{"version":"0.2.6","file":"a.exe","sha256":"x"}"#).unwrap();
        assert_eq!(legacy.target(), TARGET_DESKTOP);
        let cli: StagingMeta = serde_json::from_str(
            r#"{"version":"0.2.7","file":"cli.exe","sha256":"x","target":"cli"}"#,
        )
        .unwrap();
        assert_eq!(cli.target(), TARGET_CLI);
    }

    #[test]
    fn supplement_version_marker_reads_only_valid_documents() {
        let dir = std::env::temp_dir().join(format!("wunder-update-supp-{}", std::process::id()));
        let home = dir.join("home");
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(read_supplement_version(&home), None, "missing marker");
        fs::create_dir_all(home.join("opt")).unwrap();
        fs::write(
            home.join("opt/supplement.json"),
            br#"{"version":"0.2.7","built_at":"2026-11-01"}"#,
        )
        .unwrap();
        assert_eq!(
            read_supplement_version(&home).as_deref(),
            Some("0.2.7"),
            "valid marker parses"
        );
        fs::write(home.join("opt/supplement.json"), br#"{"built_at":"x"}"#).unwrap();
        assert_eq!(read_supplement_version(&home), None, "marker without the field");
        fs::write(home.join("opt/supplement.json"), b"not json").unwrap();
        assert_eq!(read_supplement_version(&home), None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn staged_packages_restore_only_when_the_startup_gate_accepts_them() {
        let dir = std::env::temp_dir().join(format!("wunder-update-restore-{}", std::process::id()));
        let package = dir.join("pkg.bin");
        write_fixture(&package, b"staged-bytes");
        let meta = |target: &str, version: &str| -> StagingMeta {
            serde_json::from_str(&format!(
                r#"{{"version":"{version}","file":"pkg.bin","sha256":"x","target":"{target}"}}"#
            ))
            .unwrap()
        };
        assert!(
            staging_restores(&meta(TARGET_CLI, "0.0.1"), &package),
            "cli packages have no compile-time baseline"
        );
        assert!(
            staging_restores(&meta(TARGET_SUPPLEMENT, "0.0.1"), &package),
            "supplement packages have no compile-time baseline"
        );
        assert!(
            staging_restores(&meta(TARGET_DESKTOP, "999.0.0"), &package),
            "a newer desktop package restores"
        );
        assert!(
            !staging_restores(&meta(TARGET_DESKTOP, "0.0.1"), &package),
            "a desktop package the running binary already beats is consumed"
        );
        assert!(
            !staging_restores(&meta(TARGET_CLI, "0.0.1"), &dir.join("missing.bin")),
            "a vanished package file never restores"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
