//! 本地数据面的备份与恢复（桌面版）。
//!
//! 归档只装引擎自己的状态：`<home>/config/**` 与 `<home>/data` 顶层的
//! SQLite 库（含 `-wal`/`-shm` 边车）。工作区文件是用户内容，不在其中。
//! 与安卓版 `BackupBridge` 的归档形状一致，两端的备份可以互相恢复。
//!
//! 恢复不在运行中覆盖数据库：先校验并解包到 `<home>/.restore`，写标记，
//! 由下一次进程启动、在 SQLite 打开之前完成替换。被替换的旧文件会挪到
//! `config.restore-<时间戳>` 与 `data/.pre-restore-<时间戳>` 留底，恢复错了
//! 还能手工找回。

use anyhow::{anyhow, bail, Context as _, Result};
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use zip::write::FileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const ZIP_PREFIX: &str = "wunder-backup-";
const ZIP_SUFFIX: &str = ".zip";
/// 保留的归档数量；更旧的自动清理，目录不会无界增长。
const KEEP_BACKUPS: usize = 5;
/// 单个条目 / 整包的恢复上限，防止畸形归档把磁盘写满。
const MAX_ENTRY_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const MAX_TREE_DEPTH: usize = 32;
const PENDING_MARKER: &str = "pending";
const ASIDE_PREFIX: &str = ".pre-restore-";
/// 崩溃日志预览最多读这么多字节，界面不需要整份回溯。
const CRASH_PREVIEW_BYTES: u64 = 64 * 1024;

/// 数据与恢复页的一行：已有备份。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalBackup {
    pub path: String,
    /// 可读时间：`今天 09:41` / `昨天 18:07` / `2026-11-16 21:52`。
    pub name: String,
    /// 体积与原始文件名，例如 `1.2 MB · wunder-backup-20261116.zip`。
    pub detail: String,
}

/// 共享可写根目录（`WUNDER_HOME`，便携版即安装根）。
pub fn home_dir() -> PathBuf {
    wunder_server::local_bootstrap::resolve_home_from_env_or_user()
}

pub fn backups_dir() -> PathBuf {
    home_dir().join("backups")
}

/// 打包 config 与顶层数据库，返回新归档路径。
pub fn create_backup() -> Result<PathBuf> {
    create_backup_at(&home_dir())
}

/// 已有归档，新→旧。
pub fn list_backups() -> Vec<LocalBackup> {
    list_backups_at(&home_dir())
}

/// 校验并解包到暂存目录，等下次启动生效。
pub fn stage_restore(zip_path: &str) -> Result<()> {
    stage_restore_at(&home_dir(), zip_path)
}

/// 在打开 SQLite 之前应用上一进程留下的恢复。失败时保留标记，下次再试。
pub fn apply_pending_restore() {
    apply_pending_restore_at(&home_dir());
}

/// 最近一份崩溃报告（标题、正文）。
pub fn latest_crash_log() -> Option<(String, String)> {
    latest_crash_log_at(&home_dir())
}

fn create_backup_at(home: &Path) -> Result<PathBuf> {
    let config = home.join("config");
    if !config.is_dir() {
        bail!("本地配置目录不存在");
    }
    let directory = home.join("backups");
    fs::create_dir_all(&directory)
        .with_context(|| format!("创建备份目录失败：{}", directory.display()))?;
    let target = directory.join(format!(
        "{ZIP_PREFIX}{}{ZIP_SUFFIX}",
        chrono::Utc::now().timestamp_millis()
    ));
    let file = File::create(&target)
        .with_context(|| format!("创建备份文件失败：{}", target.display()))?;
    let mut zip = ZipWriter::new(BufWriter::new(file));
    let options = FileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);
    let staged = (|| -> Result<()> {
        add_tree(&mut zip, &config, &config, "config", 0, &options)?;
        let data = home.join("data");
        let mut databases: Vec<PathBuf> = match fs::read_dir(&data) {
            Ok(entries) => entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| {
                    path.is_file()
                        && path
                            .file_name()
                            .map(|name| is_database_name(&name.to_string_lossy()))
                            .unwrap_or(false)
                })
                .collect(),
            Err(_) => Vec::new(),
        };
        databases.sort();
        for database in databases {
            let name = database
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            add_file(&mut zip, &database, &format!("data/{name}"), &options)?;
        }
        Ok(())
    })();
    if let Err(error) = staged {
        // 半成品归档不能留在目录里，否则它会被当成一份可恢复的备份。
        drop(zip);
        let _ = fs::remove_file(&target);
        return Err(error);
    }
    let mut writer = zip.finish().context("写入备份包失败")?;
    writer.flush().context("写入备份包失败")?;
    let file = writer.into_inner().map_err(|error| anyhow!("{error}"))?;
    file.sync_all().ok();
    prune_backups(&directory);
    Ok(target)
}

