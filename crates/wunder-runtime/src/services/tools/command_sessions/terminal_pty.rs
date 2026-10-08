//! winpty console: a real terminal device for the desktop shell on Windows.
//!
//! The pipe path hands a shell three pipes, which is enough for line output but
//! not for a program that asks what it is attached to: no echo of what you type,
//! no full-screen redraw, no Ctrl-C, and tools like `git` or `docker` drop their
//! progress because `isatty` says no. winpty gives a program the console it
//! expects on every Windows version back to 7 (ConPTY needs 1809+), and it ships
//! inside Git for Windows, so the supplement package already carries it.
//!
//! It is loaded at run time rather than linked: an install without the package,
//! or a 64-bit process facing the 32-bit winpty that ships with Git, must still
//! start and fall back to pipes. The API below is winpty 0.4.3's C surface, which
//! is cdecl on x86 (hence `extern "C"`, not `extern "system"`), and its data
//! channels are named pipes the caller opens by name, so ordinary blocking file
//! handles carry the traffic. `WINPTY_SPAWN_FLAG_AUTO_SHUTDOWN` makes the agent
//! kill the shell when the console goes away, so dropping [`PtyInner`] is the
//! termination path and no explicit process kill is needed.

use std::ffi::{c_void, CStr};
use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;
use std::ptr;
use std::sync::{Arc, Mutex, OnceLock};
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE, HMODULE};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE};

/// Ask the agent to translate console attributes into colour escapes, so a
/// program that paints with console calls still looks painted here.
const WINPTY_FLAG_COLOR_ESCAPES: u64 = 0x4;
/// The agent kills the shell when the console handle goes away.
const WINPTY_SPAWN_FLAG_AUTO_SHUTDOWN: u64 = 0x1;
const WINPTY_MOUSE_MODE_NONE: i32 = 0;
/// Reported by `GetExitCodeProcess` while a process is still running.
const STILL_ACTIVE: u32 = 259;
/// `ERROR_BAD_EXE_FORMAT`: the library is there but not for this architecture.
const ERROR_BAD_EXE_FORMAT: u32 = 193;
/// `GetExitCodeProcess` and friends report no handle with `-1`.
const INVALID_HANDLE: HANDLE = -1;

type WinptyError = *mut c_void;

/// Everything the console needs out of `winpty.dll`. Resolved by name once the
/// library is loaded, so a missing export is a fallback rather than a crash.
struct Api {
    config_new: unsafe extern "C" fn(u64, *mut WinptyError) -> *mut c_void,
    config_free: unsafe extern "C" fn(*mut c_void),
    config_set_initial_size: unsafe extern "C" fn(*mut c_void, i32, i32),
    config_set_mouse_mode: unsafe extern "C" fn(*mut c_void, i32),
    open: unsafe extern "C" fn(*const c_void, *mut WinptyError) -> *mut c_void,
    spawn_config_new: unsafe extern "C" fn(
        u64,
        *const u16,
        *const u16,
        *const u16,
        *const u16,
        *mut WinptyError,
    ) -> *mut c_void,
    spawn_config_free: unsafe extern "C" fn(*mut c_void),
    spawn: unsafe extern "C" fn(
        *mut c_void,
        *const c_void,
        *mut HANDLE,
        *mut HANDLE,
        *mut u32,
        *mut WinptyError,
    ) -> i32,
    set_size: unsafe extern "C" fn(*mut c_void, i32, i32, *mut WinptyError) -> i32,
    conin_name: unsafe extern "C" fn(*mut c_void) -> *const u16,
    conout_name: unsafe extern "C" fn(*mut c_void) -> *const u16,
    free: unsafe extern "C" fn(*mut c_void),
    error_code: unsafe extern "C" fn(WinptyError) -> u32,
    error_msg: unsafe extern "C" fn(WinptyError) -> *const u16,
    error_free: unsafe extern "C" fn(WinptyError),
}

/// The loaded library is kept for the life of the process: its function pointers
/// are handed to console sessions on other threads, and unloading while one is
/// reading would fault. The module is small and mapped once.
static API: OnceLock<Arc<Api>> = OnceLock::new();
/// Keeps two terminals starting at once from both paying for the load, and lets
/// a failed load be retried once the supplement package arrives.
static LOAD_LOCK: Mutex<()> = Mutex::new(());

