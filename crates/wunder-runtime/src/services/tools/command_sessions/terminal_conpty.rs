//! Native Windows pseudoconsole. Entry points are resolved at runtime so the
//! same binary still starts on Windows 7, where winpty supplies the console.
use std::{
    ffi::c_void,
    fs::File,
    mem::{size_of, zeroed},
    os::windows::io::{FromRawHandle, OwnedHandle},
    path::Path,
    ptr,
    sync::{Arc, Mutex, OnceLock},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE},
    System::{
        LibraryLoader::{GetModuleHandleW, GetProcAddress},
        Pipes::CreatePipe,
        Threading::{
            CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess,
            InitializeProcThreadAttributeList, TerminateProcess, UpdateProcThreadAttribute,
            WaitForSingleObject, CREATE_UNICODE_ENVIRONMENT, EXTENDED_STARTUPINFO_PRESENT,
            INFINITE, PROCESS_INFORMATION, STARTF_USESTDHANDLES, STARTUPINFOEXW,
        },
    },
};

#[repr(C)]
#[derive(Clone, Copy)]
struct Coord {
    x: i16,
    y: i16,
}
type Hpc = isize;
struct Api {
    create: unsafe extern "system" fn(Coord, HANDLE, HANDLE, u32, *mut Hpc) -> i32,
    resize: unsafe extern "system" fn(Hpc, Coord) -> i32,
    close: unsafe extern "system" fn(Hpc),
}
static API: OnceLock<Option<Api>> = OnceLock::new();

pub struct NativeConsole {
    console: Mutex<Hpc>,
    process: OwnedHandle,
    api: &'static Api,
}
pub struct NativeSession {
    pub console: Arc<NativeConsole>,
    pub input: File,
    pub output: File,
}

fn api() -> Option<&'static Api> {
    API.get_or_init(|| unsafe {
        let module = GetModuleHandleW(wide("kernel32.dll").as_ptr());
        if module == 0 {
            return None;
        }
        Some(Api {
            create: std::mem::transmute(GetProcAddress(
                module,
                c"CreatePseudoConsole".as_ptr().cast(),
            )?),
            resize: std::mem::transmute(GetProcAddress(
                module,
                c"ResizePseudoConsole".as_ptr().cast(),
            )?),
            close: std::mem::transmute(GetProcAddress(
                module,
                c"ClosePseudoConsole".as_ptr().cast(),
            )?),
        })
    })
    .as_ref()
}

fn pipe() -> Result<(OwnedHandle, OwnedHandle), String> {
    let (mut read, mut write) = (0, 0);
    if unsafe { CreatePipe(&mut read, &mut write, ptr::null(), 0) } == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(unsafe {
        (
            OwnedHandle::from_raw_handle(read as _),
            OwnedHandle::from_raw_handle(write as _),
        )
    })
}

