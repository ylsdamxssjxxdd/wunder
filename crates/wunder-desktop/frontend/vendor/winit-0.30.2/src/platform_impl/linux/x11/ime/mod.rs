// Important: all XIM calls need to happen from the same thread!

mod callbacks;
mod context;
mod inner;
mod input_method;

use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use tracing::debug;

use super::{ffi, util, XConnection, XError};

use self::callbacks::*;
use self::context::ImeContext;
pub use self::context::ImeContextCreationError;
use self::inner::{close_im, ImeInner};
use self::input_method::{PotentialInputMethods, Style};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ImeEvent {
    Enabled,
    Start,
    Update(String, usize),
    End,
    Disabled,
}

pub type ImeReceiver = Receiver<ImeRequest>;
pub type ImeSender = Sender<ImeRequest>;
pub type ImeEventReceiver = Receiver<(ffi::Window, ImeEvent)>;
pub type ImeEventSender = Sender<(ffi::Window, ImeEvent)>;

/// Request to control XIM handler from the window.
pub enum ImeRequest {
    /// Set IME spot position for given `window_id`.
    Position(ffi::Window, i16, i16),

    /// Allow IME input for the given `window_id`.
    Allow(ffi::Window, bool),
}

#[derive(Debug)]
pub(crate) enum ImeCreationError {
    // Boxed to prevent large error type
    OpenFailure(Box<PotentialInputMethods>),
    SetDestroyCallbackFailed(#[allow(dead_code)] XError),
}

pub(crate) struct Ime {
    xconn: Arc<XConnection>,
    // The actual meat of this struct is boxed away, since it needs to have a fixed location in
    // memory so we can pass a pointer to it around.
    inner: Box<ImeInner>,
    // The window that currently holds X focus. A context (re)created while its window is
    // already focused must take IC focus immediately: the FocusIn that would normally call
    // XSetICFocus may have arrived before the context existed, leaving IMEs (fcitx/sogou)
    // detached after a blur/refocus cycle.
    focused_window: Option<ffi::Window>,
}

impl Ime {
    pub fn new(
        xconn: Arc<XConnection>,
        event_sender: ImeEventSender,
    ) -> Result<Self, ImeCreationError> {
        let potential_input_methods = PotentialInputMethods::new(&xconn);

        let (mut inner, client_data) = {
            let mut inner = Box::new(ImeInner::new(xconn, potential_input_methods, event_sender));
            let inner_ptr = Box::into_raw(inner);
            let client_data = inner_ptr as _;
            let destroy_callback =
                ffi::XIMCallback { client_data, callback: Some(xim_destroy_callback) };
            inner = unsafe { Box::from_raw(inner_ptr) };
            inner.destroy_callback = destroy_callback;
            (inner, client_data)
        };

        let xconn = Arc::clone(&inner.xconn);

        let input_method = inner.potential_input_methods.open_im(
            &xconn,
            Some(&|| {
                let _ = unsafe { set_instantiate_callback(&xconn, client_data) };
            }),
        );

        let is_fallback = input_method.is_fallback();
        if let Some(input_method) = input_method.ok() {
            inner.is_fallback = is_fallback;
            unsafe {
                let result = set_destroy_callback(&xconn, input_method.im, &inner)
                    .map_err(ImeCreationError::SetDestroyCallbackFailed);
                if result.is_err() {
                    let _ = close_im(&xconn, input_method.im);
                }
                result?;
            }
            inner.im = Some(input_method);
            Ok(Ime { xconn, inner, focused_window: None })
        } else {
            Err(ImeCreationError::OpenFailure(Box::new(inner.potential_input_methods)))
        }
    }

    pub fn is_destroyed(&self) -> bool {
        self.inner.is_destroyed
    }

