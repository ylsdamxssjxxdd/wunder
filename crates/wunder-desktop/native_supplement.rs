//! Supplement package import: unpack the `opt/` runtime tree (python, git,
//! rg) next to the executable and refresh the tool command environment in
//! process, so newly spawned tool commands use it without an app restart.

use super::NativeDesktop;
use crate::runtime::{
    embedded_supplement_roots_for_env, load_desktop_settings, refresh_runtime_tool_env,
    resolve_effective_tool_bins,
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

        // Prefer a root that already carries `opt/` (upgrade in place), else
        // the first writable supplement root (AppImage users unpack beside
        // the image because the mount point is read-only).
        let roots = embedded_supplement_roots_for_env(&self.desktop.app_dir);
        let target_root = roots
            .iter()
            .find(|root| root.join("opt").is_dir())
            .cloned()
            .or_else(|| {
                roots.iter().find_map(|root| {
                    std::fs::create_dir_all(root.join("opt")).ok()?;
                    Some(root.clone())
                })
            })
            .ok_or_else(|| anyhow!("没有可写入的补充包目录，请检查安装目录权限"))?;

        let strip = plan_supplement_archive(archive, kind)?;
        let (files, bytes) = match kind {
            archive_extract::ArchiveKind::Zip => {
                let prefix = (!strip.is_empty()).then(|| format!("{strip}/"));
                archive_extract::extract_zip_streaming(
                    archive,
                    &target_root,
                    prefix.as_deref(),
                    |entry| entry.starts_with(SUPPLEMENT_ROOT),
                )?
            }
            _ => extract_tar_supplement(archive, &target_root, &strip)?,
        };
        if !target_root.join("opt").is_dir() {
            bail!("补充包中未找到 opt 运行时目录，不是有效的补充包");
        }

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

/// Inspect the archive entry list and decide which top-level prefix to strip:
/// packages rooted at `opt/` unpack as-is; a package wrapped in one folder
/// (e.g. `wunder-supplement/opt/...`) strips that folder. Anything without an
/// `opt/` runtime tree is rejected before touching the install directory.
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
        .context("系统缺少 tar 命令，Linux 补充包请解压到安装目录或改用 zip 格式")?;
    if !output.status.success() {
        bail!(
            "读取压缩包目录失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let mut names = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let name = line.trim().replace('\\', "/");
        let name = name.trim_end_matches('/');
        if name.is_empty() {
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
        if !strip.is_empty() {
            command.arg("--strip-components=1");
        }
        let status = command.status()?;
        if !status.success() {
            bail!("tar 解压失败，请确认压缩包完整");
        }
        let source_opt = temp.join("opt");
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
    std::fs::create_dir_all(dst)?;
    let mut files = 0u64;
    let mut bytes = 0u64;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let entry_type = entry.file_type()?;
        let target = dst.join(entry.file_name());
        if entry_type.is_dir() {
            let (sub_files, sub_bytes) = merge_dir(&entry.path(), &target)?;
            files += sub_files;
            bytes += sub_bytes;
        } else if entry_type.is_file() {
            bytes += std::fs::copy(entry.path(), &target)?;
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
}