pub fn spawn(
    cmdline: &str,
    cwd: &Path,
    env: &[(String, String)],
    cols: u16,
    rows: u16,
) -> Result<NativeSession, String> {
    use std::os::windows::io::AsRawHandle;
    let api = api().ok_or("native pseudoconsole unavailable")?;
    let (input_read, input_write) = pipe()?;
    let (output_read, output_write) = pipe()?;
    let mut hpc = 0;
    let result = unsafe {
        (api.create)(
            size(cols, rows),
            input_read.as_raw_handle() as _,
            output_write.as_raw_handle() as _,
            0,
            &mut hpc,
        )
    };
    if result < 0 {
        return Err(format!("pseudoconsole creation failed ({result:#x})"));
    }
    drop(input_read);
    drop(output_write);
    // Own the pseudoconsole during setup as well, including every failure path.
    struct Pending<'a>(Hpc, &'a Api);
    impl Drop for Pending<'_> {
        fn drop(&mut self) {
            if self.0 != 0 {
                unsafe { (self.1.close)(self.0) };
            }
        }
    }
    let mut pending = Pending(hpc, api);
    let mut bytes = 0;
    unsafe {
        InitializeProcThreadAttributeList(ptr::null_mut(), 1, 0, &mut bytes);
    }
    let mut memory = vec![0usize; bytes.div_ceil(size_of::<usize>())];
    let attributes = memory.as_mut_ptr().cast();
    if unsafe { InitializeProcThreadAttributeList(attributes, 1, 0, &mut bytes) } == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let mut startup: STARTUPINFOEXW = unsafe { zeroed() };
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    startup.lpAttributeList = attributes;
    // Clear inherited redirected handles: MSYS checks them before attaching
    // to its console, unlike native cmd/PowerShell.
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = -1;
    startup.StartupInfo.hStdOutput = -1;
    startup.StartupInfo.hStdError = -1;
    let mut process: PROCESS_INFORMATION = unsafe { zeroed() };
    let mut cmdline = wide(cmdline);
    let cwd = wide(&cwd.to_string_lossy());
    let environment = super::terminal_pty::env_block(env);
    let updated = unsafe {
        UpdateProcThreadAttribute(
            attributes,
            0,
            0x0002_0016,
            hpc as *const c_void,
            size_of::<Hpc>(),
            ptr::null_mut(),
            ptr::null(),
        )
    };
    let created = if updated != 0 {
        unsafe {
            CreateProcessW(
                ptr::null(),
                cmdline.as_mut_ptr(),
                ptr::null(),
                ptr::null(),
                0,
                EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT,
                environment.as_ptr().cast(),
                cwd.as_ptr(),
                &startup.StartupInfo,
                &mut process,
            )
        }
    } else {
        0
    };
    let failure = std::io::Error::last_os_error();
    unsafe {
        DeleteProcThreadAttributeList(attributes);
    }
    if created == 0 {
        return Err(format!("pseudoconsole shell launch failed: {failure}"));
    }
    unsafe {
        CloseHandle(process.hThread);
    }
    pending.0 = 0;
    Ok(NativeSession {
        console: Arc::new(NativeConsole {
            console: Mutex::new(hpc),
            process: unsafe { OwnedHandle::from_raw_handle(process.hProcess as _) },
            api,
        }),
        input: File::from(input_write),
        output: File::from(output_read),
    })
}

fn size(cols: u16, rows: u16) -> Coord {
    Coord {
        x: cols.clamp(1, i16::MAX as u16) as i16,
        y: rows.clamp(1, i16::MAX as u16) as i16,
    }
}
impl NativeConsole {
    pub fn set_size(&self, cols: u16, rows: u16) -> Result<(), String> {
        let guard = self.console.lock().unwrap_or_else(|e| e.into_inner());
        if *guard == 0 {
            return Err("pseudoconsole closed".into());
        }
        let result = unsafe { (self.api.resize)(*guard, size(cols, rows)) };
        if result < 0 {
            Err(format!("pseudoconsole resize failed ({result:#x})"))
        } else {
            Ok(())
        }
    }
    pub fn wait(&self) -> Option<i32> {
        use std::os::windows::io::AsRawHandle;
        let handle = self.process.as_raw_handle() as HANDLE;
        let mut code = 0;
        unsafe {
            WaitForSingleObject(handle, INFINITE);
        }
        (unsafe { GetExitCodeProcess(handle, &mut code) } != 0 && code != 259)
            .then_some(code as i32)
    }
    pub fn shutdown(&self) {
        use std::os::windows::io::AsRawHandle;
        let mut guard = self.console.lock().unwrap_or_else(|e| e.into_inner());
        if *guard != 0 {
            unsafe {
                TerminateProcess(self.process.as_raw_handle() as HANDLE, 1);
                (self.api.close)(*guard);
            }
            *guard = 0;
        }
    }
}
impl Drop for NativeConsole {
    fn drop(&mut self) {
        self.shutdown();
    }
}
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}
