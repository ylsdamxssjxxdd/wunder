use super::glb::Scene;

#[derive(Clone, Copy, Debug)]
pub struct View {
    pub yaw: f32,
    pub pitch: f32,
    pub zoom: f32,
    pub pan_x: f32,
    pub pan_y: f32,
}

/// Initial camera copied from the bridge Three.js viewer.  The bridge keeps
/// the model at a four-unit fitted extent and opens from the underside at
/// `(0.02, -4.88, 0.01)`.  Keeping the constants here prevents the Slint
/// fallback and the native presenter from drifting apart on reset/reload.
pub const REFERENCE_VIEW: View = View {
    // forward = normalize(target - camera) where the bridge camera is
    // `(0.02, -4.88, 0.01)`.  With the raster basis this is yaw 180.235°,
    // pitch -0.117° (the camera therefore sits on the negative-Y side).
    yaw: 180.235,
    pitch: -0.117,
    zoom: 1.0,
    pan_x: 0.0,
    pan_y: 0.0,
};

pub const REFERENCE_DISTANCE: f32 = 4.88;

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(0.0001);
    [v[0] / length, v[1] / length, v[2] / length]
}

/// Compact metallic-roughness approximation for the software fallback. It
/// keeps the glTF base colour while adding a stable broad highlight, which is
/// sufficient for the model silhouette and metal panels on Win7/X11 without
/// pulling a GPU renderer into the desktop binary.
fn shade(color: [f32; 3], normal: [f32; 3], roughness: f32, metallic: f32) -> [u8; 3] {
    let n = normalize(normal);
    // Match the bridge lights: HemisphereLight(sky #e5efff, ground #858b95,
    // 2.1), a white key at (3,4,5) with intensity 2.7 and a cool rim at
    // (-4,2,-3) with intensity 1.6.  The coefficients are compressed for
    // the CPU path but retain the same light direction and palette.
    let key = normalize([3.0, 4.0, 5.0]);
    let rim = normalize([-4.0, 2.0, -3.0]);
    let sky = [0.898, 0.937, 1.0];
    let ground = [0.522, 0.545, 0.584];
    let hemisphere = n[1] * 0.5 + 0.5;
    let ambient = [0, 1, 2].map(|i| sky[i] * hemisphere + ground[i] * (1.0 - hemisphere));
    let ndotl_key = (n[0] * key[0] + n[1] * key[1] + n[2] * key[2]).max(0.0);
    let ndotl_rim = (n[0] * rim[0] + n[1] * rim[1] + n[2] * rim[2]).max(0.0);
    let diffuse = (ndotl_key * 0.74 + ndotl_rim * 0.38) * (1.0 - metallic);
    let view = normalize([0.0, 1.0, 0.0]);
    let half_key = normalize([key[0] + view[0], key[1] + view[1], key[2] + view[2]]);
    let half_rim = normalize([rim[0] + view[0], rim[1] + view[1], rim[2] + view[2]]);
    let ndoth = (n[0] * half_key[0] + n[1] * half_key[1] + n[2] * half_key[2])
        .max(0.0)
        .max((n[0] * half_rim[0] + n[1] * half_rim[1] + n[2] * half_rim[2]).max(0.0));
    let exponent = (2.0 / (roughness.clamp(0.04, 1.0).powi(2)) - 2.0).clamp(1.0, 128.0);
    let fresnel_base = 0.04 * (1.0 - metallic);
    let specular =
        ndoth.powf(exponent) * (0.16 + 0.34 * (1.0 - roughness)) * (fresnel_base + metallic);
    [0, 1, 2].map(|i| {
        let reflected = if metallic > 0.5 { color[i] } else { 1.0 };
        ((color[i] * (ambient[i] * 0.42 + diffuse) + reflected * specular).clamp(0.0, 1.0)
            * 255.0) as u8
    })
}