/// What it takes to open a console for a shell.
pub struct PtyRequest<'a> {
    /// Path to `winpty.dll`; `winpty-agent.exe` must sit beside it.
    pub dll: &'a Path,
    /// Full command line, as `CreateProcessW` expects it (quoted program first).
    pub cmdline: &'a str,
    pub cwd: Option<&'a Path>,
    /// Environment block for the shell; `None` inherits this process.
    pub env: Option<&'a [(String, String)]>,
    pub cols: u16,
    pub rows: u16,
}

/// A live console: the winpty handle plus the shell's process. Shared behind an
/// `Arc` so the viewport can be resized while another thread reads the output.
pub struct PtyInner {
    /// winpty is not thread safe, and a cancel frees the console while a resize
    /// may be about to use it, so every call takes this lock and a freed console
    /// stays a null handle rather than a dangling pointer.
    winpty: Mutex<WinptyHandle>,
    process: HANDLE,
    api: Arc<Api>,
}

/// The raw library handle, which only travels between threads under the lock.
struct WinptyHandle(*mut c_void);

unsafe impl Send for WinptyHandle {}

unsafe impl Send for PtyInner {}
unsafe impl Sync for PtyInner {}

/// A console plus its data channels, taken out of the agent.
pub struct PtySession {
    pub console: Arc<PtyInner>,
    /// Bytes typed by the user go here; the shell echoes them back out.
    pub input: File,
    /// Everything the shell draws.
    pub output: File,
}

/// Start a shell inside a console.
pub fn spawn(request: &PtyRequest<'_>) -> Result<PtySession, String> {
    let api = load_api(request.dll)?;
    let mut failure: WinptyError = ptr::null_mut();
    let config = unsafe { (api.config_new)(WINPTY_FLAG_COLOR_ESCAPES, &mut failure) };
    if config.is_null() {
        return Err(describe(&api, &mut failure));
    }
    unsafe {
        (api.config_set_mouse_mode)(config, WINPTY_MOUSE_MODE_NONE);
        (api.config_set_initial_size)(config, i32::from(request.cols), i32::from(request.rows));
    }
    let winpty = unsafe { (api.open)(config, &mut failure) };
    unsafe { (api.config_free)(config) };
    if winpty.is_null() {
        return Err(describe(&api, &mut failure));
    }
    let cmdline = wide(request.cmdline);
    let cwd = request.cwd.map(|path| wide(&path.to_string_lossy()));
    let env = request.env.map(env_block);
    let spawn_config = unsafe {
        (api.spawn_config_new)(
            WINPTY_SPAWN_FLAG_AUTO_SHUTDOWN,
            ptr::null(),
            cmdline.as_ptr(),
            cwd.as_ref().map_or(ptr::null(), Vec::as_ptr),
            env.as_ref().map_or(ptr::null(), Vec::as_ptr),
            &mut failure,
        )
    };
    if spawn_config.is_null() {
        unsafe { (api.free)(winpty) };
        return Err(describe(&api, &mut failure));
    }
    let mut process: HANDLE = 0;
    let mut thread: HANDLE = 0;
    let mut create_error = 0u32;
    let spawned = unsafe {
        (api.spawn)(
            winpty,
            spawn_config,
            &mut process,
            &mut thread,
            &mut create_error,
            &mut failure,
        )
    };
    unsafe { (api.spawn_config_free)(spawn_config) };
    if spawned == 0 {
        unsafe { (api.free)(winpty) };
        let reason = describe(&api, &mut failure);
        return Err(if create_error != 0 {
            format!("{reason} (CreateProcess error {create_error})")
        } else {
            reason
        });
    }
    if thread != 0 && thread != INVALID_HANDLE {
        unsafe { CloseHandle(thread) };
    }
    let console = Arc::new(PtyInner {
        winpty: Mutex::new(WinptyHandle(winpty)),
        process,
        api: Arc::clone(&api),
    });
    // The agent's channels are named pipes, so an ordinary file handle carries
    // them: blocking reads, and a write that waits for the shell to take it.
    let input = open_pipe(unsafe { (api.conin_name)(winpty) }, false);
    let output = open_pipe(unsafe { (api.conout_name)(winpty) }, true);
    match (input, output) {
        (Ok(input), Ok(output)) => Ok(PtySession {
            console,
            input,
            output,
        }),
        (Err(error), _) | (_, Err(error)) => {
            // Dropping the console takes the agent and, with it, the shell.
            Err(format!("winpty data pipes could not be opened: {error}"))
        }
    }
}

