//! One shutdown path for the Slint window, tray menu and process teardown.

pub fn request_exit() {
    crate::tray::uninstall();
    crate::hotkey::stop();
    let _ = slint::quit_event_loop();
}
