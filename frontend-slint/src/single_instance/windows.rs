use super::Launch;
use std::io;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use windows_sys::Win32::Foundation::{
    CloseHandle, BOOL, HANDLE, HWND, LPARAM, WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::System::Threading::{
    CreateEventW, CreateMutexW, ReleaseMutex, SetEvent, WaitForSingleObject, INFINITE,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, EnumWindows, GetPropW, GetWindowThreadProcessId, SetPropW,
};

struct Handle(HANDLE);

impl Handle {
    fn new(raw: HANDLE) -> io::Result<Self> {
        if raw == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(raw))
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

pub struct Instance {
    mutex: Handle,
    event: Arc<Handle>,
    property: Vec<u16>,
    stopped: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    owner_thread: PhantomData<Rc<()>>,
}

pub fn claim(key: &str) -> io::Result<Launch> {
    let name: Vec<u16> = format!("Local\\wunder.slint.{key}.owner\0")
        .encode_utf16()
        .collect();
    let mutex = Handle::new(unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) })?;
    let name: Vec<u16> = format!("Local\\wunder.slint.{key}.activate\0")
        .encode_utf16()
        .collect();
    let event = Arc::new(Handle::new(unsafe {
        CreateEventW(std::ptr::null(), 0, 0, name.as_ptr())
    })?);
    let property: Vec<u16> = format!("wunder.slint.{key}\0").encode_utf16().collect();
    match unsafe { WaitForSingleObject(mutex.0, 0) } {
        WAIT_OBJECT_0 | WAIT_ABANDONED => Ok(Launch::Primary(Instance {
            mutex,
            event,
            property,
            stopped: Arc::new(AtomicBool::new(false)),
            worker: None,
            owner_thread: PhantomData,
        })),
        WAIT_TIMEOUT => {
            unsafe {
                EnumWindows(Some(allow_foreground), property.as_ptr() as LPARAM);
            }
            if unsafe { SetEvent(event.0) } == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(Launch::Activated)
        }
        _ => Err(io::Error::last_os_error()),
    }
}

unsafe extern "system" fn allow_foreground(window: HWND, property: LPARAM) -> BOOL {
    if GetPropW(window, property as *const u16) == 0 {
        return 1;
    }
    let mut process = 0;
    GetWindowThreadProcessId(window, &mut process);
    if process != 0 {
        AllowSetForegroundWindow(process);
    }
    0
}

impl Instance {
    pub fn register_window(&self, app: &crate::MainWindow) -> io::Result<()> {
        use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        use slint::winit_030::WinitWindowAccessor;
        use slint::ComponentHandle;

        let weak = app.as_weak();
        let property = self.property.clone();
        slint::spawn_local(async move {
            let Some(app) = weak.upgrade() else { return };
            let Ok(window) = app.window().winit_window().await else {
                return;
            };
            let Ok(handle) = window.window_handle() else {
                return;
            };
            let RawWindowHandle::Win32(handle) = handle.as_raw() else {
                return;
            };
            if unsafe { SetPropW(handle.hwnd.get() as HWND, property.as_ptr(), 1) } == 0 {
                eprintln!(
                    "desktop activation registration failed: {}",
                    io::Error::last_os_error()
                );
            }
        })
        .map(|_| ())
        .map_err(|error| io::Error::other(error.to_string()))
    }

    pub fn listen(&mut self, activate: impl Fn() + Send + 'static) -> io::Result<()> {
        if self.worker.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "activation listener already started",
            ));
        }
        let event = self.event.clone();
        let stopped = self.stopped.clone();
        self.worker = Some(
            std::thread::Builder::new()
                .name("desktop-activation".into())
                .spawn(move || {
                    while unsafe { WaitForSingleObject(event.0, INFINITE) } == WAIT_OBJECT_0 {
                        if stopped.load(Ordering::Acquire) {
                            break;
                        }
                        activate();
                    }
                })?,
        );
        Ok(())
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        unsafe {
            SetEvent(self.event.0);
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        unsafe {
            ReleaseMutex(self.mutex.0);
        }
    }
}
