use bevy::prelude::*;

use super::helpers::*;
use crate::sdf::bounds::*;
use crate::sdf::brush::*;
use crate::sdf::field::*;
use crate::sdf::grid::*;

#[test]
fn a_spot_packs_a_cone_that_fades_outwards() {
    use crate::sdf::light::{Light, LightKind};

    let light = Light {
        kind: LightKind::Spot {
            inner: 0.25,
            outer: 0.45,
        },
        ..default()
    };
    let packed = light.to_gpu(&GlobalTransform::from(
        Transform::from_xyz(0.0, 5.0, 0.0).looking_at(Vec3::ZERO, Vec3::Z),
    ));

    assert!(
        packed.cos_inner > packed.cos_outer,
        "inner {} should have the larger cosine, outer is {}",
        packed.cos_inner,
        packed.cos_outer
    );

    assert!(
        packed.direction.dot(Vec3::NEG_Y) > 0.99,
        "expected it to point down, got {:?}",
        packed.direction
    );

    let inverted = Light {
        kind: LightKind::Spot {
            inner: 0.5,
            outer: 0.1,
        },
        ..default()
    }
    .to_gpu(&GlobalTransform::IDENTITY);
    assert!(inverted.cos_inner > inverted.cos_outer);
}

#[test]
fn the_soft_shadow_ratio_is_not_darkened_by_the_grid() {
    const SOFTNESS: f32 = 12.0;
    const BIAS: f32 = 0.02;
    const STEPS: u32 = 48;

    let penumbra = |field: &dyn Fn(Vec3) -> f32, origin: Vec3, direction: Vec3, far: f32| {
        let mut shade = 1.0f32;
        let mut travelled = BIAS;
        for _ in 0..STEPS {
            if travelled >= far {
                break;
            }
            let distance = field(origin + direction * travelled);
            if distance < 0.001 {
                return 0.0;
            }
            shade = shade.min(SOFTNESS * distance / travelled);
            travelled += distance;
        }
        shade.clamp(0.0, 1.0)
    };

    let shapes = vec![
        shaped(
            Transform {
                translation: Vec3::new(0.0, -0.5, 0.0),
                scale: Vec3::new(20.0, 1.0, 20.0),
                ..default()
            },
            Modifiers::default(),
        ),
        shaped(Transform::from_xyz(0.0, 1.5, 0.0), sphere_modifiers()),
    ];

    let (bounds_min, bounds_max) = scene_bounds(&shapes);
    let grid = build_grid(&shapes, bounds_min, bounds_max, 16);
    let exact = |point| scene_distance(&shapes, point);
    let gridded = |point| scene_distance_gridded(&shapes, &grid, point);

    let sun = Vec3::Y;
    let mut checked = 0;
    let mut worst: f32 = 0.0;
    for step in 0..24 {
        let across = 4.0 + step as f32 * 0.25;
        let origin = Vec3::new(across, 2.0, 0.0);
        let open = penumbra(&exact, origin, sun, 40.0);
        assert!(
            open > 0.9,
            "the exact field shadowed an open ray at x = {across}: {open}"
        );
        let through_grid = penumbra(&gridded, origin, sun, 40.0);
        worst = worst.max(open - through_grid);
        checked += 1;
    }

    assert!(checked == 24);

    let grazing = Vec3::new(1.0, 0.25, 0.0).normalize();
    for step in 0..12 {
        let along = -9.0 + step as f32 * 0.5;
        let origin = Vec3::new(along, 1.0, 8.0);
        let open = penumbra(&exact, origin, grazing, 40.0);
        assert!(
            open > 0.9,
            "the exact field shadowed an open grazing ray at x = {along}: {open}"
        );
        worst = worst.max(open - penumbra(&gridded, origin, grazing, 40.0));
        checked += 1;
    }

    assert!(
        worst < 0.05,
        "the grid darkened an unoccluded ray by {worst} over {checked} rays;              the penumbra is reading cell walls as geometry"
    );
}

