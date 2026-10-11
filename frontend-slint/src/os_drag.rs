//! OS drag-out for the working-directory tree (Windows): stages one cloud
//! file into the local workspace (the caller does that first) and hands the
//! local copy to the shell as an OLE drag with a `CF_HDROP` data object, so
//! the desktop or Explorer receives a plain copy gesture.
//!
//! The drag loop is modal on the calling thread, so the entry point blocks
//! for the whole gesture and must run on a worker thread. Everything here is
//! self-contained: the data object, format enumerator and drop source are
//! small hand-rolled COM objects declared through raw `#[link]` externs, so
//! the drag path adds no windows-sys feature beyond what the shell dialog
//! already pulls in.

#[cfg(windows)]
pub use imp::drag_file;

/// Non-Windows fallback: the tree still reports an honest error instead of
/// pretending the gesture happened.
#[cfg(not(windows))]
pub fn drag_file(_path: &std::path::Path) -> Result<bool, String> {
    Err("当前系统不支持拖出到桌面".to_string())
}

#[cfg(windows)]
mod imp {
    use std::cell::Cell;
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    // --- Fixed Win32 values (objidl.h / oleidl.h / shellapi.h) ---
    const CF_HDROP: u16 = 15;
    const DVASPECT_CONTENT: u32 = 1;
    const TYMED_HGLOBAL: u32 = 1;
    const DATADIR_GET: u32 = 1;
    const DV_E_FORMATETC: i32 = 0x8004_0064u32 as i32;
    const DV_E_TYMED: i32 = 0x8004_0069u32 as i32;
    const E_INVALIDARG: i32 = 0x8000_0003u32 as i32;
    const E_NOINTERFACE: i32 = 0x8000_4002u32 as i32;
    const E_OUTOFMEMORY: i32 = 0x8007_000Eu32 as i32;
    const E_UNEXPECTED: i32 = 0x8000_FFFFu32 as i32;
    const DATA_S_SAMEFORMATETC: i32 = 0x0004_0003;
    const OLE_E_ADVISENOTSUPPORTED: i32 = 0x8004_0003u32 as i32;
    const DRAGDROP_S_CANCEL: i32 = 0x0004_0101;
    const DRAGDROP_S_DROP: i32 = 0x0004_0102;
    const DRAGDROP_S_USEDEFAULTCURSORS: i32 = 0x0004_0100;
    const S_OK: i32 = 0;
    const S_FALSE: i32 = 1;
    const DROPEFFECT_COPY: u32 = 1;
    const MK_LBUTTON: u32 = 0x0001;
    const MK_RBUTTON: u32 = 0x0002;
    const MK_MBUTTON: u32 = 0x0004;
    const GMEM_MOVEABLE: u32 = 0x0002;
    const GMEM_ZEROINIT: u32 = 0x0040;
    const GMEM_DDESHARE: u32 = 0x2000;
    const COINIT_APARTMENTTHREADED: u32 = 0x2;

    #[repr(C)]
    #[derive(Clone, Copy, PartialEq, Eq)]
    struct Guid {
        d0: u32,
        d1: u16,
        d2: u16,
        d3: [u8; 8],
    }

    const IID_IUNKNOWN: Guid = Guid {
        d0: 0x0000_0000,
        d1: 0x0000,
        d2: 0x0000,
        d3: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
    };
    const IID_IDATA_OBJECT: Guid = Guid {
        d0: 0x0000_010E,
        d1: 0x0000,
        d2: 0x0000,
        d3: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
    };

    /// `DROPFILES` header of a `CF_HDROP` block: the file list starts at
    /// `p_files` bytes into the allocation.
    #[repr(C)]
    struct DropFiles {
        p_files: u32,
        pt_x: i32,
        pt_y: i32,
        f_nc: i32,
        f_wide: i32,
    }

    #[repr(C)]
    struct FormatEtc {
        cf_format: u16,
        dw_aspect: u32,
        l_index: i32,
        tymed: u32,
        p_unk: *mut c_void,
    }

    #[repr(C)]
    struct StgMedium {
        tymed: u32,
        h_global: *mut c_void,
        p_unk: *mut c_void,
    }

    type QueryInterfaceFn = unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32;
    type AddRefFn = unsafe extern "system" fn(*mut c_void) -> u32;
    type ReleaseFn = unsafe extern "system" fn(*mut c_void) -> u32;
    type GetDataFn = unsafe extern "system" fn(*mut c_void, *const FormatEtc, *mut StgMedium) -> i32;
    type QueryGetDataFn = unsafe extern "system" fn(*mut c_void, *const FormatEtc) -> i32;
    type EnumFormatEtcFn =
        unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> i32;