fn add_tree(
    zip: &mut ZipWriter<BufWriter<File>>,
    root: &Path,
    file: &Path,
    entry: &str,
    depth: usize,
    options: &FileOptions,
) -> Result<()> {
    if depth > MAX_TREE_DEPTH {
        return Ok(());
    }
    if file.is_dir() {
        let mut children: Vec<PathBuf> = match fs::read_dir(file) {
            Ok(entries) => entries.flatten().map(|entry| entry.path()).collect(),
            Err(error) => {
                // 读不动的子目录（权限/占用）跳过，不让整份备份失败。
                eprintln!("skip unreadable backup directory {}: {error}", file.display());
                return Ok(());
            }
        };
        children.sort();
        for child in children {
            let name = child
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            add_tree(zip, root, &child, &format!("{entry}/{name}"), depth + 1, options)?;
        }
        return Ok(());
    }
    if !file.is_file() {
        return Ok(());
    }
    // 符号链接可能指向家目录之外；归档只收根目录内的真实文件。
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    if !canonical.starts_with(root) {
        return Ok(());
    }
    add_file(zip, &canonical, entry, options)
}

fn add_file(
    zip: &mut ZipWriter<BufWriter<File>>,
    file: &Path,
    entry: &str,
    options: &FileOptions,
) -> Result<()> {
    let meta = file.metadata()?;
    if meta.len() > MAX_ENTRY_BYTES {
        eprintln!("skip oversized backup entry {entry}");
        return Ok(());
    }
    let modified = meta.modified().ok().and_then(|time| {
        let stamp: chrono::DateTime<chrono::Utc> = time.into();
        Some(zip::DateTime::from_date_and_time(
            stamp.format("%Y").to_string().parse().ok()?,
            stamp.format("%m").to_string().parse().ok()?,
            stamp.format("%d").to_string().parse().ok()?,
            stamp.format("%H").to_string().parse().ok()?,
            stamp.format("%M").to_string().parse().ok()?,
            stamp.format("%S").to_string().parse().ok()?,
        )
        .ok()?)
    });
    let mut header = (*options).clone();
    if let Some(modified) = modified {
        header = header.last_modified_time(modified);
    }
    zip.start_file(entry, header)
        .with_context(|| format!("写入备份条目失败：{entry}"))?;
    let mut input = BufReader::new(File::open(file)?);
    let mut buffer = vec![0u8; 256 * 1024];
    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        zip.write_all(&buffer[..read])?;
    }
    Ok(())
}

fn list_backups_at(home: &Path) -> Vec<LocalBackup> {
    let directory = home.join("backups");
    let mut archives: Vec<PathBuf> = match fs::read_dir(&directory) {
        Ok(entries) => entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_file() && is_archive_name(&file_name_of(path)))
            .collect(),
        Err(_) => Vec::new(),
    };
    // 文件名里的毫秒时间戳就是创建顺序，倒序即新→旧。
    archives.sort_by(|left, right| file_name_of(right).cmp(&file_name_of(left)));
    archives
        .iter()
        .map(|path| backup_row(path))
        .collect::<Vec<_>>()
}

fn backup_row(path: &Path) -> LocalBackup {
    let file_name = file_name_of(path);
    let meta = fs::metadata(path).ok();
    let stamp = file_name
        .strip_prefix(ZIP_PREFIX)
        .and_then(|rest| rest.strip_suffix(ZIP_SUFFIX))
        .and_then(|millis| millis.parse::<i64>().ok())
        .and_then(chrono::DateTime::from_timestamp_millis)
        .map(|utc| utc.with_timezone(&chrono::Local))
        .or_else(|| {
            let modified = meta.as_ref()?.modified().ok()?;
            Some(chrono::DateTime::<chrono::Local>::from(modified))
        });
    let name = stamp
        .map(|stamp| {
            let today = chrono::Local::now().date_naive();
            match (today - stamp.date_naive()).num_days() {
                0 => format!("今天 {}", stamp.format("%H:%M")),
                1 => format!("昨天 {}", stamp.format("%H:%M")),
                _ => stamp.format("%Y-%m-%d %H:%M").to_string(),
            }
        })
        .unwrap_or_else(|| file_name.clone());
    let detail = match meta {
        Some(meta) => format!("{} · {file_name}", format_size(meta.len())),
        None => file_name,
    };
    LocalBackup {
        path: path.to_string_lossy().into_owned(),
        name,
        detail,
    }
}

