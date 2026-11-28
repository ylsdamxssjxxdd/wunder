use super::open;
use crate::MobileWindow;
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, PointerEventButton, WindowAdapter, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, PhysicalSize, PlatformError};
use std::{path::Path, rc::Rc};

struct TestPlatform;

impl Platform for TestPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer))
    }
}

fn click(ui: &MobileWindow, x: f32, y: f32) {
    let position = LogicalPosition::new(x, y);
    let button = PointerEventButton::Left;
    ui.window()
        .dispatch_event(WindowEvent::PointerPressed { position, button });
    ui.window()
        .dispatch_event(WindowEvent::PointerReleased { position, button });
}

#[test]
fn context_map_taps_open_details_and_return_to_map() {
    slint::platform::set_platform(Box::new(TestPlatform)).unwrap();
    let mut fonts = slint::fontique_012::shared_collection();
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../wunder-desktop/frontend/assets/fonts/MiSans-Regular.ttf");
    let families = fonts.register_fonts(std::fs::read(path).unwrap().into(), None);
    for family in [
        fontique::GenericFamily::SansSerif,
        fontique::GenericFamily::Monospace,
    ] {
        fonts.set_generic_families(family, families.iter().map(|(id, _)| *id));
    }
    let ui = MobileWindow::new().unwrap();
    ui.window().set_size(PhysicalSize::new(360, 780));
    open(&ui, "context");
    ui.show().unwrap();
    slint::platform::update_timers_and_animations();

    click(&ui, 180.0, 530.0);
    assert!(!ui.get_context_detail_open());

    click(&ui, 55.0, 280.0);
    assert!(ui.get_context_detail_open());
    assert_eq!(ui.get_context_detail_kind(), "item");
    assert_eq!(ui.get_context_detail_name(), "sample_reader");
    click(&ui, 337.0, 27.0);
    assert!(!ui.get_context_detail_open());
    assert!(ui.get_context_window_open());

    click(&ui, 146.0, 377.0);
    assert!(ui.get_context_detail_open());
    assert_eq!(ui.get_context_detail_kind(), "part");
    assert_eq!(ui.get_context_detail_slot(), 2);
    click(&ui, 337.0, 27.0);

    click(&ui, 96.0, 746.0);
    assert!(ui.get_context_detail_open());
    assert_eq!(ui.get_context_detail_kind(), "part");
    assert_eq!(ui.get_context_detail_slot(), 2);
    click(&ui, 60.0, 100.0);
    assert_eq!(ui.get_context_detail_row_open(), 0);
    click(&ui, 60.0, 100.0);
    assert_eq!(ui.get_context_detail_row_open(), -1);

    click(&ui, 337.0, 27.0);
    click(&ui, 27.0, 27.0);
    assert!(!ui.get_context_window_open());
}
