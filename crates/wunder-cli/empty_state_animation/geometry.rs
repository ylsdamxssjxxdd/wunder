//! Surface sampling for the banner: the wheel is a union of coplanar tubes, so every
//! sample point can report how deep inside a tube it is and which way that surface faces.

/// Radial squashing applied to tube cross-sections. A terminal row is about twice as
/// tall as a column is wide, so a cross-section must bulge less in Y to read as round.
const CROSS_SECTION_FLATTEN: f32 = 0.86;

/// Coverage above which a sample counts as fully inside the tube.
const CORE_COVERAGE: f32 = 0.62;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Sample {
    /// Edge falloff in `0..=1`; `1.0` is well inside the tube.
    pub(crate) coverage: f32,
    /// Surface normal: x right, y down, z toward the viewer.
    pub(crate) normal: [f32; 3],
    /// Distance from the wheel centre, for tonal falloff.
    pub(crate) radius: f32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Prim {
    /// Torus seen head-on: a hoop around the origin.
    Ring { radius: f32, tube: f32 },
    /// Capsule between two points; a zero-length bar is a disc.
    Bar {
        ax: f32,
        ay: f32,
        bx: f32,
        by: f32,
        tube: f32,
    },
}

impl Prim {
    fn sample(&self, point: [f32; 2]) -> Option<Sample> {
        let (distance, outward, tube) = match *self {
            Self::Ring { radius, tube } => {
                let length = length_of(point);
                let radial = unit_outward(point, length);
                // A hoop faces away from the centre on its outer flank and toward it on
                // its inner flank; the radial direction alone would light both alike.
                let side = if length >= radius { 1.0 } else { -1.0 };
                (
                    (length - radius).abs(),
                    [radial[0] * side, radial[1] * side],
                    tube,
                )
            }
            Self::Bar {
                ax,
                ay,
                bx,
                by,
                tube,
            } => {
                let abx = bx - ax;
                let aby = by - ay;
                let span = abx * abx + aby * aby;
                let along = if span > f32::EPSILON {
                    ((point[0] - ax) * abx + (point[1] - ay) * aby) / span
                } else {
                    0.0
                }
                .clamp(0.0, 1.0);
                let offset = [point[0] - (ax + abx * along), point[1] - (ay + aby * along)];
                let distance = length_of(offset);
                (distance, unit_outward(offset, distance), tube)
            }
        };

        if distance >= tube {
            return None;
        }

        let ratio = distance / tube;
        let depth = (1.0 - ratio * ratio).sqrt() * CROSS_SECTION_FLATTEN;
        Some(Sample {
            coverage: 1.0 - smoothstep(CORE_COVERAGE, 1.0, ratio),
            normal: normalize([outward[0] * ratio, outward[1] * ratio, depth]),
            radius: length_of(point),
        })
    }
}

/// Frontmost surface at `point`, if any tube covers it. Ties go to the inner tube, so
/// spokes read over the hoop they pass through.
pub(crate) fn surface(prims: &[Prim], point: [f32; 2]) -> Option<Sample> {
    prims
        .iter()
        .filter_map(|prim| prim.sample(point))
        .max_by(|left, right| {
            left.coverage
                .total_cmp(&right.coverage)
                .then_with(|| right.radius.total_cmp(&left.radius))
        })
}

fn length_of(point: [f32; 2]) -> f32 {
    point[0].mul_add(point[0], point[1] * point[1]).sqrt()
}

fn unit_outward(point: [f32; 2], length: f32) -> [f32; 2] {
    if length > 1e-5 {
        [point[0] / length, point[1] / length]
    } else {
        [0.0, -1.0]
    }
}

fn normalize(vector: [f32; 3]) -> [f32; 3] {
    let length = vector
        .iter()
        .fold(0.0f32, |acc, value| acc + value * value)
        .sqrt();
    if length < f32::EPSILON {
        return [0.0, 0.0, 1.0];
    }
    [vector[0] / length, vector[1] / length, vector[2] / length]
}

fn smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::empty_state_animation::paths::{gear, GEAR_EXTENT};

    #[test]
    fn rim_is_solid_and_the_bore_stays_open() {
        let wheel = gear(0.0);
        assert!(
            surface(&wheel, [0.700, 0.0]).is_some(),
            "rim covers its own radius"
        );
        assert!(
            surface(&wheel, [0.0, -0.300]).is_none(),
            "the bore around the letter must stay open"
        );
        assert!(
            surface(&wheel, [0.560, 0.145]).is_none(),
            "between hoop and rim, off the teeth, is open air"
        );
    }

    #[test]
    fn tube_shading_faces_the_viewer_at_the_axis_and_rolls_at_the_edge() {
        let wheel = vec![Prim::Ring {
            radius: 0.69,
            tube: 0.10,
        }];
        let front = surface(&wheel, [0.69, 0.0]).expect("hoop");
        let edge = surface(&wheel, [0.772, 0.0]).expect("hoop edge");
        assert!(front.normal[2] > 0.8, "tube axis faces the viewer");
        assert!(
            edge.normal[2] < front.normal[2],
            "tube rolls away at the edge"
        );
        assert!(edge.normal[0] > edge.normal[2], "edge leans outward");
        assert!(
            edge.coverage < front.coverage,
            "edge is softer than the axis"
        );
    }

    #[test]
    fn normals_are_unit_length() {
        let wheel = gear(0.4);
        for step in 0..48 {
            let angle = step as f32 * std::f32::consts::TAU / 48.0;
            let point = [0.69 * angle.cos(), 0.69 * angle.sin()];
            if let Some(sample) = surface(&wheel, point) {
                let squared: f32 = sample.normal.iter().map(|v| v * v).sum();
                assert!((squared - 1.0).abs() < 0.02, "not a unit normal: {squared}");
            }
        }
    }

    #[test]
    fn nothing_reaches_past_the_declared_extent() {
        let wheel = gear(0.3);
        assert!(
            surface(&wheel, [GEAR_EXTENT, 0.0]).is_none(),
            "the drawing must fit inside GEAR_EXTENT"
        );
    }
}
