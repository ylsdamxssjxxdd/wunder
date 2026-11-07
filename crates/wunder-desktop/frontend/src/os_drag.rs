//! OS drag-out for the working-directory tree (Windows): stages one cloud
//! file into the local workspace (the caller does that first) and hands the
//! local copy to the shell as a file data object, so the desktop or Explorer
//! receives a plain copy gesture.
//!
//! The drag loop is modal on the calling thread and must run on the UI thread
//! that owns the window: OLE tracks the pointer through the calling thread's
//! input queue. The loop pumps window messages itself, so the window stays
//! painted for the duration of the gesture. Shell owns both the data object
//! and drop source, including cross-process marshaling and default cursors.

#[cfg(windows)]
pub use imp::{drag_file, pointer_button_held};

/// Non-Windows fallback: the tree still reports an honest error instead of
/// pretending the gesture happened.
#[cfg(not(windows))]
pub fn drag_file(_path: &std::path::Path) -> Result<bool, String> {
    Err("当前系统不支持拖出到桌面".to_string())
}

/// Whether the primary mouse button is still held. A cloud file has to be
/// staged before the shell can carry it, and that round trip outlives the
/// press that started the gesture; only the caller that still sees a held
/// button may hand the file to OLE.
#[cfg(not(windows))]
pub fn pointer_button_held() -> bool {
    false
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    const DRAGDROP_S_CANCEL: i32 = 0x0004_0101;
    const S_OK: i32 = 0;
    const S_FALSE: i32 = 1;
    const DROPEFFECT_COPY: u32 = 1;

    #[link(name = "ole32")]
    extern "system" {
        fn OleInitialize(reserved: *const c_void) -> i32;
        fn OleUninitialize();
        fn CoTaskMemFree(memory: *mut c_void);
    }

    #[link(name = "shell32")]
    extern "system" {
        fn SHParseDisplayName(
            name: *const u16,
            bind_context: *mut c_void,
            item_id: *mut *mut c_void,
            attributes: u32,
            out_attributes: *mut u32,
        ) -> i32;
        fn ILClone(item: *const c_void) -> *mut c_void;
        fn ILRemoveLastID(item: *mut c_void) -> i32;
        fn ILFindLastID(item: *const c_void) -> *mut c_void;
        fn SHDoDragDrop(
            window: isize,
            data: *mut c_void,
            source: *mut c_void,
            allowed_effects: u32,
            final_effect: *mut u32,
        ) -> i32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetModuleHandleW(name: *const u16) -> isize;
        fn GetProcAddress(module: isize, name: *const u8) -> *mut c_void;
    }

    #[link(name = "user32")]
    extern "system" {
        fn GetAsyncKeyState(key: i32) -> i16;
    }

    const VK_LBUTTON: i32 = 0x01;

    pub fn pointer_button_held() -> bool {
        (unsafe { GetAsyncKeyState(VK_LBUTTON) } as u16) & 0x8000 != 0
    }

    /// Drag one local file out. Blocks on the calling UI thread until the
    /// gesture completes. Returns `true` when the target accepted a copy.
    pub fn drag_file(path: &Path) -> Result<bool, String> {
        let hr = unsafe { OleInitialize(std::ptr::null()) };
        if hr != S_OK && hr != S_FALSE {
            return Err(format!("OLE 初始化失败（0x{hr:08X}）"));
        }
        let result = drag_file_impl(path);
        unsafe { OleUninitialize() };
        result
    }

    fn drag_file_impl(path: &Path) -> Result<bool, String> {
        let data = build_shell_data_object(path)?;
        let mut effect: u32 = 0;
        // A null drop source asks Shell to provide its default implementation.
        let hr =
            unsafe { SHDoDragDrop(0, data, std::ptr::null_mut(), DROPEFFECT_COPY, &mut effect) };
        unsafe { release_unknown(data) };
        if hr < 0 {
            return Err(format!("拖拽失败（0x{hr:08X}）"));
        }
        Ok(hr != DRAGDROP_S_CANCEL && effect & DROPEFFECT_COPY != 0)
    }

    fn build_shell_data_object(path: &Path) -> Result<*mut c_void, String> {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut item = std::ptr::null_mut();
        let parsed = unsafe {
            SHParseDisplayName(
                wide.as_ptr(),
                std::ptr::null_mut(),
                &mut item,
                0,
                std::ptr::null_mut(),
            )
        };
        if parsed < 0 {
            return Err(format!("无法解析拖出文件（0x{parsed:08X}）"));
        }

        let module_name: Vec<u16> = "shell32.dll".encode_utf16().chain(Some(0)).collect();
        let address = unsafe {
            let module = GetModuleHandleW(module_name.as_ptr());
            GetProcAddress(module, 740usize as *const u8)
        };
        if address.is_null() {
            unsafe { CoTaskMemFree(item) };
            return Err("当前系统缺少文件拖拽对象接口".to_string());
        }

        let parent = unsafe { ILClone(item) };
        if parent.is_null() || unsafe { ILRemoveLastID(parent) } == 0 {
            unsafe { CoTaskMemFree(parent) };
            unsafe { CoTaskMemFree(item) };
            return Err("无法拆分拖出文件路径".to_string());
        }
        let child = unsafe { ILFindLastID(item) };
        if child.is_null() {
            unsafe { CoTaskMemFree(parent) };
            unsafe { CoTaskMemFree(item) };
            return Err("无法拆分拖出文件路径".to_string());
        }

        // SHCreateFileDataObject has no import-library name; ordinal 740 is the
        // Win7-compatible export and supplies CF_HDROP for legacy drop targets.
        type CreateFileDataObject = unsafe extern "system" fn(
            *const c_void,
            u32,
            *const *mut c_void,
            *mut c_void,
            *mut *mut c_void,
        ) -> i32;
        let create: CreateFileDataObject = unsafe { std::mem::transmute(address) };
        let mut data = std::ptr::null_mut();
        let created = unsafe {
            create(
                parent,
                1,
                std::ptr::addr_of!(child),
                std::ptr::null_mut(),
                &mut data,
            )
        };
        unsafe { CoTaskMemFree(parent) };
        unsafe { CoTaskMemFree(item) };
        if created < 0 || data.is_null() {
            return Err(format!("无法创建文件拖拽对象（0x{created:08X}）"));
        }
        Ok(data)
    }

    unsafe fn release_unknown(this: *mut c_void) {
        let vtable = *(this as *const *const *mut c_void);
        let release: unsafe extern "system" fn(*mut c_void) -> u32 =
            std::mem::transmute(*vtable.add(2));
        release(this);
    }
}