impl PtyInner {
    /// Tell the shell how big its window is. The agent reflows the console, so
    /// the next prompt and any full-screen redraw come out at the new width.
    pub fn set_size(&self, cols: u16, rows: u16) -> Result<(), String> {
        let mut failure: WinptyError = ptr::null_mut();
        let guard = lock(&self.winpty);
        let console = guard.0;
        if console.is_null() {
            return Err("the console is already closed".to_string());
        }
        let ok =
            unsafe { (self.api.set_size)(console, i32::from(cols), i32::from(rows), &mut failure) };
        if ok == 0 {
            return Err(describe(&self.api, &mut failure));
        }
        Ok(())
    }

    /// Tear the console down right now: the agent exits and, with the shutdown
    /// flag it was spawned under, takes the shell with it. Calling it again does
    /// nothing, so a cancel and the eventual drop can both ask for it.
    pub fn shutdown(&self) {
        let mut guard = lock(&self.winpty);
        if guard.0.is_null() {
            return;
        }
        unsafe { (self.api.free)(guard.0) };
        guard.0 = ptr::null_mut();
    }

    /// Block until the shell exits, then report its code.
    pub fn wait(&self) -> Option<i32> {
        if self.process == 0 || self.process == INVALID_HANDLE {
            return None;
        }
        unsafe {
            WaitForSingleObject(self.process, INFINITE);
            exit_code(self.process)
        }
    }
}

impl Drop for PtyInner {
    fn drop(&mut self) {
        // Freeing the console closes the agent, and the auto-shutdown flag makes
        // the agent take the shell with it.
        self.shutdown();
        unsafe {
            if self.process != 0 && self.process != INVALID_HANDLE {
                CloseHandle(self.process);
            }
        }
    }
}

/// Lock a console guard, keeping a panic elsewhere from poisoning the terminal.
fn lock(handle: &Mutex<WinptyHandle>) -> std::sync::MutexGuard<'_, WinptyHandle> {
    handle
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

unsafe fn exit_code(process: HANDLE) -> Option<i32> {
    let mut code = 0u32;
    (GetExitCodeProcess(process, &mut code) != 0 && code != STILL_ACTIVE).then_some(code as i32)
}

/// Load and resolve the library, or explain precisely why the console is not
/// available. Only the exports actually used are required.
fn load_api(dll: &Path) -> Result<Arc<Api>, String> {
    if let Some(api) = API.get() {
        return Ok(Arc::clone(api));
    }
    let _guard = LOAD_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(api) = API.get() {
        return Ok(Arc::clone(api));
    }
    // An absolute path, so the agent and any sibling DLLs resolve from the
    // package directory rather than from this process's working directory.
    let module = unsafe { LoadLibraryW(wide(&dll.to_string_lossy()).as_ptr()) };
    if module == 0 {
        return Err(load_failure(dll));
    }
    let api = Arc::new(unsafe {
        Api {
            config_new: symbol(module, c"winpty_config_new")?,
            config_free: symbol(module, c"winpty_config_free")?,
            config_set_initial_size: symbol(module, c"winpty_config_set_initial_size")?,
            config_set_mouse_mode: symbol(module, c"winpty_config_set_mouse_mode")?,
            open: symbol(module, c"winpty_open")?,
            spawn_config_new: symbol(module, c"winpty_spawn_config_new")?,
            spawn_config_free: symbol(module, c"winpty_spawn_config_free")?,
            spawn: symbol(module, c"winpty_spawn")?,
            set_size: symbol(module, c"winpty_set_size")?,
            conin_name: symbol(module, c"winpty_conin_name")?,
            conout_name: symbol(module, c"winpty_conout_name")?,
            free: symbol(module, c"winpty_free")?,
            error_code: symbol(module, c"winpty_error_code")?,
            error_msg: symbol(module, c"winpty_error_msg")?,
            error_free: symbol(module, c"winpty_error_free")?,
        }
    });
    let _ = API.set(Arc::clone(&api));
    Ok(api)
}

