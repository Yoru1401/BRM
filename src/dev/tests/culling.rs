use bevy::prelude::*;

use super::helpers::*;
use crate::sdf::blending::*;
use crate::sdf::bounds::*;
use crate::sdf::brush::*;
use crate::sdf::distance::*;
use crate::sdf::field::*;

#[test]
fn box_culling_never_changes_the_field() {
    fn uncalled(shapes: &[GpuShape], point: Vec3) -> f32 {
        let mut field = MAX_MARCH_DISTANCE;
        for (index, shape) in shapes.iter().enumerate() {
            let distance = shape_distance(shape, point);
            field = if index == 0 {
                distance
            } else {
                blend(distance, field, &shape.blend, shape.blend.chamfer != 0)
            };
        }
        field
    }

    let mut state = 0x2545_F491_4F6C_DD1Du64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 40) as f32 / 16777216.0
    };
    macro_rules! spread {
        ($scale:expr) => {
            (next() - 0.5) * 2.0 * $scale
        };
    }

    for _ in 0..200 {
        let count = 2 + (next() * 8.0) as usize;
        let shapes: Vec<GpuShape> = (0..count)
            .map(|_| {
                let placement = Transform {
                    translation: Vec3::new(spread!(4.0), spread!(4.0), spread!(4.0)),
                    rotation: Quat::from_euler(
                        EulerRot::XYZ,
                        spread!(3.14),
                        spread!(3.14),
                        spread!(3.14),
                    ),
                    scale: Vec3::new(0.2 + next() * 2.0, 0.2 + next() * 2.0, 0.2 + next() * 2.0),
                };
                let modifiers = Modifiers {
                    round: next(),
                    bevel: next(),
                    thickness: next(),
                    cone: next(),
                };

                let operation = CsgOperation {
                    mode: (next() * 9.0) as u32,
                    chamfer: next() < 0.5,
                    radius: next() * 0.8,
                    strength: next() * 0.5,
                };
                pack_brush(
                    &GlobalTransform::from(placement),
                    Some(&modifiers),
                    Some(&operation),
                    None,
                )
            })
            .collect();

        for _ in 0..20 {
            let point = Vec3::new(spread!(7.0), spread!(7.0), spread!(7.0));
            let culled = scene_distance(&shapes, point);
            let full = uncalled(&shapes, point);
            assert_eq!(
                culled, full,
                "culling changed the field at {point:?} over {count} shapes"
            );
        }
    }
}

#[test]
fn the_cull_fires_on_a_distant_add_and_never_on_another_mode() {
    let far = Vec3::new(30.0, 0.0, 0.0);
    let near_field = 1.0;

    let added = placed(Transform::IDENTITY, union(0.0));
    assert!(shape_cannot_reach(&added, far, near_field));

    assert!(!shape_cannot_reach(
        &added,
        Vec3::new(1.2, 0.0, 0.0),
        near_field
    ));

    for mode in [
        GPU_MODE_SUBTRACT,
        GPU_MODE_INTERSECT,
        GPU_MODE_PAINT,
        GPU_MODE_PUSH,
        GPU_MODE_AVOID,
        GPU_MODE_EMBOSS,
        GPU_MODE_DEBOSS,
        GPU_MODE_SHELL,
    ] {
        let other = placed(Transform::IDENTITY, CsgOperation { mode, ..default() });
        assert!(
            !shape_cannot_reach(&other, far, near_field),
            "mode {mode} was culled without a proof that it is safe to"
        );
    }
}

#[test]
fn the_cull_bound_holds_under_a_taper() {
    let shape = shaped(
        Transform::from_scale(Vec3::new(2.0, 3.0, 2.0)),
        Modifiers {
            cone: 1.0,
            ..default()
        },
    );

    let mut checked = 0;
    for step in 1..200 {
        let point = Vec3::new(3.0 + step as f32 * 0.03, step as f32 * 0.04 - 3.0, -0.2);
        let estimate = shape_distance(&shape, point);
        if estimate <= 0.0 {
            continue;
        }
        let bound = shape.cull_scale * cull_box_distance(point - shape.center, shape.cull_extent);
        assert!(
            bound <= estimate + 1e-4,
            "bound {bound} overshot the estimate {estimate} at {point:?}"
        );
        checked += 1;
    }
    assert!(checked > 50, "only {checked} points were outside the taper");
}