    // This pattern is used for various methods here:
    // Ok(_) indicates that nothing went wrong internally
    // Ok(true) indicates that the action was actually performed
    // Ok(false) indicates that the action is not presently applicable
    pub fn create_context(
        &mut self,
        window: ffi::Window,
        allowed: bool,
    ) -> Result<bool, ImeContextCreationError> {
        let context = if self.is_destroyed() {
            // Create empty entry in map, so that when IME is rebuilt, this window has a context.
            None
        } else {
            let im = self.inner.im.as_ref().unwrap();
            let style = im.preedit_style;
            if allowed && matches!(style, Style::None(_)) {
                debug!("failed to create IME context with preedit support.")
            }

            let context = unsafe {
                ImeContext::new(
                    &self.inner.xconn,
                    im.im,
                    style,
                    window,
                    allowed,
                    None,
                    self.inner.event_sender.clone(),
                )?
            };

            let event = if context.is_allowed() { ImeEvent::Enabled } else { ImeEvent::Disabled };
            self.inner.event_sender.send((window, event)).expect("Failed to send enabled event");

            Some(context)
        };

        self.inner.contexts.insert(window, context);
        // If the window already holds X focus, the FocusIn that would set IC focus has
        // already passed; focus the fresh context now so the IME attaches immediately.
        if self.focused_window == Some(window) {
            if let Some(Some(context)) = self.inner.contexts.get_mut(&window) {
                if context.is_allowed() {
                    let _ = context.focus(&self.xconn);
                }
            }
        }
        Ok(!self.is_destroyed())
    }

    pub fn get_context(&self, window: ffi::Window) -> Option<ffi::XIC> {
        if self.is_destroyed() {
            return None;
        }
        if let Some(Some(context)) = self.inner.contexts.get(&window) {
            Some(context.ic)
        } else {
            None
        }
    }

    pub fn remove_context(&mut self, window: ffi::Window) -> Result<bool, XError> {
        if let Some(Some(context)) = self.inner.contexts.remove(&window) {
            unsafe {
                self.inner.destroy_ic_if_necessary(context.ic)?;
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn focus(&mut self, window: ffi::Window) -> Result<bool, XError> {
        self.focused_window = Some(window);
        if self.is_destroyed() {
            return Ok(false);
        }
        if let Some(&mut Some(ref mut context)) = self.inner.contexts.get_mut(&window) {
            if context.is_allowed() {
                context.focus(&self.xconn).map(|_| true)
            } else {
                Ok(false)
            }
        } else {
            Ok(false)
        }
    }

    pub fn unfocus(&mut self, window: ffi::Window) -> Result<bool, XError> {
        if self.focused_window == Some(window) {
            self.focused_window = None;
        }
        if self.is_destroyed() {
            return Ok(false);
        }
        if let Some(&mut Some(ref mut context)) = self.inner.contexts.get_mut(&window) {
            context.unfocus(&self.xconn).map(|_| true)
        } else {
            Ok(false)
        }
    }

    pub fn is_ime_allowed(&self, window: ffi::Window) -> bool {
        if self.is_destroyed() {
            false
        } else if let Some(Some(context)) = self.inner.contexts.get(&window) {
            context.is_allowed()
        } else {
            false
        }
    }

    pub fn send_xim_spot(&mut self, window: ffi::Window, x: i16, y: i16) {
        if self.is_destroyed() {
            return;
        }
        if let Some(&mut Some(ref mut context)) = self.inner.contexts.get_mut(&window) {
            context.set_spot(&self.xconn, x as _, y as _);
        }
    }

    pub fn set_ime_allowed(&mut self, window: ffi::Window, allowed: bool) {
        if self.is_destroyed() {
            return;
        }

        let window_focused = self.focused_window == Some(window);
        if let Some(&mut Some(ref mut context)) = self.inner.contexts.get_mut(&window) {
            if allowed == context.is_allowed() {
                if allowed && window_focused {
                    let _ = context.focus(&self.xconn);
                }
                return;
            }

            // Focus changes must not replace the XIC: fcitx keeps the selected
            // input method on that server-side context.
            context.set_allowed(allowed);
            if context.is_allowed() {
                if window_focused {
                    let _ = context.focus(&self.xconn);
                }
            } else {
                let _ = context.unfocus(&self.xconn);
            }
            let event = if context.is_allowed() { ImeEvent::Enabled } else { ImeEvent::Disabled };
            self.inner.event_sender.send((window, event)).expect("Failed to send enabled event");
            return;
        }

        let _ = self.create_context(window, allowed);
    }
}

impl Drop for Ime {
    fn drop(&mut self) {
        unsafe {
            let _ = self.inner.destroy_all_contexts_if_necessary();
            let _ = self.inner.close_im_if_necessary();
        }
    }
}