pub fn render(scene: &Scene, view: View, width: usize, height: usize) -> Vec<u8> {
    let width = width.clamp(160, 900);
    let height = height.clamp(140, 700);
    let mut rgba = vec![0u8; width * height * 4];
    let mut depth = vec![f32::INFINITY; width * height];
    for y in 0..height {
        for x in 0..width {
            // CSS reference: radial-gradient(ellipse at 50% 40%, #fff 0%,
            // #edf2f7 75%). Use the same focal point and clamp the ellipse
            // so resizing keeps the visual proportions stable.
            let dx = (x as f32 / width.max(1) as f32 - 0.5) * 1.0;
            let dy = (y as f32 / height.max(1) as f32 - 0.4) * (height as f32 / width.max(1) as f32);
            let t = (dx.mul_add(dx, dy * dy).sqrt() / 0.85).clamp(0.0, 1.0);
            let edge = [237.0, 242.0, 247.0];
            let c = [0, 1, 2].map(|i| 255.0 + (edge[i] - 255.0) * t);
            let p = (y * width + x) * 4;
            rgba[p] = c[0] as u8;
            rgba[p + 1] = c[1] as u8;
            rgba[p + 2] = c[2] as u8;
            rgba[p + 3] = 255;
        }
    }
    let cy = view.yaw.to_radians().sin();
    let sy = view.yaw.to_radians().cos();
    let cp = view.pitch.to_radians().cos();
    let sp = view.pitch.to_radians().sin();
    let forward = [cy * cp, -sy * cp, sp];
    let right = [sy, cy, 0.0];
    let up = [-cy * sp, sy * sp, cp];
    let focal = height as f32 / (2.0 * (18.0_f32.to_radians()).tan());
    let distance = REFERENCE_DISTANCE / view.zoom.clamp(0.4, 5.0);
    let project = |p: [f32; 3]| -> Option<(f32, f32, f32)> {
        let q = [
            p[0] + forward[0] * distance,
            p[1] + forward[1] * distance,
            p[2] + forward[2] * distance,
        ];
        let z = q[0] * forward[0] + q[1] * forward[1] + q[2] * forward[2];
        if z <= 0.02 {
            return None;
        }
        let x = q[0] * right[0] + q[1] * right[1] + q[2] * right[2] + view.pan_x;
        let y = q[0] * up[0] + q[1] * up[1] + q[2] * up[2] + view.pan_y;
        Some((
            width as f32 * 0.5 + x * focal / z,
            height as f32 * 0.50 - y * focal / z,
            z,
        ))
    };
    for indices in &scene.triangles {
        let tri = [
            scene.vertices[indices[0] as usize],
            scene.vertices[indices[1] as usize],
            scene.vertices[indices[2] as usize],
        ];
        let (Some(a), Some(b), Some(c)) = (
            project(tri[0].position),
            project(tri[1].position),
            project(tri[2].position),
        ) else {
            continue;
        };
        let mut area = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
        if area.abs() < 0.05 {
            continue;
        };
        // Normalize winding once so the inner loop only performs additions and
        // comparisons. This removes three cross products and three divisions
        // for every covered pixel.
        let winding = if area < 0.0 { -1.0 } else { 1.0 };
        area *= winding;
        let inv_area = 1.0 / area;
        let minx = a.0.min(b.0).min(c.0).floor().max(0.0) as i32;
        let maxx = a.0.max(b.0).max(c.0).ceil().min(width as f32 - 1.0) as i32;
        let miny = a.1.min(b.1).min(c.1).floor().max(0.0) as i32;
        let maxy = a.1.max(b.1).max(c.1).ceil().min(height as f32 - 1.0) as i32;
        let color_base =
            [0, 1, 2].map(|i| (tri[0].color[i] + tri[1].color[i] + tri[2].color[i]) / 3.0);
        let normal =
            [0, 1, 2].map(|i| (tri[0].normal[i] + tri[1].normal[i] + tri[2].normal[i]) / 3.0);
        let roughness = (tri[0].roughness + tri[1].roughness + tri[2].roughness) / 3.0;
        let metallic = (tri[0].metallic + tri[1].metallic + tri[2].metallic) / 3.0;
        let color = shade(color_base, normal, roughness, metallic);
        let e0_dx = (c.1 - b.1) * winding;
        let e1_dx = (a.1 - c.1) * winding;
        let e2_dx = (b.1 - a.1) * winding;
        let z_dx = a.2 * e0_dx + b.2 * e1_dx + c.2 * e2_dx;
        for y in miny..=maxy {
            let py = y as f32 + 0.5;
            let px = minx as f32 + 0.5;
            let mut e0 = ((b.0 - c.0) * (py - c.1) - (b.1 - c.1) * (px - c.0)) * winding;
            let mut e1 = ((c.0 - a.0) * (py - a.1) - (c.1 - a.1) * (px - a.0)) * winding;
            let mut e2 = ((a.0 - b.0) * (py - b.1) - (a.1 - b.1) * (px - b.0)) * winding;
            let mut z_num = a.2 * e0 + b.2 * e1 + c.2 * e2;
            for x in minx..=maxx {
                if e0 >= 0.0 && e1 >= 0.0 && e2 >= 0.0 {
                    let z = z_num * inv_area;
                    let index = y as usize * width + x as usize;
                    if z < depth[index] {
                        depth[index] = z;
                        let p = index * 4;
                        rgba[p] = color[0];
                        rgba[p + 1] = color[1];
                        rgba[p + 2] = color[2];
                    }
                }
                e0 += e0_dx;
                e1 += e1_dx;
                e2 += e2_dx;
                z_num += z_dx;
            }
        }
    }
    rgba
}

#[cfg(test)]
mod tests {
    use super::{render, View, REFERENCE_VIEW};
    use crate::native::wunder_model::glb::parse;

    #[test]
    fn default_view_writes_model_pixels() {
        let scene = parse(include_bytes!("../../../assets/models/wunder.glb"))
            .expect("bundled GLB should parse");
        let frame = render(
            &scene,
            View {
                ..REFERENCE_VIEW
            },
            320,
            240,
        );
        let background = [frame[0], frame[1], frame[2]];
        let covered = frame
            .chunks_exact(4)
            .filter(|pixel| pixel[..3] != background)
            .count();
        assert!(
            covered > 320 * 240 / 100,
            "default view should contain visible geometry"
        );
    }
}
