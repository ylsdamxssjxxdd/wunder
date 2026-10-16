//! Supplement package import: unpack the `opt/` runtime tree (python, git,
//! rg) into the shared WUNDER_HOME and refresh the tool command environment in
//! process, so newly spawned tool commands use it without an app restart.

use super::NativeDesktop;
use crate::runtime::{
    load_desktop_settings, refresh_runtime_tool_env, resolve_effective_tool_bins,
};
use anyhow::{anyhow, bail, Context, Result};
use std::path::{Path, PathBuf};
use wunder_server::archive_extract;

/// User-facing import outcome. Paths are what tool commands will resolve to
/// right now, not what was configured. `settings` carries the refreshed tool
/// status projection so the UI updates in one round trip.
#[derive(Clone, Debug)]
pub struct SupplementImportReport {
    pub target_dir: String,
    pub extracted_files: u64,
    pub extracted_bytes: u64,
    pub python_path: String,
    pub git_path: String,
    pub rg_path: String,
    pub settings: super::DesktopSettings,
}

impl SupplementImportReport {
    pub fn summary_line(&self) -> String {
        format!(
            "{} 个文件 · Python：{}",
            self.extracted_files,
            non_empty_or(&self.python_path, "未检测到")
        )
    }
}

fn non_empty_or<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    if value.trim().is_empty() {
        fallback
    } else {
        value
    }
}

const SUPPLEMENT_ROOT: &str = "opt/";

impl NativeDesktop {
    /// Import a supplement archive (Win7 zip / Linux tar.gz carrying `opt/`).
    /// Runs on a worker thread; the UI shows progress meanwhile.
    pub fn import_supplement(&self, archive_input: &str) -> Result<SupplementImportReport> {
        let trimmed = archive_input.trim();
        if trimmed.is_empty() {
            bail!("请先选择补充包压缩包");
        }
        let archive = Path::new(trimmed);
        if !archive.is_file() {
            bail!("补充包文件不存在：{trimmed}");
        }
        let kind = archive
            .file_name()
            .and_then(|name| archive_extract::detect_archive_kind(&name.to_string_lossy()))
            .ok_or_else(|| anyhow!("请选择 zip 或 tar.gz 格式的补充包"))?;
        if kind == archive_extract::ArchiveKind::SevenZipLike {
            bail!("请使用 zip 或 tar.gz 格式的补充包");
        }

        // Installation location never chooses the data root, including when
        // an older supplement is already present beside the executable/image.
        let target_root = &self.desktop.wunder_home;
        let (files, bytes) = import_supplement_archive(archive, kind, target_root)?;

        let settings = load_desktop_settings(&self.desktop.settings_path).unwrap_or_default();
        refresh_runtime_tool_env(&settings, &self.desktop.app_dir);
        let (python, git, rg) = resolve_effective_tool_bins(&settings, &self.desktop.app_dir);
        let path_string = |path: Option<PathBuf>| {
            path.map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_default()
        };
        Ok(SupplementImportReport {
            target_dir: target_root.join("opt").to_string_lossy().into_owned(),
            extracted_files: files,
            extracted_bytes: bytes,
            python_path: path_string(python),
            git_path: path_string(git),
            rg_path: path_string(rg),
            settings: self.get_desktop_settings()?,
        })
    }
}

fn import_supplement_archive(
    archive: &Path,
    kind: archive_extract::ArchiveKind,
    home: &Path,
) -> Result<(u64, u64)> {
    let strip = plan_supplement_archive(archive, kind)?;
    std::fs::create_dir_all(home.join("config"))
        .context("无法创建共享补充包目录，请检查 WUNDER_HOME 权限")?;
    // Persistent kernel lock: concurrent instances cannot interleave upgrades.
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(home.join("config/supplement.lock"))?;
    lock.try_lock()
        .map_err(|error| anyhow!("补充包导入目录正忙或无法锁定，请稍后重试：{error}"))?;
    match kind {
        archive_extract::ArchiveKind::Zip => {
            let prefix = (!strip.is_empty()).then(|| format!("{strip}/"));
            archive_extract::extract_zip_streaming(archive, home, prefix.as_deref(), |entry| {
                entry.starts_with(SUPPLEMENT_ROOT)
            })
        }
        _ => extract_tar_supplement(archive, home, &strip),
    }
}

