use bevy::prelude::*;

use super::helpers::*;
use crate::game::world::*;
use crate::sdf::brush::*;
use crate::sdf::distance::*;
use crate::sdf::field::*;

#[test]
fn authored_shapes_land_where_they_were_written() {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        bevy::scene::ScenePlugin,
        TransformPlugin,
    ));
    app.world_mut().spawn_scene(world_scene()).unwrap();
    app.update();

    let mut placed: Vec<Vec3> = app
        .world_mut()
        .query_filtered::<&GlobalTransform, With<Brush>>()
        .iter(app.world())
        .map(|placement| placement.translation())
        .collect();
    assert!(
        placed.len() >= 8,
        "expected the authored brushes, got {}",
        placed.len()
    );

    placed.sort_by(|a, b| a.to_array().partial_cmp(&b.to_array()).unwrap());
    placed.dedup_by(|a, b| a.distance(*b) < 1e-4);
    assert_eq!(
        placed.len(),
        app.world_mut()
            .query_filtered::<Entity, With<Brush>>()
            .iter(app.world())
            .count(),
        "every brush should sit where it was written, not stacked at the origin"
    );
}

#[test]
fn a_fully_rounded_box_is_an_exact_sphere() {
    let scene = [shaped(Transform::IDENTITY, sphere_modifiers())];
    for probe in [
        Vec3::new(3.0, 0.0, 0.0),
        Vec3::new(0.0, 3.0, 0.0),
        Vec3::new(2.0, -1.0, 0.0),
        Vec3::new(2.0, 2.0, 2.0),
    ] {
        let expected = probe.length() - 1.0;
        let actual = scene_distance(&scene, probe);
        assert!(
            (actual - expected).abs() < 1e-5,
            "at {probe}: {actual} against an exact sphere's {expected}"
        );
    }
    assert!((scene_distance(&scene, Vec3::ZERO) + 1.0).abs() < 1e-5);
}

#[test]
fn a_fully_bevelled_box_is_an_exact_cylinder() {
    let scene = [shaped(
        Transform::IDENTITY,
        Modifiers {
            bevel: 1.0,
            ..default()
        },
    )];

    assert!((scene_distance(&scene, Vec3::new(3.0, 0.0, 0.0)) - 2.0).abs() < 1e-5);

    assert!((scene_distance(&scene, Vec3::new(0.0, 3.0, 0.0)) - 2.0).abs() < 1e-5);

    let rim = scene_distance(&scene, Vec3::new(2.0, 2.0, 0.0));
    assert!((rim - 2f32.sqrt()).abs() < 1e-5);

    let across = scene_distance(&scene, Vec3::new(2.0, 0.0, 2.0));
    assert!((across - (8f32.sqrt() - 1.0)).abs() < 1e-5);
}

#[test]
fn box_matches_closed_form_outside_face_edge_and_inside() {
    let scene = [placed(Transform::IDENTITY, union(0.0))];

    assert!((scene_distance(&scene, Vec3::new(3.0, 0.0, 0.0)) - 2.0).abs() < 1e-5);

    let diagonal = scene_distance(&scene, Vec3::new(2.0, 2.0, 0.0));
    assert!((diagonal - 2f32.sqrt()).abs() < 1e-5);

    assert!((scene_distance(&scene, Vec3::ZERO) + 1.0).abs() < 1e-5);
}

#[test]
fn uniform_scale_scales_the_distance() {
    let scene = [shaped(
        Transform::from_scale(Vec3::splat(2.0)),
        sphere_modifiers(),
    )];

    assert!((scene_distance(&scene, Vec3::new(5.0, 0.0, 0.0)) - 3.0).abs() < 1e-5);
}

