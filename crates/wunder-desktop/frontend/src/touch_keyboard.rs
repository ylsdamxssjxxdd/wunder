//! Windows touch keyboard (TabTip) invocation for the workbench text fields.
//!
//! Winit/Slint windows are IMM-only and never register a TSF text store, so
//! Windows never auto-raises the touch keyboard when a field is tapped; the
//! classic `set_ime_allowed` hook only toggles the IMM association. The
//! reliable trigger is the shell's `ITipInvocation::Toggle`, gated here on the
//! activation actually coming from a finger: the winit window is registered
//! for `WM_TOUCH`, so a tap arrives as `WindowEvent::Touch` while mouse and
//! precision-touchpad clicks stay `WindowEvent::MouseInput`. The winit event
//! filter (see `install_composer_popover_dismissal` in main.rs) records the
//! source before Slint dispatches the press that moves focus.

use std::{
    cell::Cell,
    time::{Duration, Instant},
};

/// What produced the most recent activation-capable input on this thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ActivationSource {
    None,
    Mouse,
    Touch,
    Keyboard,
}

thread_local! {
    static LAST_ACTIVATION: Cell<ActivationSource> = const { Cell::new(ActivationSource::None) };
    // Some touch stacks synthesize a left mouse press immediately after their
    // `WM_TOUCH` notification. Keep the touch origin briefly so that the
    // synthetic press does not turn the same tap into a mouse activation.
    static LAST_TOUCH_STARTED: Cell<Option<Instant>> = const { Cell::new(None) };
    // Windows can report one finger through both WM_TOUCH and WM_POINTER.
    // Avoid issuing Toggle twice before InputPane has reported its new state.
    static LAST_KEYBOARD_REQUEST: Cell<Option<Instant>> = const { Cell::new(None) };
}

const SYNTHETIC_MOUSE_AFTER_TOUCH_WINDOW: Duration = Duration::from_millis(250);
const TOUCH_FOCUS_WINDOW: Duration = Duration::from_millis(750);
const DUPLICATE_TOUCH_REQUEST_WINDOW: Duration = Duration::from_millis(250);

fn debug_enabled() -> bool {
    matches!(
        std::env::var("WUNDER_TOUCH_KEYBOARD_DEBUG").ok().as_deref(),
        Some("1" | "true" | "yes")
    )
}

/// Records the source of a winit window event. Called from the single winit
/// event filter before Slint dispatches the event, so a focus change caused by
/// the same press already sees the right answer.
pub(crate) fn note_winit_event(event: &winit::event::WindowEvent) {
    let source = match event {
        winit::event::WindowEvent::Touch(touch)
            if touch.phase == winit::event::TouchPhase::Started =>
        {
            LAST_TOUCH_STARTED.with(|slot| slot.set(Some(Instant::now())));
            ActivationSource::Touch
        }
        winit::event::WindowEvent::MouseInput {
            state: winit::event::ElementState::Pressed,
            ..
        } => {
            let synthetic_after_touch = LAST_TOUCH_STARTED.with(|slot| {
                slot.get()
                    .is_some_and(|started| started.elapsed() <= SYNTHETIC_MOUSE_AFTER_TOUCH_WINDOW)
            });
            if synthetic_after_touch {
                if debug_enabled() {
                    eprintln!(
                        "[touch-keyboard] preserve touch origin across synthesized mouse press"
                    );
                }
                return;
            }
            ActivationSource::Mouse
        }
        winit::event::WindowEvent::KeyboardInput { event, .. }
            if event.state == winit::event::ElementState::Pressed =>
        {
            ActivationSource::Keyboard
        }
        _ => return,
    };
    LAST_ACTIVATION.with(|slot| {
        if slot.get() != source {
            slot.set(source);
            if debug_enabled() {
                eprintln!("[touch-keyboard] activation source -> {source:?}");
            }
        }
    });
}