    /// `IDataObject` vtable in declaration order: `IUnknown` (3) then
    /// GetData, GetDataHere, QueryGetData, GetCanonicalFormatEtc, SetData,
    /// EnumFormatEtc, DAdvise, DUnadvise, EnumDAdvise.
    #[repr(C)]
    struct DataObjectVtbl {
        query_interface: QueryInterfaceFn,
        add_ref: AddRefFn,
        release: ReleaseFn,
        get_data: GetDataFn,
        get_data_here: GetDataFn,
        query_get_data: QueryGetDataFn,
        get_canonical_format_etc: unsafe extern "system" fn(*mut c_void, *const FormatEtc, *mut FormatEtc) -> i32,
        set_data: unsafe extern "system" fn(*mut c_void, *const FormatEtc, *mut StgMedium, i32) -> i32,
        enum_format_etc: EnumFormatEtcFn,
        d_advise: unsafe extern "system" fn(*mut c_void, *const FormatEtc, u32, *mut c_void, *mut u32) -> i32,
        d_unadvise: unsafe extern "system" fn(*mut c_void, u32) -> i32,
        enum_d_advise: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32,
    }

    /// `IDropSource` vtable: `IUnknown` then QueryContinueDrag, GiveFeedback.
    #[repr(C)]
    struct DropSourceVtbl {
        query_interface: QueryInterfaceFn,
        add_ref: AddRefFn,
        release: ReleaseFn,
        query_continue_drag: unsafe extern "system" fn(*mut c_void, i32, u32) -> i32,
        give_feedback: unsafe extern "system" fn(*mut c_void, u32) -> i32,
    }

    /// `IEnumFORMATETC` vtable: `IUnknown` then Next, Skip, Reset, Clone.
    #[repr(C)]
    struct EnumFormatsVtbl {
        query_interface: QueryInterfaceFn,
        add_ref: AddRefFn,
        release: ReleaseFn,
        next: unsafe extern "system" fn(*mut c_void, u32, *mut FormatEtc, *mut u32) -> i32,
        skip: unsafe extern "system" fn(*mut c_void, u32) -> i32,
        reset: unsafe extern "system" fn(*mut c_void) -> i32,
        clone: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32,
    }

    /// One drag's shared state: the `CF_HDROP` block and the refcount every
    /// COM wrapper of the gesture holds. Freed when the last reference dies.
    /// The vtable pointer MUST be the first field: COM callers read it off
    /// the object pointer itself.
    #[repr(C)]
    struct DragData {
        vtbl: &'static DataObjectVtbl,
        refs: Cell<u32>,
        h_drop: *mut c_void,
    }

    /// One drag's drop source: vtable first (COM reads it off the object
    /// pointer), then the refcount the drag loop's add/release pairs drive.
    #[repr(C)]
    struct DropSource {
        vtbl: &'static DropSourceVtbl,
        refs: Cell<u32>,
    }

    /// Single-format enumerator; vtable first for the same COM reason.
    #[repr(C)]
    struct FormatEnum {
        vtbl: &'static EnumFormatsVtbl,
        refs: Cell<u32>,
        index: Cell<u32>,
    }

    #[link(name = "ole32")]
    extern "system" {
        fn CoInitializeEx(reserved: *const c_void, model: u32) -> i32;
        fn CoUninitialize();
        fn DoDragDrop(
            data: *mut c_void,
            source: *mut c_void,
            allowed_effects: u32,
            final_effect: *mut u32,
        ) -> i32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GlobalAlloc(flags: u32, bytes: usize) -> *mut c_void;
        fn GlobalLock(block: *mut c_void) -> *mut c_void;
        fn GlobalUnlock(block: *mut c_void) -> i32;
        fn GlobalFree(block: *mut c_void) -> *mut c_void;
        fn GlobalSize(block: *mut c_void) -> usize;
    }

