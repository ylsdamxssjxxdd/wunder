//! 本地实例存活标记：让线程租约能判断“持有者是不是已经死了”。
//!
//! 打开租约（`thread_open_leases`）只靠 15 秒 TTL 兜底，进程崩溃或被强杀后
//! 在这段时间内会让另一个实例一直收到 `THREAD_IN_USE`。这里给每个本地进程
//! 建一个 flock 独占的实例文件：进程活着时锁一直被持有，进程消失（无论正常
//! 退出、崩溃还是 SIGKILL）内核都会释放它。
//!
//! 判活只有一个安全方向：**能拿到锁 = 进程已死**。文件不存在、无权限或其它
//! IO 错误都返回 `None`（无法判断），调用方继续保持 TTL 行为，绝不抢占可能
//! 还活着的实例。

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// 本进程的实例 id，供跨重启记录（如后台命令）标注归属。
static CURRENT_INSTANCE_ID: OnceLock<String> = OnceLock::new();

/// 记录本进程的实例 id；只接受一次，非法 id 忽略。
pub fn set_current_instance(instance_id: &str) {
    let Ok(id) = sanitize_id(instance_id) else {
        return;
    };
    let _ = CURRENT_INSTANCE_ID.set(id.to_string());
}

pub fn current_instance() -> Option<&'static str> {
    CURRENT_INSTANCE_ID.get().map(String::as_str)
}

/// 持有实例存活文件；Drop 时解锁并删除。
pub struct InstanceGuard {
    path: PathBuf,
    file: File,
}

impl InstanceGuard {
    /// 在 `dir` 下认领 `instance_id` 的存活文件。重复认领同一个 id 会失败。
    pub fn claim(dir: &Path, instance_id: &str) -> io::Result<Self> {
        let id = sanitize_id(instance_id)?;
        std::fs::create_dir_all(dir)?;
        let path = dir.join(lease_file_name(id));
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)?;
        file.try_lock().map_err(|error| match error {
            TryLockError::WouldBlock => {
                io::Error::new(io::ErrorKind::WouldBlock, "instance id already claimed")
            }
            TryLockError::Error(error) => error,
        })?;
        // 内容和存活无关，只写 pid 方便事后排查。
        let _ = file.set_len(0);
        let _ = writeln!(file, "pid={}", std::process::id());
        let _ = file.flush();
        Ok(Self { path, file })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        let _ = self.file.unlock();
        let _ = std::fs::remove_file(&self.path);
    }
}

/// 判断本地实例是否还活着。
///
/// - `Some(true)`：锁正被持有，进程还活着；
/// - `Some(false)`：文件存在但没有锁 —— 持有者已经消失，文件会被顺手清掉；
/// - `None`：无法判断（文件不存在、权限或 IO 错误），调用方应保持 TTL 兜底。
pub fn is_instance_alive(dir: &Path, instance_id: &str) -> Option<bool> {
    let id = sanitize_id(instance_id).ok()?;
    let path = dir.join(lease_file_name(id));
    let file = OpenOptions::new().read(true).write(true).open(&path).ok()?;
    match file.try_lock() {
        Ok(()) => {
            let _ = file.unlock();
            let _ = std::fs::remove_file(&path);
            Some(false)
        }
        Err(TryLockError::WouldBlock) => Some(true),
        Err(TryLockError::Error(_)) => None,
    }
}

/// 回收“持有者已死”的本地实例留下的线程打开租约。
///
/// 只认 `desktop` / `cli` 两种本地客户端，并且只在实例存活文件明确报告
/// 已死（锁可获取）时动手；文件不存在、无权限或其它错误都视为未知，
/// 保持原来的 TTL 兜底，避免抢占可能还活着的实例。
pub fn reclaim_dead_thread_lease(
    storage: &dyn crate::storage::StorageBackend,
    user_id: &str,
    session_id: &str,
    instance_dir: &Path,
    self_instance_id: &str,
) -> bool {
    let Ok(Some(lease)) = storage.get_thread_open_lease(session_id, user_id) else {
        return false;
    };
    if lease.owner_instance_id == self_instance_id
        || !matches!(lease.client_kind.as_str(), "desktop" | "cli")
    {
        return false;
    }
    if is_instance_alive(instance_dir, &lease.owner_instance_id) != Some(false) {
        return false;
    }
    storage
        .release_thread_open_lease(session_id, &lease.owner_instance_id, lease.fencing_token)
        .unwrap_or(false)
}

fn lease_file_name(id: &str) -> String {
    let mut name = String::with_capacity(id.len() + 5);
    name.push_str(id);
    name.push_str(".lock");
    name
}

fn sanitize_id(instance_id: &str) -> io::Result<&str> {
    let trimmed = instance_id.trim();
    if trimmed.is_empty()
        || !trimmed
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid instance id",
        ));
    }
    Ok(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let mut name = String::from("wunder-instance-liveness-");
        name.push_str(tag);
        name.push('-');
        name.push_str(&std::process::id().to_string());
        name.push('-');
        name.push_str(
            &std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
                .to_string(),
        );
        let dir = std::env::temp_dir().join(name);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn claimed_instance_is_alive_and_released_on_drop() {
        let dir = temp_dir("claim");
        let guard = InstanceGuard::claim(&dir, "instance-a").unwrap();
        assert_eq!(is_instance_alive(&dir, "instance-a"), Some(true));
        let path = guard.path().to_path_buf();
        drop(guard);
        assert!(!path.exists());
        assert_eq!(is_instance_alive(&dir, "instance-a"), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unlocked_leftover_file_is_reported_dead_and_removed() {
        let dir = temp_dir("leftover");
        let path = dir.join("instance-b.lock");
        std::fs::write(&path, b"pid=1\n").unwrap();
        assert_eq!(is_instance_alive(&dir, "instance-b"), Some(false));
        assert!(!path.exists(), "dead instance file should be cleaned up");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unknown_instance_is_not_guessed() {
        let dir = temp_dir("unknown");
        assert_eq!(is_instance_alive(&dir, "instance-c"), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn invalid_ids_are_rejected() {
        let dir = temp_dir("invalid");
        assert!(InstanceGuard::claim(&dir, "../escape").is_err());
        assert_eq!(is_instance_alive(&dir, ""), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn current_instance_keeps_the_first_valid_claim() {
        set_current_instance("fixture-instance");
        assert_eq!(current_instance(), Some("fixture-instance"));
        set_current_instance("../invalid");
        assert_eq!(current_instance(), Some("fixture-instance"));
    }
}