/// A text field was focused or pressed. Only a touch-originated activation
/// asks the system for the on-screen keyboard; mouse, pen-independent pointer
/// and keyboard focus stay quiet. Every project field reports focus through
/// the `TextFieldActivation` global, and the winit filter invokes this for a
/// finger tap after the composer editor already owns focus.
pub(crate) fn on_text_field_activated(window: &slint::Window) {
    let source = LAST_ACTIVATION.with(Cell::get);
    let recent_touch = LAST_TOUCH_STARTED.with(|slot| {
        slot.get()
            .is_some_and(|started| started.elapsed() <= TOUCH_FOCUS_WINDOW)
    });
    // A focus transition is consumed once. This prevents an old tap from
    // opening TabTip after a later programmatic focus change.
    LAST_ACTIVATION.with(|slot| slot.set(ActivationSource::None));
    LAST_TOUCH_STARTED.with(|slot| slot.set(None));
    if debug_enabled() {
        eprintln!(
            "[touch-keyboard] text field activation, last activation {source:?}, recent touch {recent_touch}"
        );
    }
    if source != ActivationSource::Touch || !recent_touch {
        return;
    }
    let duplicate_request = LAST_KEYBOARD_REQUEST.with(|slot| {
        slot.get()
            .is_some_and(|requested| requested.elapsed() <= DUPLICATE_TOUCH_REQUEST_WINDOW)
    });
    if duplicate_request {
        if debug_enabled() {
            eprintln!("[touch-keyboard] skip duplicate touch keyboard request");
        }
        return;
    }
    LAST_KEYBOARD_REQUEST.with(|slot| slot.set(Some(Instant::now())));
    show(window);
}

#[cfg(windows)]
fn show(window: &slint::Window) {
    let Some(hwnd) = native_hwnd(window) else {
        return;
    };
    imp::show_touch_keyboard(hwnd);
}

#[cfg(not(windows))]
fn show(_: &slint::Window) {}

