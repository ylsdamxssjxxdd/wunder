//! Android APK update transport.
//!
//! The Android shell cannot replace its own APK the way the desktop executable
//! rotates beside itself: the system package installer owns that step. This
//! module only provides the network half — check the public GitHub releases for
//! a matching arm64/x64 APK and stream it to the app cache with on-the-fly
//! sha256 verification. The Android crate then hands the verified file to the
//! installer through JNI.
//!
//! The check uses the same public repository as the desktop GitHub source but
//! does not require a `manifest.json` release asset: the APK file name carries
//! the version, and GitHub publishes a per-asset sha256 digest. That lets the
//! moving `nightly` release carry updates without a separate manifest.

use super::NativeDesktop;
use anyhow::{bail, Result};
use std::path::Path;
use std::sync::atomic::AtomicU64;
use std::sync::OnceLock;
use wunder_server::cloud::update::{
    AndroidRelease, UpdateAsset, UpdateSourceClient,
};

static SOURCE_CLIENT: OnceLock<UpdateSourceClient> = OnceLock::new();

fn source_client() -> &'static UpdateSourceClient {
    SOURCE_CLIENT.get_or_init(UpdateSourceClient::new)
}

impl NativeDesktop {
    /// Newest public Android APK for the device's release arch label
    /// (`arm64` / `x64`). Blocking; the Android engine calls it from a worker
    /// thread so the UI tick is never behind a network round-trip.
    pub fn check_android_update(
        &self,
        repo: &str,
        arch_label: &str,
    ) -> Result<Option<AndroidRelease>> {
        self.runtime
            .block_on(source_client().fetch_github_android(repo, arch_label))
    }

    /// Newest Android APK from the signed-in cloud server. Cloud manifests use
    /// the Rust target arch (`aarch64` / `x86_64`), matching the desktop
    /// client's own cloud queries.
    pub fn check_android_update_cloud(&self, arch: &str) -> Result<Option<AndroidRelease>> {
        let cloud = wunder_server::cloud::shared();
        if cloud.session().is_none() {
            bail!("请先登录云端账号");
        }
        let asset = self
            .runtime
            .block_on(cloud.fetch_latest_update("android", "android", arch))?;
        Ok(asset.map(AndroidRelease::from_update_asset))
    }

    /// Stream one APK to `dest`, hashing while writing. Returns the actual
    /// sha256. The caller compares sizes/checksums and then starts the system
    /// installer; this function never launches or replaces anything.
    pub fn download_android_update(
        &self,
        release: &AndroidRelease,
        dest: &Path,
        progress: &AtomicU64,
    ) -> Result<String> {
        if release.url.is_empty() {
            bail!("更新包缺少下载地址");
        }
        let asset = UpdateAsset {
            url: release.url.clone(),
            size: release.size,
            sha256: release.sha256.clone(),
            ..Default::default()
        };
        let (written, sha256) = self
            .runtime
            .block_on(source_client().download_asset(&asset, dest, progress))?;
        verify_download(release, written, &sha256)?;
        Ok(sha256)
    }

    /// Stream an APK from the logged-in cloud server (authenticated download
    /// endpoint). Same integrity contract as the public source.
    pub fn download_android_update_cloud(
        &self,
        release: &AndroidRelease,
        dest: &Path,
        progress: &AtomicU64,
    ) -> Result<String> {
        if release.file.is_empty() {
            bail!("云端更新包缺少文件名");
        }
        let cloud = wunder_server::cloud::shared();
        if cloud.session().is_none() {
            bail!("请先登录云端账号");
        }
        let (written, sha256) = self.runtime.block_on(
            cloud.download_update_file(&release.file, dest, progress),
        )?;
        verify_download(release, written, &sha256)?;
        Ok(sha256)
    }
}

fn verify_download(release: &AndroidRelease, written: u64, sha256: &str) -> Result<()> {
    if release.size > 0 && written != release.size {
        bail!(
            "更新包大小不一致：期望 {} 字节，实际 {written} 字节",
            release.size
        );
    }
    if !release.sha256.is_empty() && !sha256.eq_ignore_ascii_case(&release.sha256) {
        bail!("更新包校验失败：sha256 不一致");
    }
    Ok(())
}
