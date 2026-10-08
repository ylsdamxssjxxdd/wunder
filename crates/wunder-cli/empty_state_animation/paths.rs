//! The gear itself: path data in a normalised square where `1.0` is the gear radius.
//!
//! Rust-style form — a toothed ring around an inner hoop with a fixed letter in the hub.
//! The teeth turn with the wheel while the letter holds upright, which is both how a real
//! geared panel reads and what keeps the hub legible at braille resolution.
//!
//! Cross-sections are deliberately fat. A braille cell only affords two dots across a
//! drawn feature, so a hairline tooth disappears at exactly the sizes a terminal can show.

use super::geometry::Prim;

pub(crate) const TOOTH_COUNT: usize = 12;

/// Angle between teeth; the silhouette repeats every one of these.
pub(crate) const TOOTH_PITCH_RAD: f32 = std::f32::consts::TAU / (TOOTH_COUNT as f32);

/// Outermost reach of the drawing, including a tooth tip.
pub(crate) const GEAR_EXTENT: f32 = 1.04;

const RIM_RADIUS: f32 = 0.700;
const RIM_TUBE: f32 = 0.085;
const TOOTH_INNER: f32 = 0.700;
const TOOTH_OUTER: f32 = 0.950;
const TOOTH_TUBE: f32 = 0.062;
const TOOTH_TIP_TUBE: f32 = 0.050;
const HOOP_RADIUS: f32 = 0.430;
const HOOP_TUBE: f32 = 0.072;

/// The letter in the hub: a four-stroke W, upright at any gear angle. It floats inside
/// the inner hoop rather than sitting on a disc, or the disc would fill its counters.
const LETTER_HALF_WIDTH: f32 = 0.290;
const LETTER_HALF_HEIGHT: f32 = 0.200;
const LETTER_TUBE: f32 = 0.050;

fn polar(radius: f32, angle: f32) -> [f32; 2] {
    [radius * angle.cos(), radius * angle.sin()]
}

fn bar(a: [f32; 2], b: [f32; 2], tube: f32) -> Prim {
    Prim::Bar {
        ax: a[0],
        ay: a[1],
        bx: b[0],
        by: b[1],
        tube,
    }
}

fn disc(at: [f32; 2], tube: f32) -> Prim {
    bar(at, at, tube)
}

/// The gear turned by `angle` radians: rim, twelve teeth, inner hoop and the hub letter.
pub(crate) fn gear(angle: f32) -> Vec<Prim> {
    let mut prims = vec![
        Prim::Ring {
            radius: RIM_RADIUS,
            tube: RIM_TUBE,
        },
        Prim::Ring {
            radius: HOOP_RADIUS,
            tube: HOOP_TUBE,
        },
    ];

    for index in 0..TOOTH_COUNT {
        let direction = angle + index as f32 * TOOTH_PITCH_RAD;
        prims.push(bar(
            polar(TOOTH_INNER, direction),
            polar(TOOTH_OUTER, direction),
            TOOTH_TUBE,
        ));
        prims.push(disc(polar(TOOTH_OUTER, direction), TOOTH_TIP_TUBE));
    }

    prims.extend(letter_w());
    prims
}

