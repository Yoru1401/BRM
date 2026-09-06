use bevy::prelude::*;

use super::helpers::*;
use crate::sdf::bounds::scene_bounds;
use crate::sdf::brush::{GpuShape, Modifiers};
use crate::sdf::field::scene_distance;
use crate::sdf::grid::{SkipPyramid, build_skip_pyramid};

fn scattered() -> Vec<GpuShape> {
    let mut state = 0x2C13_9F04_5AB7_0011u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 40) as f32 / 16777216.0
    };
    (0..40)
        .map(|_| {
            shaped(
                Transform {
                    translation: Vec3::new(
                        (next() - 0.5) * 80.0,
                        (next() - 0.5) * 12.0,
                        (next() - 0.5) * 80.0,
                    ),
                    scale: Vec3::splat(0.4 + next() * 1.2),
                    ..default()
                },
                Modifiers::default(),
            )
        })
        .collect()
}

fn coarse_of(shapes: &[GpuShape]) -> SkipPyramid {
    let (low, high) = scene_bounds(shapes);
    build_skip_pyramid(shapes, low, high)
}

#[test]
fn an_empty_coarse_cell_never_claims_more_room_than_the_field_has() {
    let shapes = scattered();
    let coarse = coarse_of(&shapes);

    let mut state = 0x9911_3344_5566_7788u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 40) as f32 / 16777216.0
    };

    let mut skipped = 0;
    for _ in 0..4000 {
        let point = Vec3::new(
            (next() - 0.5) * 100.0,
            (next() - 0.5) * 20.0,
            (next() - 0.5) * 100.0,
        );
        let span = coarse.empty_span(point);
        if span <= 0.0 {
            continue;
        }
        skipped += 1;
        let field = scene_distance(&shapes, point);
        assert!(
            span <= field + 1e-3,
            "coarse offered a {span} jump where the field is only {field}, at {point:?}",
        );
    }
    assert!(
        skipped > 500,
        "only {skipped} samples landed in an empty coarse cell",
    );
}

#[test]
fn the_pyramid_crosses_empty_space_without_touching_a_single_shape() {
    let shapes = scattered();
    let coarse = coarse_of(&shapes);

    let cross = |use_pyramid: bool, origin: Vec3, direction: Vec3| -> (u32, u32) {
        let (mut travelled, mut steps, mut evaluations) = (0.0f32, 0u32, 0u32);
        while travelled < 120.0 && steps < 512 {
            let point = origin + direction * travelled;
            let span = match use_pyramid {
                true => coarse.empty_span(point),
                false => 0.0,
            };
            let step = match span > 0.0 {
                true => span,
                false => {
                    evaluations += 1;
                    let reach = scene_distance(&shapes, point);
                    if reach < 0.01 {
                        break;
                    }
                    reach
                }
            };
            travelled += step.max(1e-3);
            steps += 1;
        }
        (steps, evaluations)
    };

    let (mut plain, mut with_pyramid) = (0u32, 0u32);
    for step in 0..32 {
        let angle = step as f32 * std::f32::consts::TAU / 32.0;
        let origin = Vec3::new(angle.cos() * 55.0, 2.0, angle.sin() * 55.0);
        let direction = (Vec3::new(0.0, 2.0, 0.0) - origin).normalize();
        plain += cross(false, origin, direction).1;
        with_pyramid += cross(true, origin, direction).1;
    }

    println!("field evaluations: plain {plain}, pyramid {with_pyramid}");
    assert!(
        with_pyramid * 2 < plain,
        "the pyramid spent {with_pyramid} evaluations against {plain}, which is not worth a buffer",
    );
}

#[test]
fn the_uploaded_buffer_is_exactly_the_size_the_pyramid_needs() {
    assert_eq!(
        crate::sdf::grid::SKIP_WORDS,
        SkipPyramid::words(),
        "the buffer the shader reads and the pyramid the CPU builds must agree",
    );
}
