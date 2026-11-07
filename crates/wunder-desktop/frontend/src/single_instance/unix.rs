use super::Launch;
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::UnixDatagram;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

pub struct Instance {
    lock: File,
    socket: Option<Arc<UnixDatagram>>,
    socket_path: PathBuf,
    stopped: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

pub fn claim(key: &str) -> io::Result<Launch> {
    let user = unsafe { libc::geteuid() };
    let directory = PathBuf::from("/tmp").join(format!("wunder-slint-{user}"));
    let mut builder = fs::DirBuilder::new();
    builder.mode(0o700);
    match builder.create(&directory) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    let metadata = fs::symlink_metadata(&directory)?;
    if !metadata.is_dir() || metadata.uid() != user || metadata.mode() & 0o077 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "unsafe desktop instance directory",
        ));
    }
    let display = std::env::var_os("WAYLAND_DISPLAY")
        .or_else(|| std::env::var_os("DISPLAY"))
        .unwrap_or_default();
    let mut identity = Sha256::new();
    identity.update(key.as_bytes());
    identity.update(display.as_encoded_bytes());
    let name = format!("{:x}", identity.finalize());
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(directory.join(format!("{name}.lock")))?;
    let socket_path = directory.join(format!("{name}.sock"));
    for _attempt in 0..20 {
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            let mut instance = Instance {
                lock,
                socket: None,
                socket_path,
                stopped: Arc::new(AtomicBool::new(false)),
                worker: None,
            };
            match fs::remove_file(&instance.socket_path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            let socket = UnixDatagram::bind(&instance.socket_path)?;
            fs::set_permissions(&instance.socket_path, fs::Permissions::from_mode(0o600))?;
            socket.set_read_timeout(Some(Duration::from_secs(1)))?;
            socket.set_write_timeout(Some(Duration::from_millis(50)))?;
            instance.socket = Some(Arc::new(socket));
            return Ok(Launch::Primary(instance));
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::WouldBlock && error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
        let client = UnixDatagram::unbound()?;
        client.set_write_timeout(Some(Duration::from_millis(50)))?;
        match client.send_to(b"activate", &socket_path) {
            Ok(_) => return Ok(Launch::Activated),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound
                        | io::ErrorKind::ConnectionRefused
                        | io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                ) => {}
            Err(error) => return Err(error),
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        "desktop instance did not accept activation",
    ))
}

impl Instance {
    pub fn listen(&mut self, activate: impl Fn() + Send + 'static) -> io::Result<()> {
        if self.worker.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "activation listener already started",
            ));
        }
        let socket = self.socket.as_ref().unwrap().clone();
        let stopped = self.stopped.clone();
        self.worker = Some(
            std::thread::Builder::new()
                .name("desktop-activation".into())
                .spawn(move || {
                    let mut message = [0; 16];
                    while !stopped.load(Ordering::Acquire) {
                        match socket.recv(&mut message) {
                            Ok(length) if &message[..length] == b"activate" => activate(),
                            Ok(_) => {}
                            Err(error)
                                if matches!(
                                    error.kind(),
                                    io::ErrorKind::WouldBlock
                                        | io::ErrorKind::TimedOut
                                        | io::ErrorKind::Interrupted
                                ) => {}
                            Err(_) => break,
                        }
                    }
                })?,
        );
        Ok(())
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        if let Some(socket) = &self.socket {
            let _ = socket.send_to(b"stop", &self.socket_path);
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        self.socket.take();
        let _ = fs::remove_file(&self.socket_path);
        unsafe {
            libc::flock(self.lock.as_raw_fd(), libc::LOCK_UN);
        }
    }
}
