//! Small cached Markdown projection: paragraphs, headings, lists, inline code
//! and fenced code. The original message remains untouched for copying.

use egui::{text::LayoutJob, Color32, FontId, Frame, Label, Margin, RichText, TextFormat, Ui};
use std::ops::Range;
use std::sync::Arc;

struct InlineText {
    job: LayoutJob,
    emphasis: Vec<Range<usize>>,
}

fn inline_job(text: &str, size: f32, color: Color32) -> InlineText {
    let mut job = LayoutJob::default();
    let mut emphasis = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let next = [
            rest.find("**").map(|p| (p, "**")),
            rest.find('`').map(|p| (p, "`")),
        ]
        .into_iter()
        .flatten()
        .min_by_key(|(p, _)| *p);
        let Some((at, marker)) = next else {
            job.append(rest, 0.0, format(size, color, false));
            break;
        };
        job.append(&rest[..at], 0.0, format(size, color, false));
        let tail = &rest[at + marker.len()..];
        let Some(end) = tail.find(marker) else {
            job.append(&rest[at..], 0.0, format(size, color, false));
            break;
        };
        let mut text_format = format(size, color, marker == "`");
        if marker == "**" {
            let start = job.text.chars().count();
            emphasis.push(start..start + tail[..end].chars().count());
        } else {
            text_format.background = crate::theme::tokens().code_inline;
        }
        job.append(&tail[..end], 0.0, text_format);
        rest = &tail[end + marker.len()..];
    }
    InlineText { job, emphasis }
}

/// The shared YaHei collection contains regular faces only. A small glyph
/// overdraw supplies emphasis without embedding another CJK font. The normal
/// egui label still owns selection, wrapping and copying.
fn show_inline(ui: &mut Ui, text: InlineText, color: Color32) {
    if text.emphasis.is_empty() {
        ui.add(Label::new(text.job).wrap().selectable(true));
        return;
    }
    let (pos, galley, response) = Label::new(text.job)
        .wrap()
        .selectable(true)
        .layout_in_ui(ui);
    if ui.is_rect_visible(response.rect) {
        let pixels_per_point = ui.pixels_per_point();
        let mut overlay = (*galley).clone();
        overlay.num_vertices = 0;
        overlay.num_indices = 0;
        let mut ranges = text.emphasis.iter().peekable();
        let mut char_index = 0;
        for row in &mut overlay.rows {
            let mut mesh = egui::epaint::Mesh::default();
            for glyph in &row.glyphs {
                while ranges.peek().is_some_and(|range| range.end <= char_index) {
                    ranges.next();
                }
                if ranges
                    .peek()
                    .is_some_and(|range| range.contains(&char_index))
                    && !glyph.uv_rect.is_nothing()
                {
                    let glyph_pos = glyph.pos + glyph.uv_rect.offset;
                    let origin = egui::pos2(
                        (glyph_pos.x * pixels_per_point).round() / pixels_per_point,
                        (glyph_pos.y * pixels_per_point).round() / pixels_per_point,
                    );
                    let rect = egui::Rect::from_min_size(origin, glyph.uv_rect.size);
                    let uv = egui::Rect::from_min_max(
                        egui::pos2(glyph.uv_rect.min[0] as f32, glyph.uv_rect.min[1] as f32),
                        egui::pos2(glyph.uv_rect.max[0] as f32, glyph.uv_rect.max[1] as f32),
                    );
                    for offset in [egui::vec2(-0.35, 0.0), egui::vec2(0.35, 0.0)] {
                        mesh.add_rect_with_uv(rect.translate(offset), uv, color);
                    }
                }
                char_index += 1;
            }
            char_index += usize::from(row.ends_with_newline);
            overlay.num_vertices += mesh.vertices.len();
            overlay.num_indices += mesh.indices.len();
            let mesh_bounds = mesh.calc_bounds();
            let glyph_vertex_range = 0..mesh.vertices.len();
            Arc::make_mut(&mut row.row).visuals = egui::epaint::text::RowVisuals {
                mesh,
                mesh_bounds,
                glyph_index_start: 0,
                glyph_vertex_range,
            };
        }
        // Keep UVs in texels until egui tessellates: later labels in the same
        // frame may grow the font atlas and invalidate normalized UVs.
        ui.painter().galley(pos, Arc::new(overlay), color);
        egui::text_selection::LabelSelectionState::label_text_selection(
            ui,
            &response,
            pos,
            galley,
            color,
            egui::Stroke::NONE,
        );
    }
}