/// W as four strokes in the hub box: down, up to a low peak, down, up. Screen y grows
/// downward, so the crests are at `-height` and the valleys at `+height`.
fn letter_w() -> [Prim; 4] {
    let (w, h) = (LETTER_HALF_WIDTH, LETTER_HALF_HEIGHT);
    let crest_left = [-w, -h];
    let valley_left = [-w * 0.5, h];
    let peak_mid = [0.0, -h * 0.30];
    let valley_right = [w * 0.5, h];
    let crest_right = [w, -h];
    [
        bar(crest_left, valley_left, LETTER_TUBE),
        bar(valley_left, peak_mid, LETTER_TUBE),
        bar(peak_mid, valley_right, LETTER_TUBE),
        bar(valley_right, crest_right, LETTER_TUBE),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::empty_state_animation::geometry::surface;

    #[test]
    fn rim_and_hoop_are_solid_and_the_space_between_is_open() {
        let wheel = gear(0.0);
        assert!(
            surface(&wheel, [RIM_RADIUS, 0.0]).is_some(),
            "rim covers its own radius"
        );
        assert!(
            surface(&wheel, [HOOP_RADIUS, 0.0]).is_some(),
            "hoop covers its own radius"
        );
        let [x, y] = polar(0.560, TOOTH_PITCH_RAD / 2.0);
        assert!(
            surface(&wheel, [x, y]).is_none(),
            "between hoop and rim, off the teeth, is open air"
        );
    }

    #[test]
    fn teeth_reach_past_the_rim() {
        let wheel = gear(0.0);
        let [x, y] = polar(TOOTH_OUTER, 0.0);
        assert!(
            surface(&wheel, [x, y]).is_some(),
            "a tooth tip sits outside the rim"
        );
        assert!(TOOTH_OUTER + TOOTH_TIP_TUBE <= GEAR_EXTENT + 1e-6);
    }

    #[test]
    fn silhouette_repeats_every_tooth_pitch() {
        assert_eq!(
            probe(0.0),
            probe(TOOTH_PITCH_RAD),
            "one tooth pitch must be the same gear"
        );
    }

    #[test]
    fn a_small_turn_moves_the_teeth() {
        assert_ne!(
            probe(0.0),
            probe(TOOTH_PITCH_RAD / 3.0),
            "teeth must leave visible gaps as the gear turns"
        );
    }

    #[test]
    fn the_hub_letter_stays_upright_while_the_gear_turns() {
        let still = letter_ink(gear(0.0));
        let turned = letter_ink(gear(1.1));
        assert_eq!(still, turned, "the letter must not rotate with the teeth");
    }

    #[test]
    fn the_letter_has_ink_across_its_box() {
        let wheel = gear(0.0);
        let strokes = letter_ink(wheel.clone());
        let inked = strokes.iter().filter(|hit| **hit).count();
        assert!(
            inked > strokes.len() / 5 && inked < strokes.len() / 2,
            "a letter is a minority of its box: {inked}/{}",
            strokes.len()
        );
        assert!(
            surface(&wheel, [-LETTER_HALF_WIDTH, -LETTER_HALF_HEIGHT]).is_some(),
            "the W must reach the top corners of its box"
        );
        assert!(
            surface(&wheel, [0.0, 0.13]).is_none(),
            "the W must keep an open counter, not become a blob"
        );
    }

    #[test]
    fn nothing_reaches_past_the_declared_extent() {
        let wheel = gear(0.3);
        for step in 0..96 {
            let angle = step as f32 * std::f32::consts::TAU / 96.0;
            let [x, y] = polar(GEAR_EXTENT, angle);
            assert!(
                surface(&wheel, [x, y]).is_none(),
                "nothing may reach past GEAR_EXTENT at {angle}"
            );
        }
    }

    /// Coverage along a fixed circle outside the rim, where only teeth register.
    fn probe(angle: f32) -> Vec<u8> {
        let wheel = gear(angle);
        (0..96)
            .map(|step| {
                let [x, y] = polar(TOOTH_OUTER, step as f32 * std::f32::consts::TAU / 96.0);
                u8::from(surface(&wheel, [x, y]).is_some())
            })
            .collect()
    }

    /// Ink pattern inside the hub box, which is the letter and nothing else.
    fn letter_ink(wheel: Vec<Prim>) -> Vec<bool> {
        let mut hits = Vec::new();
        for row in 0..9 {
            for col in 0..13 {
                let x = -LETTER_HALF_WIDTH + col as f32 * (2.0 * LETTER_HALF_WIDTH / 12.0);
                let y = -LETTER_HALF_HEIGHT + row as f32 * (2.0 * LETTER_HALF_HEIGHT / 8.0);
                hits.push(surface(&wheel, [x, y]).is_some());
            }
        }
        hits
    }
}