#[cfg(windows)]
fn native_hwnd(window: &slint::Window) -> Option<isize> {
    use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let handle = window.window_handle();
    let RawWindowHandle::Win32(handle) = handle.window_handle().ok()?.as_raw() else {
        return None;
    };
    Some(handle.hwnd.get() as isize)
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;

    // --- Fixed COM values (shobjidl_core.h / objbase.h) ---
    const S_OK: i32 = 0;
    const S_FALSE: i32 = 1;
    const RPC_E_CHANGED_MODE: i32 = 0x8001_0106u32 as i32;
    const COINIT_APARTMENTTHREADED: u32 = 0x2;
    const CLSCTX_INPROC_SERVER: u32 = 0x1;
    const CLSCTX_LOCAL_SERVER: u32 = 0x4;
    const CLSCTX_INPROC_HANDLER: u32 = 0x2;

    #[repr(C)]
    #[derive(Clone, Copy, PartialEq, Eq)]
    struct Guid {
        d0: u32,
        d1: u16,
        d2: u16,
        d3: [u8; 8],
    }

    // 4CE576FA-83DC-4F88-951C-9D0782B4E376
    const CLSID_TIP_INVOCATION: Guid = Guid {
        d0: 0x4CE5_76FA,
        d1: 0x83DC,
        d2: 0x4F88,
        d3: [0x95, 0x1C, 0x9D, 0x07, 0x82, 0xB4, 0xE3, 0x76],
    };
    // 37C994E7-432B-4834-A2F7-DCE1F13B834B
    const IID_ITIP_INVOCATION: Guid = Guid {
        d0: 0x37C9_94E7,
        d1: 0x432B,
        d2: 0x4834,
        d3: [0xA2, 0xF7, 0xDC, 0xE1, 0xF1, 0x3B, 0x83, 0x4B],
    };
    // D5120AA3-46BA-44C5-822D-CA8092C1FC72
    const CLSID_FRAMEWORK_INPUT_PANE: Guid = Guid {
        d0: 0xD512_0AA3,
        d1: 0x46BA,
        d2: 0x44C5,
        d3: [0x82, 0x2D, 0xCA, 0x80, 0x92, 0xC1, 0xFC, 0x72],
    };
    // 5752238B-24F0-495A-82F1-2FD593056796
    const IID_IFRAMEWORK_INPUT_PANE: Guid = Guid {
        d0: 0x5752_238B,
        d1: 0x24F0,
        d2: 0x495A,
        d3: [0x82, 0xF1, 0x2F, 0xD5, 0x93, 0x05, 0x67, 0x96],
    };

    #[repr(C)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    type QueryInterfaceFn =
        unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32;
    type AddRefFn = unsafe extern "system" fn(*mut c_void) -> u32;
    type ReleaseFn = unsafe extern "system" fn(*mut c_void) -> u32;

    /// `ITipInvocation` vtable: `IUnknown` (3) then Toggle.
    #[repr(C)]
    struct TipInvocationVtbl {
        query_interface: QueryInterfaceFn,
        add_ref: AddRefFn,
        release: ReleaseFn,
        toggle: unsafe extern "system" fn(*mut c_void, isize) -> i32,
    }

    /// `IFrameworkInputPane` vtable: `IUnknown` (3) then Advise,
    /// AdviseWithHWND, Unadvise, Location.
    #[repr(C)]
    struct FrameworkInputPaneVtbl {
        query_interface: QueryInterfaceFn,
        add_ref: AddRefFn,
        release: ReleaseFn,
        advise: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void, *mut u32) -> i32,
        advise_with_hwnd:
            unsafe extern "system" fn(*mut c_void, isize, *mut c_void, *mut u32) -> i32,
        unadvise: unsafe extern "system" fn(*mut c_void, u32) -> i32,
        location: unsafe extern "system" fn(*mut c_void, *mut Rect) -> i32,
    }

    // Classic COM lives in ole32.dll, which ships on the Win7 baseline; only
    // the WinRT `RoActivateInstance` family moved to the banned combase.dll.
    // `ITipInvocation` and `IFrameworkInputPane` themselves exist on Win8+ —
    // on older systems CoCreateInstance simply fails and we stay silent.
    #[link(name = "ole32")]
    extern "system" {
        fn CoInitializeEx(reserved: *const c_void, model: u32) -> i32;
        fn CoUninitialize();
        fn CoCreateInstance(
            class: *const Guid,
            outer: *mut c_void,
            context: u32,
            interface: *const Guid,
            out: *mut *mut c_void,
        ) -> i32;
    }

    struct ComApartment {
        initialized_here: bool,
    }

    impl Drop for ComApartment {
        fn drop(&mut self) {
            if self.initialized_here {
                unsafe { CoUninitialize() };
            }
        }
    }

    fn initialize_com() -> Option<ComApartment> {
        let hr = unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED) };
        match hr {
            S_OK | S_FALSE => Some(ComApartment {
                initialized_here: true,
            }),
            // The UI thread can already use a different apartment model. COM
            // remains available, but this call did not add an init count.
            RPC_E_CHANGED_MODE => Some(ComApartment {
                initialized_here: false,
            }),
            _ => None,
        }
    }

    unsafe fn release(object: *mut c_void) {
        if object.is_null() {
            return;
        }
        let vtbl = *(object as *const *const IUnknownHead);
        ((*vtbl).release)(object);
    }

    /// Just the IUnknown prefix every vtable here starts with, for Release.
    #[repr(C)]
    struct IUnknownHead {
        query_interface: QueryInterfaceFn,
        add_ref: AddRefFn,
        release: ReleaseFn,
    }

    /// `Toggle` flips the pane in both directions, so an already-visible
    /// keyboard must be left alone: tapping between fields while typing would
    /// otherwise dismiss it. `Location` reports an empty rect while hidden.
    fn pane_visible() -> Option<bool> {
        let mut pane: *mut c_void = std::ptr::null_mut();
        let hr = unsafe {
            CoCreateInstance(
                &CLSID_FRAMEWORK_INPUT_PANE,
                std::ptr::null_mut(),
                CLSCTX_INPROC_SERVER | CLSCTX_LOCAL_SERVER,
                &IID_IFRAMEWORK_INPUT_PANE,
                &mut pane,
            )
        };
        if hr != S_OK || pane.is_null() {
            return None;
        }
        let mut rect = Rect {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        let vtbl = unsafe { *(pane as *const *const FrameworkInputPaneVtbl) };
        let hr = unsafe { ((*vtbl).location)(pane, &mut rect) };
        unsafe { release(pane) };
        if hr != S_OK {
            return None;
        }
        Some(rect.right > rect.left && rect.bottom > rect.top)
    }

    pub fn show_touch_keyboard(hwnd: isize) {
        let Some(_com) = initialize_com() else {
            return;
        };
        match pane_visible() {
            Some(true) => {
                if super::debug_enabled() {
                    eprintln!("[touch-keyboard] pane already visible, skip toggle");
                }
                return;
            }
            Some(false) | None => {}
        }
        let mut tip: *mut c_void = std::ptr::null_mut();
        let hr = unsafe {
            CoCreateInstance(
                &CLSID_TIP_INVOCATION,
                std::ptr::null_mut(),
                CLSCTX_INPROC_HANDLER | CLSCTX_LOCAL_SERVER,
                &IID_ITIP_INVOCATION,
                &mut tip,
            )
        };
        if hr != S_OK || tip.is_null() {
            if super::debug_enabled() {
                eprintln!("[touch-keyboard] CoCreateInstance(TipInvocation) failed 0x{hr:08X}");
            }
            return;
        }
        // Toggle needs the window that owns the focused text field. Passing
        // the desktop window works only inconsistently in tablet mode; using
        // Slint's actual Winit HWND gives TabTip the current input target.
        let vtbl = unsafe { *(tip as *const *const TipInvocationVtbl) };
        let hr = unsafe { ((*vtbl).toggle)(tip, hwnd) };
        unsafe { release(tip) };
        if super::debug_enabled() {
            eprintln!("[touch-keyboard] ITipInvocation::Toggle -> 0x{hr:08X}");
        }
    }
}
