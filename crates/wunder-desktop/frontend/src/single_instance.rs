use sha2::{Digest, Sha256};
use std::io;
use std::path::Path;

#[cfg(unix)]
#[path = "single_instance/unix.rs"]
mod platform;
#[cfg(windows)]
#[path = "single_instance/windows.rs"]
mod platform;

pub enum Launch {
    Primary(platform::Instance),
    Activated,
}

/// 单实例按**本地数据目录**认领，而不是按可执行文件路径。
///
/// 安装版与开发版、或两个不同安装目录的桌面版，可能指向同一个 `~/.wunder`
/// （同一套 SQLite、租约和线程历史）。按 exe 路径分会把它们当成互不相干的
/// 两个实例同时启动：后启动的那个在崩溃恢复里会清掉前者的线程租约，并把
/// 正在执行的轮次结算成「已停止」，用户看到的就是正常完成的回复被标成中断。
///
/// 可执行文件只是入口；共享同一份数据的桌面进程必须只有一个。
/// 便携版各自带独立 home，因此仍然可以并行运行。
pub fn claim() -> io::Result<Launch> {
    platform::claim(&instance_key()?)
}

fn instance_key() -> io::Result<String> {
    let home = wunder_server::local_bootstrap::resolve_home_from_env_or_user();
    // 同一条路径可能有多种写法（软链接、大小写、末尾斜杠）；能规范化就
    // 规范化，规范化失败的路径仍按原样参与哈希，宁可多认一个实例也不能
    // 因为读不到目录而让保护失效。
    let home = home.canonicalize().unwrap_or(home);
    Ok(home_key(home.as_path()))
}

fn home_key(home: &Path) -> String {
    #[cfg(windows)]
    let identity = {
        // Windows 路径大小写不敏感：同一个 home 不能因为盘符或目录名大小写
        // 不同而被当成两个实例。
        home.to_string_lossy().replace('\\', "/").to_lowercase().into_bytes()
    };
    #[cfg(unix)]
    let identity = {
        use std::os::unix::ffi::OsStrExt;
        home.as_os_str().as_bytes().to_vec()
    };
    format!("home:{:x}", Sha256::digest(&identity))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;

    fn unique_key() -> String {
        static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
        format!(
            "test-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        )
    }

    fn primary(key: &str) -> platform::Instance {
        match platform::claim(key).unwrap() {
            Launch::Primary(instance) => instance,
            Launch::Activated => panic!("expected independent primary"),
        }
    }

    fn activate(key: String) {
        std::thread::spawn(move || {
            assert!(matches!(platform::claim(&key).unwrap(), Launch::Activated));
        })
        .join()
        .unwrap();
    }

    #[test]
    fn instance_key_is_stable_and_home_scoped() {
        let first = home_key(Path::new("home-a/.wunder"));
        assert_eq!(first, home_key(Path::new("home-a/.wunder")));
        assert_ne!(first, home_key(Path::new("home-b/.wunder")));
        assert!(first.starts_with("home:"));
        #[cfg(windows)]
        assert_eq!(
            home_key(Path::new("C:\\Users\\Sample\\.wunder")),
            home_key(Path::new("c:/users/sample/.wunder"))
        );
    }

    #[test]
    fn duplicate_launch_wakes_primary_repeatedly() {
        let key = unique_key();
        let mut instance = primary(&key);
        let (sender, receiver) = mpsc::sync_channel(1);
        instance
            .listen(move || {
                let _ = sender.try_send(());
            })
            .unwrap();
        for _attempt in 0..3 {
            activate(key.clone());
            receiver.recv_timeout(Duration::from_secs(3)).unwrap();
        }
    }

    #[test]
    fn activation_survives_primary_startup() {
        let key = unique_key();
        let mut instance = primary(&key);
        activate(key);
        let (sender, receiver) = mpsc::sync_channel(1);
        instance
            .listen(move || {
                let _ = sender.try_send(());
            })
            .unwrap();
        receiver.recv_timeout(Duration::from_secs(3)).unwrap();
    }

    #[test]
    fn distinct_installations_have_independent_ownership_and_activation() {
        let first_key = unique_key();
        let second_key = unique_key();
        let mut first = primary(&first_key);
        let mut second = primary(&second_key);
        let (first_sender, first_receiver) = mpsc::sync_channel(1);
        let (second_sender, second_receiver) = mpsc::sync_channel(1);
        first
            .listen(move || {
                let _ = first_sender.try_send(());
            })
            .unwrap();
        second
            .listen(move || {
                let _ = second_sender.try_send(());
            })
            .unwrap();
        activate(second_key);
        second_receiver
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        assert!(matches!(
            first_receiver.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        activate(first_key);
        first_receiver.recv_timeout(Duration::from_secs(3)).unwrap();
    }

    #[test]
    fn releasing_primary_allows_restart() {
        let key = unique_key();
        let mut instance = primary(&key);
        instance.listen(|| {}).unwrap();
        drop(instance);
        drop(primary(&key));
    }
}