/// Inspect the archive entry list and decide which top-level prefix to strip:
/// packages rooted at `opt/` unpack as-is; a package wrapped in one folder
/// (e.g. `wunder-supplement/opt/...`) strips that folder. Anything without an
/// `opt/` runtime tree is rejected before touching the shared home.
fn plan_supplement_archive(archive: &Path, kind: archive_extract::ArchiveKind) -> Result<String> {
    let names = match kind {
        archive_extract::ArchiveKind::Zip => list_zip_entries(archive)?,
        _ => list_tar_entries(archive)?,
    };
    if names.is_empty() {
        bail!("补充包压缩包为空");
    }
    if names
        .iter()
        .any(|name| name == "opt" || name.starts_with(SUPPLEMENT_ROOT))
    {
        return Ok(String::new());
    }
    let first = names[0].split('/').next().unwrap_or_default().to_string();
    if !first.is_empty()
        && names
            .iter()
            .all(|name| name.as_str() == first || name.starts_with(&format!("{first}/")))
        && names
            .iter()
            .any(|name| name.starts_with(&format!("{first}/{SUPPLEMENT_ROOT}")))
    {
        return Ok(first);
    }
    bail!("压缩包中未找到 opt 运行时目录，不是有效的补充包")
}

fn list_zip_entries(archive: &Path) -> Result<Vec<String>> {
    archive_extract::list_zip_entry_names(archive).context("压缩包格式无效")
}