fn format_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    let bytes = bytes as f64;
    if bytes >= MB {
        format!("{:.1} MB", bytes / MB)
    } else if bytes >= KB {
        format!("{:.0} KB", bytes / KB)
    } else {
        format!("{} B", bytes as u64)
    }
}

fn prune_backups(directory: &Path) {
    let mut archives: Vec<PathBuf> = match fs::read_dir(directory) {
        Ok(entries) => entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_file() && is_archive_name(&file_name_of(path)))
            .collect(),
        Err(_) => return,
    };
    archives.sort_by(|left, right| file_name_of(left).cmp(&file_name_of(right)));
    while archives.len() > KEEP_BACKUPS {
        let oldest = archives.remove(0);
        let _ = fs::remove_file(oldest);
    }
}

fn stage_restore_at(home: &Path, zip_path: &str) -> Result<()> {
    let source = zip_path.trim();
    if source.is_empty() {
        bail!("未指定备份文件");
    }
    let archive = Path::new(source);
    if !archive.is_file() {
        bail!("备份文件不存在或不可读");
    }
    let staging = home.join(".restore");
    if staging.exists() {
        fs::remove_dir_all(&staging).context("清理上一次的恢复暂存失败")?;
    }
    fs::create_dir_all(&staging).context("创建恢复暂存目录失败")?;
    let mut total = 0u64;
    let mut saw_entry = false;
    let result = (|| -> Result<()> {
        let file = File::open(archive)?;
        let mut zip = ZipArchive::new(BufReader::new(file)).context("备份包格式不受支持")?;
        for index in 0..zip.len() {
            let mut entry = zip.by_index(index)?;
            if entry.is_dir() {
                continue;
            }
            let Some(relative) = entry.enclosed_name() else {
                bail!("备份包包含不安全路径");
            };
            let name = relative.to_string_lossy().replace('\\', "/");
            if !safe_entry_name(&name) {
                bail!("备份包包含不受支持的条目：{name}");
            }
            let target = staging.join(&name);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).context("写入恢复暂存目录失败")?;
            }
            let mut output = BufWriter::new(File::create(&target)?);
            let written = copy_bounded(&mut entry, &mut output, MAX_ENTRY_BYTES)?;
            output.flush().ok();
            total += written;
            if total > MAX_TOTAL_BYTES {
                bail!("备份包过大");
            }
            saw_entry = true;
        }
        Ok(())
    })();
    if let Err(error) = result {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }
    if !saw_entry {
        let _ = fs::remove_dir_all(&staging);
        bail!("备份包为空或格式不受支持");
    }
    fs::write(
        staging.join(PENDING_MARKER),
        chrono::Utc::now().timestamp_millis().to_string(),
    )
    .context("写入恢复标记失败")?;
    Ok(())
}

fn copy_bounded(input: &mut impl Read, output: &mut impl Write, limit: u64) -> Result<u64> {
    let mut buffer = vec![0u8; 256 * 1024];
    let mut total = 0u64;
    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        total += read as u64;
        if total > limit {
            bail!("单个文件超过恢复上限");
        }
        output.write_all(&buffer[..read])?;
    }
    Ok(total)
}

fn apply_pending_restore_at(home: &Path) {
    let staging = home.join(".restore");
    if !staging.join(PENDING_MARKER).is_file() {
        return;
    }
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    match swap_staged(home, &staging, &stamp) {
        Ok(applied) => {
            if let Err(error) = fs::remove_dir_all(&staging) {
                eprintln!("remove the applied restore staging: {error}");
            }
            eprintln!("applied {applied} restored item(s) from the pending backup");
        }
        Err(error) => {
            // 标记留着，下次启动再试；暂存内容不删。
            eprintln!("pending restore is incomplete, will retry next start: {error:#}");
        }
    }
}

