// Copyright © SixtyFPS GmbH <info@slint.dev>
// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0

//! Path rendering support for the software renderer using zeno

use super::draw_functions::{PremultipliedRgbaColor, TargetPixel};
use super::{PhysicalRect, PhysicalRegion};
use alloc::vec;
use alloc::vec::Vec;
use zeno::{Cap, Fill, Join, Mask, Stroke, Style};

pub use zeno::Command;

/// Convert Slint's PathDataIterator to zeno's Command format
pub fn convert_path_data_to_zeno(
    path_data: i_slint_core::graphics::PathDataIterator,
    rotation: crate::RotationInfo,
    scale_factor: i_slint_core::lengths::ScaleFactor,
    offset: euclid::Vector2D<f32, i_slint_core::lengths::PhysicalPx>,
) -> Vec<Command> {
    use crate::Transform as _;
    use i_slint_core::lengths::LogicalPoint;
    use lyon_path::Event;
    let mut commands = Vec::new();

    let convert_point = |p| {
        let p = (LogicalPoint::from_untyped(p) * scale_factor + offset).transformed(rotation);
        zeno::Point::new(p.x, p.y)
    };

    for event in path_data.iter() {
        match event {
            Event::Begin { at } => {
                commands.push(Command::MoveTo(convert_point(at)));
            }
            Event::Line { to, .. } => {
                commands.push(Command::LineTo(convert_point(to)));
            }
            Event::Quadratic { ctrl, to, .. } => {
                commands.push(Command::QuadTo(convert_point(ctrl), convert_point(to)));
            }
            Event::Cubic {
                ctrl1, ctrl2, to, ..
            } => {
                commands.push(Command::CurveTo(
                    convert_point(ctrl1),
                    convert_point(ctrl2),
                    convert_point(to),
                ));
            }
            Event::End { close, .. } => {
                if close {
                    commands.push(Command::Close);
                }
            }
        }
    }

    commands
}

/// Common rendering logic for both filled and stroked paths
fn render_path_with_style<T: TargetPixel>(
    commands: &[Command],
    path_geometry: &PhysicalRect,
    clip_geometry: &PhysicalRect,
    dirty_region: &PhysicalRegion,
    color: PremultipliedRgbaColor,
    style: zeno::Style,
    buffer: &mut impl crate::target_pixel_buffer::TargetPixelBuffer<TargetPixel = T>,
) {
    let Some(path_clip) = path_geometry.intersection(clip_geometry) else {
        return;
    };
    let buffer_clip = PhysicalRect::new(
        euclid::point2(0, 0),
        euclid::size2(buffer.line_slice(0).len() as i16, buffer.num_lines() as i16),
    );
    let Some(path_clip) = path_clip.intersection(&buffer_clip) else {
        return;
    };

    // Normalize Slint's overlapping dirty rectangles into a disjoint set so
    // translucent paths cannot blend an overlap twice in one frame.
    for dirty_rect in dirty_region.iter_disjoint_box() {
        let Some(render_rect) = path_clip.intersection(&dirty_rect.to_rect()) else {
            continue;
        };
        rasterize_path_region(
            commands,
            path_geometry,
            render_rect,
            color,
            style,
            buffer,
        );
    }
}