fn format(size: f32, color: Color32, mono: bool) -> TextFormat {
    TextFormat {
        font_id: if mono {
            FontId::monospace(size - 1.0)
        } else {
            FontId::proportional(size)
        },
        color,
        line_height: Some(size * 1.7),
        ..Default::default()
    }
}

/// egui caches each stable LayoutJob/galley; completed paragraphs stay intact
/// while only the final streamed paragraph changes its layout key.
pub fn show(ui: &mut Ui, text: &str, color: Color32) {
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let mut lines = text.lines().peekable();
        while let Some(line) = lines.next() {
            if line.starts_with("```") {
                let language = line.trim_start_matches('`').trim();
                let mut code = String::new();
                for next in lines.by_ref() {
                    if next.starts_with("```") {
                        break;
                    }
                    if !code.is_empty() {
                        code.push('\n');
                    }
                    code.push_str(next);
                }
                Frame::NONE
                    .fill(crate::theme::tokens().code_background)
                    .corner_radius(8.0)
                    .inner_margin(Margin::same(12))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        if !language.is_empty() {
                            ui.label(
                                RichText::new(language)
                                    .size(11.0)
                                    .color(crate::theme::tokens().text_muted),
                            );
                            ui.add_space(6.0);
                        }
                        ui.add(
                            Label::new(RichText::new(code).monospace().size(13.0).color(color))
                                .wrap()
                                .selectable(true),
                        );
                    });
                ui.add_space(12.0);
            } else if line.is_empty() {
                ui.add_space(12.0);
            } else if line.starts_with('#') {
                let hashes = line.bytes().take_while(|b| *b == b'#').count();
                if hashes <= 6 && line.as_bytes().get(hashes) == Some(&b' ') {
                    ui.add_space(if hashes <= 2 { 16.0 } else { 10.0 });
                    let title = &line[hashes + 1..];
                    let mut heading = inline_job(
                        title,
                        match hashes {
                            1 => 26.0,
                            2 => 21.0,
                            _ => 18.0,
                        },
                        color,
                    );
                    // The browser's h2 is 1.5em = 21px. Keep the measured
                    // line box stable while matching the glyph size; changing
                    // the CJK face size must not move every following block.
                    let h2_line_height = (hashes == 2)
                        .then(|| ui.fonts(|fonts| fonts.row_height(&FontId::proportional(22.0))));
                    for section in &mut heading.job.sections {
                        section.format.line_height = h2_line_height;
                    }
                    heading.emphasis = vec![0..heading.job.text.chars().count()];
                    show_inline(ui, heading, color);
                    ui.add_space(10.0);
                } else {
                    show_inline(ui, inline_job(line, 14.0, color), color);
                }
            } else {
                let list = line.strip_prefix("- ").or_else(|| line.strip_prefix("* "));
                let ordered = line
                    .split_once(". ")
                    .filter(|(n, _)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
                if list.is_some() || ordered.is_some() {
                    let (marker, content) = if let Some(content) = list {
                        ("•".to_owned(), content)
                    } else if let Some((n, content)) = ordered {
                        (format!("{n}."), content)
                    } else {
                        (String::new(), line)
                    };
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        ui.add_space(24.0);
                        show_inline(ui, inline_job(&marker, 14.0, color), color);
                        show_inline(ui, inline_job(content, 14.0, color), color);
                    });
                } else {
                    show_inline(ui, inline_job(line, 14.0, color), color);
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn cjk_emphasis_wraps_into_valid_meshes() {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        let source =
            "## 标题\n\n1. **连续中文强调文本与换行**：保留正文。\n2. `sample` 与 **第二段强调**。";
        for _ in 0..2 {
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(170.0, 600.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        super::show(ui, source, egui::Color32::BLACK);
                    });
                },
            );
            let primitives = ctx.tessellate(output.shapes, output.pixels_per_point);
            assert!(!primitives.is_empty());
            for primitive in primitives {
                if let egui::epaint::Primitive::Mesh(mesh) = primitive.primitive {
                    assert!(mesh.is_valid());
                    assert!(mesh.vertices.iter().all(|vertex| {
                        vertex.pos.x.is_finite()
                            && vertex.pos.y.is_finite()
                            && vertex.uv.x.is_finite()
                            && vertex.uv.y.is_finite()
                    }));
                }
            }
        }
    }
}