#[test]
fn the_modifiers_reach_every_shape_the_field_needs() {
    let unit_box = |modifiers| shaped(Transform::IDENTITY, modifiers);

    let plain = unit_box(Modifiers::default());
    assert!((shape_distance(&plain, Vec3::new(2.0, 0.0, 0.0)) - 1.0).abs() < 1e-4);

    let ball = unit_box(Modifiers {
        round: 1.0,
        ..default()
    });
    for probe in [
        Vec3::new(3.0, 0.0, 0.0),
        Vec3::new(0.0, 0.0, 3.0),
        Vec3::splat(2.0),
    ] {
        let measured = shape_distance(&ball, probe);
        assert!(
            (measured - (probe.length() - 1.0)).abs() < 1e-3,
            "at {probe} expected a sphere, got {measured}"
        );
    }

    let bevelled = unit_box(Modifiers {
        bevel: 1.0,
        ..default()
    });
    let corner = shape_distance(&bevelled, Vec3::new(1.5, 0.0, 1.5));
    assert!(
        (corner - (4.5f32.sqrt() - 1.0)).abs() < 1e-4,
        "got {corner}"
    );

    assert!((shape_distance(&bevelled, Vec3::new(0.0, 2.0, 0.0)) - 1.0).abs() < 1e-4);

    let tapered = unit_box(Modifiers {
        cone: 1.0,
        ..default()
    });
    assert!(shape_distance(&tapered, Vec3::new(0.0, 1.0, 0.0)).abs() < 1e-3);
    assert!(shape_distance(&tapered, Vec3::new(1.0, -1.0, 0.0)).abs() < 1e-3);
    assert!(shape_distance(&tapered, Vec3::new(0.8, 0.5, 0.0)) > 0.0);

    let base_corner = shape_distance(&tapered, Vec3::new(1.0, -1.0, 1.0));
    assert!(
        base_corner.abs() < 1e-3,
        "base corner should be sharp, got {base_corner}"
    );

    let ridge = shaped(
        Transform::from_scale(Vec3::new(3.0, 1.0, 1.0)),
        Modifiers {
            cone: 1.0,
            ..default()
        },
    );

    assert!(shape_distance(&ridge, Vec3::new(2.0, 1.0, 0.0)).abs() < 1e-3);
    assert!(shape_distance(&ridge, Vec3::new(2.4, 1.0, 0.0)) > 0.0);

    assert!(shape_distance(&ridge, Vec3::new(1.0, 0.9, 0.0)) < 0.0);

    let both = unit_box(Modifiers {
        cone: 1.0,
        bevel: 1.0,
        ..default()
    });
    assert!(shape_distance(&both, Vec3::new(0.0, 1.0, 0.0)).abs() < 1e-3);
    assert!(shape_distance(&both, Vec3::new(1.0, -1.0, 0.0)).abs() < 1e-3);

    let hollow = unit_box(Modifiers {
        thickness: 0.5,
        ..default()
    });
    assert!(shape_distance(&hollow, Vec3::ZERO) > 0.0);
    assert!((shape_distance(&hollow, Vec3::new(2.0, 0.0, 0.0)) - 1.0).abs() < 1e-4);
    assert!(shape_distance(&hollow, Vec3::new(0.5, 0.0, 0.0)).abs() < 1e-3);
    assert!(shape_distance(&hollow, Vec3::new(0.75, 0.0, 0.0)) < 0.0);
    assert!(shape_distance(&hollow, Vec3::new(0.25, 0.0, 0.0)) > 0.0);

    assert!(shape_distance(&hollow, Vec3::new(0.0, 0.99, 0.0)) > 0.0);
    assert!(shape_distance(&hollow, Vec3::new(0.0, -0.99, 0.0)) > 0.0);

    let plate = shaped(
        Transform::from_scale(Vec3::new(1.0, 0.1, 1.0)),
        Modifiers {
            thickness: 0.5,
            ..default()
        },
    );
    assert!(shape_distance(&plate, Vec3::ZERO) > 0.0);
    assert!(shape_distance(&plate, Vec3::new(0.75, 0.0, 0.0)) < 0.0);

    let paper = unit_box(Modifiers {
        thickness: 0.0,
        ..default()
    });
    assert!(shape_distance(&paper, Vec3::ZERO) > 0.0);
    assert!(shape_distance(&paper, Vec3::new(0.99, 0.0, 0.0)).abs() < 0.02);

    let funnel = unit_box(Modifiers {
        cone: 0.5,
        thickness: 0.3,
        ..default()
    });
    assert!(shape_distance(&funnel, Vec3::new(0.0, -0.5, 0.0)) < 0.0);
    assert!(shape_distance(&funnel, Vec3::new(0.0, 0.95, 0.0)) > 0.0);
}

#[test]
fn rotation_is_applied_in_the_shapes_own_frame() {
    let scene = [placed(
        Transform::from_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_4)),
        union(0.0),
    )];

    let expected = ((2f32.sqrt() - 1.0).powi(2) * 2.0).sqrt();
    let measured = scene_distance(&scene, Vec3::new(2.0, 0.0, 0.0));
    assert!(
        (measured - expected).abs() < 1e-5,
        "expected {expected}, got {measured}"
    );
}

#[test]
fn non_uniform_box_scale_stays_an_exact_distance() {
    let scene = [placed(
        Transform::from_scale(Vec3::new(4.0, 1.0, 1.0)),
        union(0.0),
    )];

    assert!((scene_distance(&scene, Vec3::new(6.0, 0.0, 0.0)) - 2.0).abs() < 1e-5);
}

#[test]
fn a_bevelled_box_is_a_cylinder_exact_on_side_cap_and_corner() {
    let cylinder = |scale| {
        shaped(
            Transform::from_scale(scale),
            Modifiers {
                bevel: 1.0,
                ..default()
            },
        )
    };

    let scene = [cylinder(Vec3::new(1.0, 2.0, 1.0))];

    assert!((scene_distance(&scene, Vec3::new(4.0, 0.0, 0.0)) - 3.0).abs() < 1e-5);

    assert!((scene_distance(&scene, Vec3::new(0.0, 5.0, 0.0)) - 3.0).abs() < 1e-5);

    let corner = scene_distance(&scene, Vec3::new(2.0, 3.0, 0.0));
    assert!((corner - 2f32.sqrt()).abs() < 1e-5);

    assert!((scene_distance(&scene, Vec3::ZERO) + 1.0).abs() < 1e-5);

    let tall = [cylinder(Vec3::new(2.0, 3.0, 2.0))];
    assert!((scene_distance(&tall, Vec3::new(5.0, 0.0, 0.0)) - 3.0).abs() < 1e-5);
    assert!((scene_distance(&tall, Vec3::new(0.0, 7.0, 0.0)) - 4.0).abs() < 1e-5);
}
