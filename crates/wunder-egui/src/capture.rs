//! Optional deterministic screenshot capture for visual review builds.
use egui::{Context, Event, Id, ViewportCommand};

pub fn tick(ctx: &Context) {
    let Some(path) = std::env::var_os("WUNDER_EGUI_CAPTURE_TO") else {
        return;
    };
    let screenshot = ctx.input(|input| {
        input.events.iter().find_map(|event| {
            if let Event::Screenshot { image, .. } = event {
                Some(image.clone())
            } else {
                None
            }
        })
    });
    if let Some(image) = screenshot {
        let bytes: Vec<u8> = image
            .pixels
            .iter()
            .flat_map(|pixel| pixel.to_array())
            .collect();
        if let Err(error) = image::save_buffer(
            path,
            &bytes,
            image.width() as u32,
            image.height() as u32,
            image::ColorType::Rgba8,
        ) {
            eprintln!("visual capture failed: {error}");
        }
        ctx.send_viewport_cmd(ViewportCommand::Close);
        return;
    }
    let frame = ctx.data_mut(|data| {
        let frame = data
            .get_temp::<u8>(Id::new("visual-capture-frame"))
            .unwrap_or(0)
            .saturating_add(1);
        data.insert_temp(Id::new("visual-capture-frame"), frame);
        frame
    });
    if frame == 8 {
        ctx.send_viewport_cmd(ViewportCommand::Screenshot(Default::default()));
    }
    ctx.request_repaint();
}