#[test]
fn the_shadow_proxy_bounds_the_field_and_still_lets_light_through() {
    const SOFTNESS: f32 = 12.0;
    const BIAS: f32 = 0.02;
    const STEPS: u32 = 48;

    let penumbra = |field: &dyn Fn(Vec3) -> f32, origin: Vec3, direction: Vec3, far: f32| {
        let mut shade = 1.0f32;
        let mut travelled = BIAS;
        for _ in 0..STEPS {
            if travelled >= far {
                break;
            }
            let distance = field(origin + direction * travelled);
            if distance < 0.001 {
                return 0.0;
            }
            shade = shade.min(SOFTNESS * distance / travelled);
            travelled += distance;
        }
        shade.clamp(0.0, 1.0)
    };

    let shapes = vec![
        shaped(
            Transform {
                translation: Vec3::new(0.0, -0.5, 0.0),
                scale: Vec3::new(20.0, 1.0, 20.0),
                ..default()
            },
            Modifiers::default(),
        ),
        shaped(Transform::from_xyz(0.0, 1.5, 0.0), sphere_modifiers()),
        shaped(
            Transform {
                translation: Vec3::new(-5.0, 1.0, 3.0),
                rotation: Quat::from_rotation_y(0.6) * Quat::from_rotation_z(0.35),
                scale: Vec3::new(3.0, 0.4, 1.2),
            },
            Modifiers {
                round: 0.5,
                ..default()
            },
        ),
        shaped(
            Transform {
                translation: Vec3::new(5.0, 1.2, -2.0),
                scale: Vec3::new(2.0, 1.2, 0.6),
                ..default()
            },
            Modifiers {
                cone: 0.7,
                ..default()
            },
        ),
    ];

    let (bounds_min, bounds_max) = scene_bounds(&shapes);
    let grid = build_grid(&shapes, bounds_min, bounds_max, 16);
    let gridded = |point| scene_distance_gridded(&shapes, &grid, point);
    let proxy = |point| shadow_proxy_distance(&shapes, &grid, point);

    let (mut outside, mut inside, mut loose) = (0, 0, 0);
    for xi in 0..13 {
        for yi in 0..13 {
            for zi in 0..13 {
                let point = bounds_min
                    + (bounds_max - bounds_min) * Vec3::new(xi as f32, yi as f32, zi as f32) / 12.0;
                let (bound, field) = (proxy(point), gridded(point));
                if field < 0.0 {
                    inside += 1;
                    continue;
                }
                assert!(
                    bound <= field + 1e-3,
                    "the proxy read {bound} where the field reads {field} at {point}"
                );
                if bound < field - 1e-3 {
                    loose += 1;
                }
                outside += 1;
            }
        }
    }

    assert!(
        outside > inside,
        "only {outside} of {} sample points were outside the geometry",
        outside + inside
    );

    assert!(
        loose > 0,
        "the proxy equalled the field at every one of {outside} points"
    );

    let sun = Vec3::Y;
    let beneath = Vec3::new(0.0, 0.3, 0.0);
    assert_eq!(penumbra(&gridded, beneath, sun, 40.0), 0.0);
    assert_eq!(penumbra(&proxy, beneath, sun, 40.0), 0.0);

    let mut checked = 0;
    for step in 0..12 {
        let across = 4.0 + step as f32 * 0.5;
        let origin = Vec3::new(across, 2.0, 0.0);
        let open = penumbra(&gridded, origin, sun, 40.0);
        assert!(open > 0.9, "the field shadowed an open ray at x = {across}");
        let through_proxy = penumbra(&proxy, origin, sun, 40.0);
        assert!(
            through_proxy > 0.9,
            "the proxy darkened an open ray at x = {across} to {through_proxy};                  its boxes reach further than the shapes that made them"
        );
        checked += 1;
    }
    assert_eq!(checked, 12);
}
