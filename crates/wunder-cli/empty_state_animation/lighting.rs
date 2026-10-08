//! Terminal-derived lighting for the banner.
//!
//! Like the reference implementation the wheel has no baked palette: the foreground and
//! background the terminal reports set the material, and the only fixed ingredient is a
//! warm specular tint, which is what reads as brass on a ship's wheel.

use super::geometry::Sample;

/// Light from the upper left, slightly toward the viewer.
const LIGHT: [f32; 3] = [-0.40, -0.56, 0.73];

/// Ambient floor, so the shaded side of a tube never vanishes into the background.
const AMBIENT: f32 = 0.19;

const SPECULAR_POWER: u32 = 7;
const SPECULAR_GAIN: f32 = 0.55;

/// Warm highlight mixed in at grazing light.
const SPECULAR_TINT: [f32; 3] = [1.00, 0.92, 0.74];

/// How far a surface falls off toward the rim of the wheel.
const RIM_FALLOFF: f32 = 0.10;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Lighting {
    foreground: [f32; 3],
    background: [f32; 3],
}

/// A lit dot: colour for truecolour, intensity for the colourless ramps.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Lit {
    pub(crate) rgb: [u8; 3],
    pub(crate) intensity: f32,
}

impl Lighting {
    /// Build the material from what the terminal reports, falling back to the usual
    /// light-on-dark defaults when it reports nothing.
    pub(crate) fn terminal(foreground: [u8; 3], background: [u8; 3]) -> Self {
        Self {
            foreground: to_floats(foreground),
            background: to_floats(background),
        }
    }

    /// Lambert diffuse plus a bounded specular term, mixed across the tube's face.
    pub(crate) fn shade(&self, sample: &Sample) -> Lit {
        let diffuse = sample
            .normal
            .iter()
            .zip(LIGHT.iter())
            .fold(0.0f32, |acc, (a, b)| acc + a * b)
            .max(0.0);
        let specular = diffuse.powi(SPECULAR_POWER as i32) * SPECULAR_GAIN;
        let falloff = 1.0 - RIM_FALLOFF * (sample.radius / super::paths::GEAR_EXTENT).min(1.0);
        let intensity =
            ((AMBIENT + (1.0 - AMBIENT) * diffuse + specular) * falloff).clamp(0.0, 1.0);

        let lit = diffuse.clamp(0.0, 1.0).powf(0.72);
        let mut rgb = [0u8; 3];
        for channel in 0..3 {
            let base = self.background[channel]
                + (self.foreground[channel] - self.background[channel]) * lit;
            let tinted = base + self.foreground[channel] * specular * SPECULAR_TINT[channel];
            rgb[channel] = (tinted * 255.0).round().clamp(0.0, 255.0) as u8;
        }
        Lit { rgb, intensity }
    }
}

/// Foreground-only ramp for a colourless terminal, where intensity must survive as weight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shade {
    Dim,
    Base,
    Bright,
}

impl Shade {
    pub(crate) fn of(intensity: f32) -> Self {
        if intensity < 0.34 {
            Self::Dim
        } else if intensity > 0.78 {
            Self::Bright
        } else {
            Self::Base
        }
    }
}

fn to_floats(rgb: [u8; 3]) -> [f32; 3] {
    [
        f32::from(rgb[0]) / 255.0,
        f32::from(rgb[1]) / 255.0,
        f32::from(rgb[2]) / 255.0,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_at(normal: [f32; 3], radius: f32) -> Sample {
        Sample {
            coverage: 1.0,
            normal,
            radius,
        }
    }

    fn terminal() -> Lighting {
        Lighting::terminal([210, 221, 235], [15, 20, 37])
    }

    #[test]
    fn a_surface_facing_the_light_is_brighter_than_one_facing_away() {
        let facing = terminal().shade(&sample_at(LIGHT, 0.5));
        let away = terminal().shade(&sample_at([-LIGHT[0], -LIGHT[1], LIGHT[2]], 0.5));
        assert!(facing.intensity > away.intensity);
        let facing_rgb = facing.rgb.iter().map(|v| u32::from(*v)).sum::<u32>();
        let away_rgb = away.rgb.iter().map(|v| u32::from(*v)).sum::<u32>();
        assert!(facing_rgb > away_rgb, "{facing:?} vs {away:?}");
    }

    #[test]
    fn shade_stays_between_the_terminals_own_colours() {
        let light = terminal();
        let background = [15u16, 20, 37];
        for radius in [0.0f32, 0.5, 1.0] {
            // A surface turned away from the light is the darkest the wheel may get.
            let away = light.shade(&sample_at([0.0, 0.0, -1.0], radius));
            let facing = light.shade(&sample_at([0.0, 0.0, 1.0], radius));
            for channel in 0..3 {
                assert!(
                    u16::from(away.rgb[channel]) >= background[channel],
                    "must not fall under the background: {:?}",
                    away.rgb
                );
                assert!(
                    u16::from(away.rgb[channel]) <= u16::from(facing.rgb[channel]),
                    "facing the light must never be darker: {away:?} vs {facing:?}"
                );
            }
        }
    }

    #[test]
    fn ramp_covers_all_three_weights() {
        assert_eq!(Shade::of(0.1), Shade::Dim);
        assert_eq!(Shade::of(0.5), Shade::Base);
        assert_eq!(Shade::of(0.95), Shade::Bright);
    }

    #[test]
    fn darker_foreground_gives_a_darker_wheel() {
        let dim = Lighting::terminal([70, 80, 95], [0, 0, 0]).shade(&sample_at(LIGHT, 0.5));
        let bright = terminal().shade(&sample_at(LIGHT, 0.5));
        let sum = |rgb: [u8; 3]| rgb.iter().map(|v| u32::from(*v)).sum::<u32>();
        assert!(sum(dim.rgb) < sum(bright.rgb), "{dim:?} vs {bright:?}");
    }
}