    /// Drag one local file out. Blocks until the gesture completes, so run it
    /// on a worker thread. Returns `true` when the target accepted a copy.
    pub fn drag_file(path: &Path) -> Result<bool, String> {
        let hr = unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED) };
        if hr != S_OK && hr != S_FALSE {
            return Err(format!("OLE 初始化失败（0x{hr:08X}）"));
        }
        let result = drag_file_impl(path);
        unsafe { CoUninitialize() };
        result
    }

    fn drag_file_impl(path: &Path) -> Result<bool, String> {
        let h_drop = build_hdrop(path)?;
        // Heap objects with the vtable first: COM hands these exact pointers
        // around, and the final releases free them through Box::from_raw.
        let data = Box::into_raw(Box::new(DragData {
            vtbl: &DATA_OBJECT_VTBL,
            refs: Cell::new(1),
            h_drop,
        }));
        let source = Box::into_raw(Box::new(DropSource {
            vtbl: &DROP_SOURCE_VTBL,
            refs: Cell::new(1),
        }));
        let mut effect: u32 = 0;
        let hr = unsafe {
            DoDragDrop(
                data as *mut c_void,
                source as *mut c_void,
                DROPEFFECT_COPY,
                &mut effect,
            )
        };
        // DoDragDrop balanced its own references; drop our initial ones.
        unsafe { do_release(data as *mut c_void) };
        unsafe { ds_release(source as *mut c_void) };
        match hr {
            DRAGDROP_S_DROP => Ok(effect & DROPEFFECT_COPY != 0),
            DRAGDROP_S_CANCEL => Ok(false),
            other => Err(format!("拖拽失败（0x{other:08X}）")),
        }
    }

    /// Build one `CF_HDROP` HGLOBAL carrying the absolute path.
    fn build_hdrop(path: &Path) -> Result<*mut c_void, String> {
        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .chain(std::iter::once(0))
            .collect();
        let bytes = std::mem::size_of::<DropFiles>() + wide.len() * 2;
        let block = unsafe { GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT | GMEM_DDESHARE, bytes) };
        if block.is_null() {
            return Err("内存不足，无法构建拖拽数据".to_string());
        }
        let locked = unsafe { GlobalLock(block) };
        if locked.is_null() {
            unsafe { GlobalFree(block) };
            return Err("无法锁定拖拽数据缓冲".to_string());
        }
        unsafe {
            let header = locked as *mut DropFiles;
            std::ptr::write(
                header,
                DropFiles {
                    p_files: std::mem::size_of::<DropFiles>() as u32,
                    pt_x: 0,
                    pt_y: 0,
                    f_nc: 0,
                    f_wide: 1,
                },
            );
            let list = locked.add(std::mem::size_of::<DropFiles>()) as *mut u16;
            std::ptr::copy_nonoverlapping(wide.as_ptr(), list, wide.len());
            GlobalUnlock(block);
        }
        Ok(block)
    }

    unsafe extern "system" fn do_query_interface(
        this: *mut c_void,
        iid: *const Guid,
        out: *mut *mut c_void,
    ) -> i32 {
        if iid.is_null() || out.is_null() {
            return E_INVALIDARG;
        }
        if *iid == IID_IUNKNOWN || *iid == IID_IDATA_OBJECT {
            *out = this;
            do_add_ref(this);
            return S_OK;
        }
        *out = std::ptr::null_mut();
        E_NOINTERFACE
    }

    unsafe extern "system" fn do_add_ref(this: *mut c_void) -> u32 {
        let state = &*(this as *const DragData);
        let refs = state.refs.get() + 1;
        state.refs.set(refs);
        refs
    }

    unsafe extern "system" fn do_release(this: *mut c_void) -> u32 {
        let state = &*(this as *const DragData);
        let refs = state.refs.get().saturating_sub(1);
        state.refs.set(refs);
        if refs == 0 {
            if !state.h_drop.is_null() {
                GlobalFree(state.h_drop);
            }
            drop(Box::from_raw(this as *mut DragData));
            return 0;
        }
        refs
    }

    unsafe extern "system" fn do_get_data(
        this: *mut c_void,
        format: *const FormatEtc,
        medium: *mut StgMedium,
    ) -> i32 {
        let state = &*(this as *const DragData);
        if format.is_null() || medium.is_null() {
            return E_INVALIDARG;
        }
        let format = &*format;
        if format.cf_format != CF_HDROP || format.dw_aspect != DVASPECT_CONTENT {
            return DV_E_FORMATETC;
        }
        if format.tymed & TYMED_HGLOBAL == 0 {
            return DV_E_TYMED;
        }
        let size = GlobalSize(state.h_drop);
        let copy = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT | GMEM_DDESHARE, size);
        if copy.is_null() {
            return E_OUTOFMEMORY;
        }
        let src = GlobalLock(state.h_drop);
        let dst = GlobalLock(copy);
        if src.is_null() || dst.is_null() {
            if !src.is_null() {
                GlobalUnlock(state.h_drop);
            }
            if !dst.is_null() {
                GlobalUnlock(copy);
            }
            GlobalFree(copy);
            return E_OUTOFMEMORY;
        }
        std::ptr::copy_nonoverlapping(src as *const u8, dst as *mut u8, size);
        GlobalUnlock(state.h_drop);
        GlobalUnlock(copy);
        std::ptr::write(
            medium,
            StgMedium { tymed: TYMED_HGLOBAL, h_global: copy, p_unk: std::ptr::null_mut() },
        );
        S_OK
    }

    unsafe extern "system" fn do_get_data_here(
        _this: *mut c_void,
        _format: *const FormatEtc,
        _medium: *mut StgMedium,
    ) -> i32 {
        // CF_HDROP consumers take whole blocks through GetData.
        E_NOTIMPL_VAL
    }

    unsafe extern "system" fn do_query_get_data(
        _this: *mut c_void,
        format: *const FormatEtc,
    ) -> i32 {
        if format.is_null() {
            return E_INVALIDARG;
        }
        let format = &*format;
        if format.cf_format == CF_HDROP
            && format.dw_aspect == DVASPECT_CONTENT
            && format.tymed & TYMED_HGLOBAL != 0
        {
            return S_OK;
        }
        S_FALSE
    }

    unsafe extern "system" fn do_get_canonical(
        _this: *mut c_void,
        format: *const FormatEtc,
        out: *mut FormatEtc,
    ) -> i32 {
        if format.is_null() || out.is_null() {
            return E_INVALIDARG;
        }
        std::ptr::copy_nonoverlapping(format, out, 1);
        (*out).p_unk = std::ptr::null_mut();
        DATA_S_SAMEFORMATETC
    }

    unsafe extern "system" fn do_set_data(
        _this: *mut c_void,
        _format: *const FormatEtc,
        _medium: *mut StgMedium,
        _release: i32,
    ) -> i32 {
        // The drag carries a fixed snapshot; consumers cannot replace it.
        E_UNEXPECTED
    }

    unsafe extern "system" fn do_enum_format_etc(
        this: *mut c_void,
        direction: u32,
        out: *mut *mut c_void,
    ) -> i32 {
        if out.is_null() || direction != DATADIR_GET {
            return E_INVALIDARG;
        }
        let state = &*(this as *const DragData);
        let enumeration = Box::new(FormatEnum {
            vtbl: &ENUM_FORMATS_VTBL,
            refs: Cell::new(1),
            index: Cell::new(0),
        });
        let _ = state; // the enum lists the one format this object serves
        *out = Box::into_raw(enumeration) as *mut c_void;
        S_OK
    }

    unsafe extern "system" fn do_d_advise(
        _this: *mut c_void,
        _format: *const FormatEtc,
        _flags: u32,
        _sink: *mut c_void,
        _token: *mut u32,
    ) -> i32 {
        OLE_E_ADVISENOTSUPPORTED
    }

    unsafe extern "system" fn do_d_unadvise(_this: *mut c_void, _token: u32) -> i32 {
        OLE_E_ADVISENOTSUPPORTED
    }

    unsafe extern "system" fn do_enum_d_advise(_this: *mut c_void, out: *mut *mut c_void) -> i32 {
        if out.is_null() {
            return E_INVALIDARG;
        }
        *out = std::ptr::null_mut();
        OLE_E_ADVISENOTSUPPORTED
    }

    // `E_NOTIMPL` as a value expression (0x80004001).
    const E_NOTIMPL_VAL: i32 = 0x8000_4001u32 as i32;

    static DATA_OBJECT_VTBL: DataObjectVtbl = DataObjectVtbl {
        query_interface: do_query_interface,
        add_ref: do_add_ref,
        release: do_release,
        get_data: do_get_data,
        get_data_here: do_get_data_here,
        query_get_data: do_query_get_data,
        get_canonical_format_etc: do_get_canonical,
        set_data: do_set_data,
        enum_format_etc: do_enum_format_etc,
        d_advise: do_d_advise,
        d_unadvise: do_d_unadvise,
        enum_d_advise: do_enum_d_advise,
    };

    // --- IDropSource ---

    unsafe extern "system" fn ds_query_interface(
        this: *mut c_void,
        iid: *const Guid,
        out: *mut *mut c_void,
    ) -> i32 {
        if iid.is_null() || out.is_null() {
            return E_INVALIDARG;
        }
        if *iid == IID_IUNKNOWN {
            *out = this;
            ds_add_ref(this);
            return S_OK;
        }
        *out = std::ptr::null_mut();
        E_NOINTERFACE
    }

    unsafe extern "system" fn ds_add_ref(this: *mut c_void) -> u32 {
        let source = &*(this as *const DropSource);
        let refs = source.refs.get() + 1;
        source.refs.set(refs);
        refs
    }

    unsafe extern "system" fn ds_release(this: *mut c_void) -> u32 {
        let source = &*(this as *const DropSource);
        let refs = source.refs.get().saturating_sub(1);
        source.refs.set(refs);
        if refs == 0 {
            drop(Box::from_raw(this as *mut DropSource));
            return 0;
        }
        refs
    }

    unsafe extern "system" fn ds_query_continue_drag(
        _this: *mut c_void,
        escape: i32,
        keys: u32,
    ) -> i32 {
        if escape != 0 {
            return DRAGDROP_S_CANCEL;
        }
        if keys & (MK_LBUTTON | MK_RBUTTON | MK_MBUTTON) == 0 {
            return DRAGDROP_S_DROP;
        }
        S_OK
    }

    unsafe extern "system" fn ds_give_feedback(_this: *mut c_void, _effect: u32) -> i32 {
        DRAGDROP_S_USEDEFAULTCURSORS
    }

    static DROP_SOURCE_VTBL: DropSourceVtbl = DropSourceVtbl {
        query_interface: ds_query_interface,
        add_ref: ds_add_ref,
        release: ds_release,
        query_continue_drag: ds_query_continue_drag,
        give_feedback: ds_give_feedback,
    };

    // --- IEnumFORMATETC (single format: CF_HDROP) ---

    unsafe extern "system" fn ef_query_interface(
        this: *mut c_void,
        iid: *const Guid,
        out: *mut *mut c_void,
    ) -> i32 {
        if iid.is_null() || out.is_null() {
            return E_INVALIDARG;
        }
        if *iid == IID_IUNKNOWN {
            *out = this;
            ef_add_ref(this);
            return S_OK;
        }
        *out = std::ptr::null_mut();
        E_NOINTERFACE
    }

    unsafe extern "system" fn ef_add_ref(this: *mut c_void) -> u32 {
        let enumeration = &*(this as *const FormatEnum);
        let refs = enumeration.refs.get() + 1;
        enumeration.refs.set(refs);
        refs
    }

    unsafe extern "system" fn ef_release(this: *mut c_void) -> u32 {
        let enumeration = &*(this as *const FormatEnum);
        let refs = enumeration.refs.get().saturating_sub(1);
        enumeration.refs.set(refs);
        if refs == 0 {
            drop(Box::from_raw(this as *mut FormatEnum));
            return 0;
        }
        refs
    }

    fn single_format() -> FormatEtc {
        FormatEtc {
            cf_format: CF_HDROP,
            dw_aspect: DVASPECT_CONTENT,
            l_index: -1,
            tymed: TYMED_HGLOBAL,
            p_unk: std::ptr::null_mut(),
        }
    }

    unsafe extern "system" fn ef_next(
        this: *mut c_void,
        count: u32,
        formats: *mut FormatEtc,
        fetched: *mut u32,
    ) -> i32 {
        let enumeration = &*(this as *const FormatEnum);
        if fetched.is_null() {
            return E_INVALIDARG;
        }
        if count == 0 || formats.is_null() || enumeration.index.get() != 0 {
            *fetched = 0;
            return S_FALSE;
        }
        std::ptr::write(formats, single_format());
        enumeration.index.set(1);
        *fetched = 1;
        S_OK
    }

    unsafe extern "system" fn ef_skip(this: *mut c_void, count: u32) -> i32 {
        let enumeration = &*(this as *const FormatEnum);
        let next = enumeration.index.get() + count;
        enumeration.index.set(next.min(1));
        if enumeration.index.get() < next {
            S_FALSE
        } else {
            S_OK
        }
    }

    unsafe extern "system" fn ef_reset(this: *mut c_void) -> i32 {
        let enumeration = &*(this as *const FormatEnum);
        enumeration.index.set(0);
        S_OK
    }

    unsafe extern "system" fn ef_clone(this: *mut c_void, out: *mut *mut c_void) -> i32 {
        let enumeration = &*(this as *const FormatEnum);
        if out.is_null() {
            return E_INVALIDARG;
        }
        let copy = Box::new(FormatEnum {
            vtbl: &ENUM_FORMATS_VTBL,
            refs: Cell::new(1),
            index: Cell::new(enumeration.index.get()),
        });
        *out = Box::into_raw(copy) as *mut c_void;
        S_OK
    }

    static ENUM_FORMATS_VTBL: EnumFormatsVtbl = EnumFormatsVtbl {
        query_interface: ef_query_interface,
        add_ref: ef_add_ref,
        release: ef_release,
        next: ef_next,
        skip: ef_skip,
        reset: ef_reset,
        clone: ef_clone,
    };
}
