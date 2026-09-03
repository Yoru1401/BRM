use bevy::prelude::*;

use super::helpers::*;
use crate::sdf::blending::*;
use crate::sdf::brush::*;
use crate::sdf::field::*;

#[test]
fn blend_modes_are_arrangements_of_the_three_booleans() {
    let mode = |mode| CsgOperation { mode, ..default() };

    let field = -1.0;
    let shape = -0.5;

    assert_eq!(blend(shape, field, &pack(mode(GPU_MODE_ADD)), false), field);
    assert_eq!(
        blend(shape, field, &pack(mode(GPU_MODE_INTERSECT)), false),
        shape
    );

    assert_eq!(
        blend(shape, field, &pack(mode(GPU_MODE_SUBTRACT)), false),
        0.5
    );

    assert_eq!(
        blend(shape, field, &pack(mode(GPU_MODE_PAINT)), false),
        field
    );
}

#[test]
fn hard_union_is_the_nearer_of_the_two() {
    let scene = [
        placed(Transform::IDENTITY, union(0.0)),
        placed(Transform::from_xyz(4.0, 0.0, 0.0), union(0.0)),
    ];
    assert!((scene_distance(&scene, Vec3::new(3.0, 0.0, 0.0)) - 0.0).abs() < 1e-5);
}

#[test]
fn blending_pulls_the_surface_outwards_between_two_shapes() {
    let apart = 1.6;
    let hard = [
        placed(Transform::IDENTITY, union(0.0)),
        placed(Transform::from_xyz(apart, 0.0, 0.0), union(0.0)),
    ];
    let blended = [
        hard[0].clone(),
        placed(Transform::from_xyz(apart, 0.0, 0.0), union(0.5)),
    ];

    let probe = Vec3::new(apart * 0.5, 1.2, 0.0);
    assert!(scene_distance(&blended, probe) < scene_distance(&hard, probe));
}

#[test]
fn subtract_carves_a_hole() {
    let scene = [
        placed(Transform::from_scale(Vec3::splat(2.0)), union(0.0)),
        placed(
            Transform::IDENTITY,
            CsgOperation {
                mode: GPU_MODE_SUBTRACT,
                ..default()
            },
        ),
    ];

    assert!(scene_distance(&scene, Vec3::ZERO) > 0.0);

    assert!(scene_distance(&scene, Vec3::new(1.5, 0.0, 0.0)) < 0.0);
}