/// 把暂存内容换到现网位置；返回搬入的条目数。任一步失败即返回错误，
/// 已完成的步骤保持现状（config 与 data 各自成段，互不牵连）。
fn swap_staged(home: &Path, staging: &Path, stamp: &str) -> Result<usize> {
    let mut applied = 0usize;

    let incoming_config = staging.join("config");
    if incoming_config.is_dir() {
        let config = home.join("config");
        let aside = home.join(format!("config.restore-{stamp}"));
        let had_config = config.exists();
        if had_config {
            fs::rename(&config, &aside).context("挪开当前 config 失败")?;
        }
        match fs::rename(&incoming_config, &config) {
            Ok(()) => {
                applied += 1;
                if had_config {
                    if let Err(error) = fs::remove_dir_all(&aside) {
                        eprintln!("keep the replaced config copy: {error}");
                    }
                }
            }
            Err(error) => {
                if had_config {
                    let _ = fs::rename(&aside, &config);
                }
                return Err(anyhow!("替换 config 失败：{error}"));
            }
        }
    }

    let incoming_data = staging.join("data");
    if incoming_data.is_dir() {
        let data = home.join("data");
        fs::create_dir_all(&data).context("创建 data 目录失败")?;
        let aside = data.join(format!("{ASIDE_PREFIX}{stamp}"));
        // 留一份被替换的库：恢复错了还能手工找回来。
        fs::create_dir_all(&aside).ok();
        // 先把归档里的文件名收集齐：判断「哪些旧边车该丢」必须按整包看，
        // 否则先搬正文再搬边车时会把刚恢复的 -wal 当残留删掉。
        let mut incoming: Vec<(std::path::PathBuf, String)> = Vec::new();
        let entries = fs::read_dir(&incoming_data).context("读取恢复暂存目录失败")?;
        for entry in entries.flatten() {
            let source = entry.path();
            if !source.is_file() {
                continue;
            }
            let Some(name) = source.file_name().map(|name| name.to_string_lossy().into_owned())
            else {
                continue;
            };
            incoming.push((source, name));
        }
        // 名字要活过 `incoming` 的 move：下面按值遍历时仍要查这份集合。
        let incoming_names: std::collections::HashSet<String> =
            incoming.iter().map(|(_, name)| name.clone()).collect();
        for (source, name) in incoming {
            let target = data.join(&name);
            if target.exists() {
                let _ = fs::rename(&target, aside.join(&name));
            }
            if name.to_ascii_lowercase().ends_with(".sqlite") {
                // 归档没带这个库的边车时，旧库残留的 -wal/-shm 才会污染它。
                for sidecar in ["-wal", "-shm"] {
                    let sidecar_name = format!("{name}{sidecar}");
                    if incoming_names.contains(&sidecar_name) {
                        continue;
                    }
                    let stale = data.join(&sidecar_name);
                    if stale.exists() {
                        let _ = fs::rename(&stale, aside.join(&sidecar_name));
                    }
                }
            }
            fs::rename(&source, &target).with_context(|| format!("恢复 {name} 失败"))?;
            applied += 1;
        }
        // 线程租约与锁记录的是旧库里的线程，跟新库对不上；一并挪进留底目录，
        // 让引擎启动时按新库重新受理，而不是复活已经不存在的任务。
        for stale in ["instances", "locks", "sessions"] {
            let path = data.join(stale);
            if path.exists() {
                let _ = fs::rename(&path, aside.join(stale));
            }
        }
        if aside.is_dir() && fs::read_dir(&aside).map(|mut it| it.next().is_none()).unwrap_or(true) {
            let _ = fs::remove_dir(&aside);
        }
    }
    Ok(applied)
}

/// 只允许 config 树与 data 顶层的数据库，和归档写入侧对称。
fn safe_entry_name(name: &str) -> bool {
    if name.is_empty() || name.starts_with('/') || name.contains('\\') || name.contains('\0') {
        return false;
    }
    let segments: Vec<&str> = name.split('/').collect();
    if segments
        .iter()
        .any(|segment| segment.is_empty() || *segment == "." || *segment == "..")
    {
        return false;
    }
    match segments[0] {
        "config" => segments.len() >= 2,
        "data" => segments.len() == 2 && is_database_name(segments[1]),
        _ => false,
    }
}

fn is_database_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".sqlite") || lower.ends_with(".sqlite-wal") || lower.ends_with(".sqlite-shm")
}

fn is_archive_name(name: &str) -> bool {
    name.starts_with(ZIP_PREFIX) && name.ends_with(ZIP_SUFFIX)
}

fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn latest_crash_log_at(home: &Path) -> Option<(String, String)> {
    let directory = home.join("logs");
    let mut reports: Vec<PathBuf> = fs::read_dir(&directory)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("crash-") && name.ends_with(".log"))
        })
        .collect();
    reports.sort();
    let newest = reports.pop()?;
    let name = file_name_of(&newest);
    let mut text = String::new();
    let file = File::open(&newest).ok()?;
    file.take(CRASH_PREVIEW_BYTES)
        .read_to_string(&mut text)
        .ok()?;
    Some((name, text))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_home(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "wunder-backup-{tag}-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        fs::create_dir_all(dir.join("config")).unwrap();
        fs::create_dir_all(dir.join("data")).unwrap();
        dir
    }

    #[test]
    fn round_trip_restores_config_and_databases() {
        let home = temp_home("roundtrip");
        fs::create_dir_all(home.join("config/prompts")).unwrap();
        fs::write(home.join("config/wunder.yaml"), b"model: one").unwrap();
        fs::write(home.join("config/prompts/a.md"), b"prompt").unwrap();
        fs::write(home.join("data/state_1.sqlite"), b"db").unwrap();
        fs::write(home.join("data/state_1.sqlite-wal"), b"wal").unwrap();
        // 子目录不是数据库，不进归档。
        fs::create_dir_all(home.join("data/instances")).unwrap();
        fs::write(home.join("data/instances/lease"), b"x").unwrap();

        let archive = create_backup_at(&home).unwrap();
        let rows = list_backups_at(&home);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].path, archive.to_string_lossy());
        assert!(rows[0].detail.contains("wunder-backup-"));

        // 破坏现网数据，再用备份恢复。
        fs::write(home.join("config/wunder.yaml"), b"broken").unwrap();
        fs::write(home.join("data/state_1.sqlite"), b"broken").unwrap();
        stage_restore_at(&home, &archive.to_string_lossy()).unwrap();
        apply_pending_restore_at(&home);

        assert_eq!(fs::read(home.join("config/wunder.yaml")).unwrap(), b"model: one");
        assert_eq!(fs::read(home.join("config/prompts/a.md")).unwrap(), b"prompt");
        assert_eq!(fs::read(home.join("data/state_1.sqlite")).unwrap(), b"db");
        assert_eq!(fs::read(home.join("data/state_1.sqlite-wal")).unwrap(), b"wal");
        // 旧库的线程租约跟新库对不上，随替换一起挪进留底目录。
        assert!(!home.join("data/instances").exists());
        let aside = fs::read_dir(home.join("data"))
            .unwrap()
            .flatten()
            .map(|entry| entry.path())
            .find(|path| {
                path.file_name()
                    .map(|name| name.to_string_lossy().starts_with(ASIDE_PREFIX))
                    .unwrap_or(false)
            })
            .expect("被替换的旧数据应留在 data/.pre-restore-* 下");
        assert!(aside.join("state_1.sqlite").is_file());
        assert!(aside.join("instances/lease").is_file());
        assert!(!home.join(".restore").exists());
    }

    #[test]
    fn prune_keeps_newest_five() {
        let home = temp_home("prune");
        fs::write(home.join("config/wunder.yaml"), b"x").unwrap();
        for _ in 0..7 {
            create_backup_at(&home).unwrap();
            // 毫秒级命名可能撞名，隔开一点保证顺序可判定。
            std::thread::sleep(std::time::Duration::from_millis(3));
        }
        assert_eq!(list_backups_at(&home).len(), KEEP_BACKUPS);
    }

    #[test]
    fn restore_rejects_unsafe_entries() {
        let home = temp_home("unsafe");
        let archive = home.join("evil.zip");
        let file = File::create(&archive).unwrap();
        let mut zip = ZipWriter::new(file);
        zip.start_file("../escape", FileOptions::default()).unwrap();
        zip.write_all(b"x").unwrap();
        zip.finish().unwrap();
        assert!(stage_restore_at(&home, &archive.to_string_lossy()).is_err());
    }

    #[test]
    fn crash_preview_picks_the_newest_report() {
        let home = temp_home("crash");
        fs::create_dir_all(home.join("logs")).unwrap();
        fs::write(home.join("logs/crash-20260101-000000-000.log"), b"old").unwrap();
        fs::write(home.join("logs/crash-20260102-000000-000.log"), b"new").unwrap();
        let (name, text) = latest_crash_log_at(&home).unwrap();
        assert_eq!(name, "crash-20260102-000000-000.log");
        assert_eq!(text, "new");
    }
}