fn rasterize_path_region<T: TargetPixel>(
    commands: &[Command],
    path_geometry: &PhysicalRect,
    render_rect: PhysicalRect,
    color: PremultipliedRgbaColor,
    style: zeno::Style,
    buffer: &mut impl crate::target_pixel_buffer::TargetPixelBuffer<TargetPixel = T>,
) {
    let width = render_rect.size.width as usize;
    let height = render_rect.size.height as usize;
    debug_assert!(width > 0 && height > 0);

    // The mask is local to `render_rect`, so shift the screen-space path
    // origin into that local coordinate system.
    let transform = zeno::Transform::translation(
        (path_geometry.origin.x - render_rect.origin.x) as f32,
        (path_geometry.origin.y - render_rect.origin.y) as f32,
    );
    let mut mask_buffer = vec![0u8; width * height];
    Mask::new(commands)
        .size(width as u32, height as u32)
        .style(style)
        .transform(Some(transform))
        .render_into(&mut mask_buffer, None);

    for (mask_y, mask_row) in mask_buffer.chunks_exact(width).enumerate() {
        let screen_y = render_rect.origin.y as usize + mask_y;
        let screen_x = render_rect.origin.x as usize;
        let line = buffer.line_slice(screen_y);
        for (pixel, coverage) in line[screen_x..screen_x + width].iter_mut().zip(mask_row) {
            if *coverage > 0 {
                let coverage_factor = *coverage as u16;
                let alpha_color = PremultipliedRgbaColor {
                    red: ((color.red as u16 * coverage_factor) / 255) as u8,
                    green: ((color.green as u16 * coverage_factor) / 255) as u8,
                    blue: ((color.blue as u16 * coverage_factor) / 255) as u8,
                    alpha: ((color.alpha as u16 * coverage_factor) / 255) as u8,
                };
                T::blend(pixel, alpha_color);
            }
        }
    }
}

/// Render a filled path
///
/// * `commands` - The path commands to render
/// * `path_geometry` - The full bounding box of the path in screen coordinates
/// * `clip_geometry` - The clipped region where the path should be rendered (intersection of path and clip)
/// * `color` - The color to render the path
/// * `buffer` - The target pixel buffer
pub fn render_filled_path<T: TargetPixel>(
    commands: &[Command],
    path_geometry: &PhysicalRect,
    clip_geometry: &PhysicalRect,
    dirty_region: &PhysicalRegion,
    color: PremultipliedRgbaColor,
    buffer: &mut impl crate::target_pixel_buffer::TargetPixelBuffer<TargetPixel = T>,
) {
    render_path_with_style(
        commands,
        path_geometry,
        clip_geometry,
        dirty_region,
        color,
        zeno::Style::Fill(Fill::NonZero),
        buffer,
    );
}

/// Render a stroked path
///
/// * `commands` - The path commands to render
/// * `path_geometry` - The full bounding box of the path in screen coordinates
/// * `clip_geometry` - The clipped region where the path should be rendered (intersection of path and clip)
/// * `color` - The color to render the path
/// * `stroke_width` - The width of the stroke
/// * `buffer` - The target pixel buffer
pub fn render_stroked_path<T: TargetPixel>(
    commands: &[Command],
    path_geometry: &PhysicalRect,
    clip_geometry: &PhysicalRect,
    dirty_region: &PhysicalRegion,
    color: PremultipliedRgbaColor,
    stroke_width: f32,
    stroke_line_cap: i_slint_core::items::LineCap,
    stroke_line_join: i_slint_core::items::LineJoin,
    stroke_miter_limit: f32,
    buffer: &mut impl crate::target_pixel_buffer::TargetPixelBuffer<TargetPixel = T>,
) {
    let mut stroke = Stroke::new(stroke_width);
    stroke
        .cap(match stroke_line_cap {
            i_slint_core::items::LineCap::Round => Cap::Round,
            i_slint_core::items::LineCap::Square => Cap::Square,
            i_slint_core::items::LineCap::Butt | _ => Cap::Butt,
        })
        .join(match stroke_line_join {
            i_slint_core::items::LineJoin::Round => Join::Round,
            i_slint_core::items::LineJoin::Bevel => Join::Bevel,
            i_slint_core::items::LineJoin::Miter | _ => Join::Miter,
        })
        .miter_limit(stroke_miter_limit);
    let style = Style::Stroke(stroke);
    render_path_with_style(
        commands,
        path_geometry,
        clip_geometry,
        dirty_region,
        color,
        style,
        buffer,
    );
}