/// A failed load is usually an architecture mismatch: the winpty that ships with
/// Git for Windows is 32-bit, and a 64-bit process cannot map it.
fn load_failure(dll: &Path) -> String {
    let code = unsafe { GetLastError() };
    if code == ERROR_BAD_EXE_FORMAT {
        return format!(
            "winpty at {} is a different architecture than this process",
            dll.display()
        );
    }
    format!(
        "failed to load winpty at {}: windows error {code}",
        dll.display()
    )
}

unsafe fn symbol<T>(module: HMODULE, name: &CStr) -> Result<T, String> {
    // Export names are ANSI, and `GetProcAddress` reads until a NUL, which is
    // why the lookups below hand it a `CStr` literal rather than a `&str`.
    // `c_char` is signed on the GNU targets, but `PCSTR` is not.
    let address = GetProcAddress(module, name.as_ptr() as *const u8);
    let address = match address {
        Some(address) => address as *const c_void as usize,
        None => {
            return Err(format!(
                "winpty.dll does not export {}",
                name.to_str().unwrap_or("?")
            ))
        }
    };
    // Every entry point here is a plain function pointer of the same width.
    Ok(unsafe { std::mem::transmute_copy::<usize, T>(&address) })
}

/// Turn a winpty error into text and clear it, so it cannot be reused.
fn describe(api: &Api, failure: &mut WinptyError) -> String {
    if failure.is_null() {
        return "winpty failed without an error".to_string();
    }
    unsafe {
        let code = (api.error_code)(*failure);
        let message = from_wide((api.error_msg)(*failure));
        (api.error_free)(*failure);
        *failure = ptr::null_mut();
        format!("{} ({message})", error_text(code))
    }
}

/// winpty's error codes, named.
fn error_text(code: u32) -> &'static str {
    match code {
        0 => "no error",
        1 => "out of memory",
        2 => "the shell could not be started",
        3 => "lost contact with the console agent",
        4 => "winpty-agent.exe was not found beside winpty.dll",
        5 => "unspecified winpty error",
        6 => "the console agent died",
        7 => "the console agent did not answer in time",
        8 => "the console agent could not be created",
        _ => "unknown winpty error",
    }
}

/// A `CreateProcessW` environment block: `KEY=VALUE` pairs, NUL separated, with
/// a trailing NUL for the end of the block.
fn env_block(entries: &[(String, String)]) -> Vec<u16> {
    let mut block = String::new();
    for (key, value) in entries {
        block.push_str(key);
        block.push('=');
        block.push_str(value);
        block.push('\0');
    }
    block.push('\0');
    wide(&block)
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

/// Open one end of the agent's named-pipe pair.
fn open_pipe(name: *const u16, read_only: bool) -> io::Result<File> {
    let path = from_wide(name);
    if path.is_empty() {
        return Err(io::Error::from(io::ErrorKind::BrokenPipe));
    }
    if read_only {
        File::open(path)
    } else {
        OpenOptions::new().write(true).open(path)
    }
}

fn from_wide(value: *const u16) -> String {
    if value.is_null() {
        return String::new();
    }
    let mut length = 0usize;
    while unsafe { *value.add(length) } != 0 {
        length += 1;
    }
    String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(value, length) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_read_as_reasons() {
        assert_eq!(
            error_text(4),
            "winpty-agent.exe was not found beside winpty.dll"
        );
        assert_eq!(error_text(7), "the console agent did not answer in time");
        assert_eq!(error_text(99), "unknown winpty error");
    }

    #[test]
    fn environment_blocks_terminate_every_entry_and_the_block() {
        let entries = vec![
            ("PATH".to_string(), "C:\\bin".to_string()),
            ("HOME".to_string(), "C:\\h".to_string()),
        ];
        let block = env_block(&entries);
        // One NUL ends each pair, and a second one ends the block; the last
        // element is just the wide-string terminator `wide` appends.
        let text = String::from_utf16(&block[..block.len() - 1]).unwrap();
        assert_eq!(text, "PATH=C:\\bin\0HOME=C:\\h\0\0");
    }

    #[test]
    fn a_missing_library_is_reported_not_panicked() {
        let error = load_api(Path::new("C:\\definitely-not-here\\winpty.dll"))
            .err()
            .expect("a missing library must not load");
        assert!(error.contains("winpty"), "{error}");
    }
}
