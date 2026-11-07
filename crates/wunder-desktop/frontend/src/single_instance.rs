use sha2::{Digest, Sha256};
use std::io;
use std::path::{Path, PathBuf};

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

pub fn claim() -> io::Result<Launch> {
    platform::claim(&installation_key(&executable_path()?)?)
}

fn executable_path() -> io::Result<PathBuf> {
    #[cfg(target_os = "linux")]
    if let Some(appimage) = std::env::var_os("APPIMAGE") {
        return PathBuf::from(appimage).canonicalize();
    }
    std::env::current_exe()?.canonicalize()
}

fn installation_key(executable: &Path) -> io::Result<String> {
    #[cfg(windows)]
    let identity = {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::WindowsAndMessaging::CharLowerBuffW;
        let mut wide: Vec<u16> = executable.as_os_str().encode_wide().collect();
        if unsafe { CharLowerBuffW(wide.as_mut_ptr(), wide.len() as u32) } == 0 {
            return Err(io::Error::last_os_error());
        }
        wide.into_iter()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    };
    #[cfg(unix)]
    let identity = {
        use std::os::unix::ffi::OsStrExt;
        executable.as_os_str().as_bytes().to_vec()
    };
    Ok(format!("{:x}", Sha256::digest(identity)))
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
    fn installation_identity_is_stable_and_path_scoped() {
        let first = installation_key(Path::new("install-a/desktop-ui.exe")).unwrap();
        assert_eq!(
            first,
            installation_key(Path::new("install-a/desktop-ui.exe")).unwrap()
        );
        assert_ne!(
            first,
            installation_key(Path::new("install-b/desktop-ui.exe")).unwrap()
        );
        #[cfg(windows)]
        assert_eq!(
            first,
            installation_key(Path::new("INSTALL-A/DESKTOP-UI.EXE")).unwrap()
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