fn list_tar_entries(archive: &Path) -> Result<Vec<String>> {
    let output = tar_command()
        .arg("-tf")
        .arg(archive)
        .output()
        .context("系统缺少 tar 命令，请手动将补充包解压到 WUNDER_HOME 或改用 zip 格式")?;
    if !output.status.success() {
        bail!(
            "读取压缩包目录失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let mut names = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let name = line.trim().replace('\\', "/");
        let name = name.trim_end_matches('/').trim_start_matches("./");
        if name.is_empty() || name == "." {
            continue;
        }
        archive_extract::validate_archive_entry_path(name)?;
        names.push(name.to_string());
    }
    Ok(names)
}

/// tar archives cannot be entry-filtered portably, so extract into a sibling
/// temp directory, then merge `opt/` over the target (overwrite = upgrade).
fn extract_tar_supplement(archive: &Path, target_root: &Path, strip: &str) -> Result<(u64, u64)> {
    let temp = target_root.join(format!(
        ".wunder-supplement-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&temp)?;
    let result = (|| -> Result<(u64, u64)> {
        let mut command = tar_command();
        command.arg("-xf").arg(archive).arg("-C").arg(&temp);
        let status = command.status()?;
        if !status.success() {
            bail!("tar 解压失败，请确认压缩包完整");
        }
        // Inspect normalized paths, but preserve the archive's own ./ prefix:
        // stripping one tar component would remove ./ instead of its wrapper.
        let source_opt = temp.join(strip).join("opt");
        if !source_opt.is_dir() {
            bail!("补充包中未找到 opt 运行时目录，不是有效的补充包");
        }
        merge_dir(&source_opt, &target_root.join("opt"))
    })();
    let _ = std::fs::remove_dir_all(&temp);
    result
}

fn tar_command() -> std::process::Command {
    #[allow(unused_mut)]
    let mut command = std::process::Command::new("tar");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    command
}

/// Recursively copy `src` over `dst`, overwriting existing files so a repeat
/// import upgrades the runtimes in place. Returns files and bytes copied.
fn merge_dir(src: &Path, dst: &Path) -> Result<(u64, u64)> {
    merge_dir_with_boundary(src, dst, &src.canonicalize()?)
}

fn merge_dir_with_boundary(src: &Path, dst: &Path, source_root: &Path) -> Result<(u64, u64)> {
    std::fs::create_dir_all(dst)?;
    let mut files = 0u64;
    let mut bytes = 0u64;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let entry_type = entry.file_type()?;
        let target = dst.join(entry.file_name());
        if entry_type.is_dir() {
            let (sub_files, sub_bytes) =
                merge_dir_with_boundary(&entry.path(), &target, source_root)?;
            files += sub_files;
            bytes += sub_bytes;
        } else if entry_type.is_file() {
            bytes += std::fs::copy(entry.path(), &target)?;
            files += 1;
        } else if entry_type.is_symlink() {
            // Linux Python packages carry aliases such as python3 -> python3.8.
            // Copy their payload so the shared install has no staging links.
            let resolved = entry.path().canonicalize()?;
            if !resolved.starts_with(source_root) || !resolved.is_file() {
                bail!("补充包包含越界或不支持的符号链接");
            }
            bytes += std::fs::copy(resolved, &target)?;
            files += 1;
        }
    }
    Ok((files, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use zip::write::FileOptions;
    use zip::{CompressionMethod, ZipWriter};

    fn write_test_zip(path: &Path, entries: &[(&str, &str)]) {
        let file = std::fs::File::create(path).expect("create zip");
        let mut writer = ZipWriter::new(file);
        let options = FileOptions::default().compression_method(CompressionMethod::Deflated);
        for (name, content) in entries {
            writer.start_file(*name, options).expect("add entry");
            use std::io::Write;
            writer.write_all(content.as_bytes()).expect("write entry");
        }
        writer.finish().expect("finish zip");
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "wunder-supplement-test-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn plan_accepts_opt_root_and_wrapped_packages() {
        let dir = temp_dir("plan");
        let archive = dir.join("plain.zip");
        write_test_zip(
            &archive,
            &[("opt/python/python.exe", "py"), ("opt/git/bin/git", "git")],
        );
        assert_eq!(
            plan_supplement_archive(&archive, archive_extract::ArchiveKind::Zip).unwrap(),
            ""
        );

        let wrapped = dir.join("wrapped.zip");
        write_test_zip(
            &wrapped,
            &[
                ("pkg-x/opt/python/python.exe", "py"),
                ("pkg-x/opt/rg/rg.exe", "rg"),
            ],
        );
        assert_eq!(
            plan_supplement_archive(&wrapped, archive_extract::ArchiveKind::Zip).unwrap(),
            "pkg-x"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn plan_rejects_archives_without_opt_tree() {
        let dir = temp_dir("reject");
        let archive = dir.join("random.zip");
        write_test_zip(&archive, &[("readme.txt", "hi"), ("docs/guide.txt", "hi")]);
        assert!(plan_supplement_archive(&archive, archive_extract::ArchiveKind::Zip).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn extract_streaming_unwraps_prefix_and_filters_to_opt() {
        let dir = temp_dir("extract");
        let archive = dir.join("wrapped.zip");
        write_test_zip(
            &archive,
            &[
                ("pkg-x/opt/python/python.exe", "py"),
                ("pkg-x/opt/git/bin/git", "git"),
                ("pkg-x/manifest.json", "{}"),
            ],
        );
        let out = dir.join("out");
        std::fs::create_dir_all(&out).expect("create out");
        let (files, bytes) =
            archive_extract::extract_zip_streaming(&archive, &out, Some("pkg-x/"), |entry| {
                entry.starts_with(SUPPLEMENT_ROOT)
            })
            .expect("extract");
        assert_eq!(files, 2);
        assert!(out.join("opt/python/python.exe").is_file());
        assert!(out.join("opt/git/bin/git").is_file());
        assert!(!out.join("manifest.json").exists());
        assert_eq!(bytes, 5);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn extract_streaming_rejects_traversal_entries() {
        let dir = temp_dir("traversal");
        let archive = dir.join("evil.zip");
        write_test_zip(&archive, &[("../escaped.txt", "x")]);
        let out = dir.join("out");
        std::fs::create_dir_all(&out).expect("create out");
        assert!(archive_extract::extract_zip_streaming(&archive, &out, None, |_| true).is_err());
        assert!(!dir.join("escaped.txt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn merge_dir_overwrites_existing_files() {
        let dir = temp_dir("merge");
        let src = dir.join("src/opt/python");
        std::fs::create_dir_all(&src).expect("create src");
        std::fs::write(src.join("python.exe"), b"new").expect("write new");
        let dst = dir.join("dst/opt/python");
        std::fs::create_dir_all(&dst).expect("create dst");
        std::fs::write(dst.join("python.exe"), b"old").expect("write old");
        let (files, bytes) = merge_dir(&dir.join("src/opt"), &dir.join("dst/opt")).expect("merge");
        assert_eq!(files, 1);
        assert_eq!(bytes, 3);
        assert_eq!(std::fs::read(dst.join("python.exe")).expect("read"), b"new");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_installs_only_opt_under_shared_home_and_upgrades_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("profile");
        let install = dir.path().join("install");
        std::fs::create_dir_all(install.join("opt/python")).unwrap();
        std::fs::write(install.join("opt/python/python.exe"), b"legacy").unwrap();
        std::fs::create_dir_all(home.join("config")).unwrap();
        std::fs::write(home.join("config/wunder.yaml"), b"user config").unwrap();
        let archive = dir.path().join("supplement.zip");
        write_test_zip(
            &archive,
            &[
                ("bundle/opt/python/python.exe", "first"),
                ("bundle/config/wunder.yaml", "must not overwrite"),
            ],
        );
        assert_eq!(
            import_supplement_archive(&archive, archive_extract::ArchiveKind::Zip, &home).unwrap(),
            (1, 5)
        );
        assert_eq!(
            std::fs::read(home.join("opt/python/python.exe")).unwrap(),
            b"first"
        );
        assert_eq!(
            std::fs::read(home.join("config/wunder.yaml")).unwrap(),
            b"user config"
        );
        assert_eq!(
            std::fs::read(install.join("opt/python/python.exe")).unwrap(),
            b"legacy"
        );
        write_test_zip(&archive, &[("opt/python/python.exe", "second")]);
        assert_eq!(
            import_supplement_archive(&archive, archive_extract::ArchiveKind::Zip, &home).unwrap(),
            (1, 6)
        );
        assert_eq!(
            std::fs::read(home.join("opt/python/python.exe")).unwrap(),
            b"second"
        );
    }

    #[test]
    fn import_rejects_invalid_package_before_creating_shared_home() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("profile");
        let archive = dir.path().join("invalid.zip");
        write_test_zip(&archive, &[("readme.txt", "fixture")]);
        assert!(
            import_supplement_archive(&archive, archive_extract::ArchiveKind::Zip, &home).is_err()
        );
        assert!(!home.exists());
    }

    #[test]
    fn import_refuses_concurrent_upgrade() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("profile");
        std::fs::create_dir_all(home.join("config")).unwrap();
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(home.join("config/supplement.lock"))
            .unwrap();
        lock.try_lock().unwrap();
        let archive = dir.path().join("supplement.zip");
        write_test_zip(&archive, &[("opt/rg/rg.exe", "fixture")]);
        assert!(
            import_supplement_archive(&archive, archive_extract::ArchiveKind::Zip, &home).is_err()
        );
        assert!(!home.join("opt").exists());
        drop(lock);
        import_supplement_archive(&archive, archive_extract::ArchiveKind::Zip, &home).unwrap();
    }

    #[test]
    fn import_accepts_tar_with_dot_root_used_by_linux_packages() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        std::fs::create_dir_all(source.join("opt/python/bin")).unwrap();
        std::fs::write(source.join("opt/python/bin/python3"), b"fixture").unwrap();
        std::fs::write(source.join("manifest.json"), b"{}").unwrap();
        let archive = dir.path().join("supplement.tar.gz");
        assert!(tar_command()
            .arg("-czf")
            .arg(&archive)
            .arg("-C")
            .arg(&source)
            .arg(".")
            .status()
            .unwrap()
            .success());
        let home = dir.path().join("profile");
        assert_eq!(
            import_supplement_archive(&archive, archive_extract::ArchiveKind::TarLike, &home)
                .unwrap(),
            (1, 7)
        );
        assert_eq!(
            std::fs::read(home.join("opt/python/bin/python3")).unwrap(),
            b"fixture"
        );
        assert!(!home.join("manifest.json").exists());

        let wrapped = source.parent().unwrap().join("wrapped");
        std::fs::create_dir_all(wrapped.join("bundle/opt/rg/bin")).unwrap();
        std::fs::write(wrapped.join("bundle/opt/rg/bin/rg"), b"fixture").unwrap();
        let archive = dir.path().join("wrapped.tar.gz");
        assert!(tar_command()
            .arg("-czf")
            .arg(&archive)
            .arg("-C")
            .arg(&wrapped)
            .arg(".")
            .status()
            .unwrap()
            .success());
        assert_eq!(
            import_supplement_archive(&archive, archive_extract::ArchiveKind::TarLike, &home)
                .unwrap(),
            (1, 7)
        );
        assert!(home.join("opt/rg/bin/rg").is_file());
    }

    #[cfg(unix)]
    #[test]
    fn merge_keeps_python_aliases_and_rejects_external_links() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("opt/python/bin");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("python3.8"), b"fixture").unwrap();
        std::fs::set_permissions(
            source.join("python3.8"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        symlink("python3.8", source.join("python3")).unwrap();
        let out = dir.path().join("installed");
        merge_dir(&dir.path().join("opt"), &out).unwrap();
        assert_eq!(
            std::fs::read(out.join("python/bin/python3")).unwrap(),
            b"fixture"
        );
        assert_ne!(
            std::fs::metadata(out.join("python/bin/python3"))
                .unwrap()
                .permissions()
                .mode()
                & 0o111,
            0
        );
        std::fs::write(dir.path().join("outside"), b"private").unwrap();
        symlink(dir.path().join("outside"), source.join("external")).unwrap();
        assert!(merge_dir(&dir.path().join("opt"), &out).is_err());
    }
}
