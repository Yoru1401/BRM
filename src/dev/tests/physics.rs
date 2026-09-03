use bevy::prelude::*;

use crate::game::physics::*;
use crate::sdf::brush::*;

#[test]
fn sliding_turns_into_spin() {
    let (velocity_change, spin_change) = contact_friction(
        Vec3::Y,
        Vec3::X,
        Vec3::ZERO,
        0.5,
        10.0,
        FRICTION_COEFFICIENT,
    );

    assert!(velocity_change.x < 0.0);

    assert!(spin_change.z < 0.0);
}

#[test]
fn rolling_without_slipping_is_left_alone() {
    let radius = 0.5;
    let velocity = Vec3::X;
    let spin = Vec3::new(0.0, 0.0, -velocity.x / radius);
    let (velocity_change, spin_change) =
        contact_friction(Vec3::Y, velocity, spin, radius, 10.0, FRICTION_COEFFICIENT);
    assert!(velocity_change.length() < 1e-5);
    assert!(spin_change.length() < 1e-5);
}

#[test]
fn coulomb_caps_friction_on_a_weak_contact() {
    let (gentle, _) = contact_friction(
        Vec3::Y,
        Vec3::X * 10.0,
        Vec3::ZERO,
        0.5,
        0.01,
        FRICTION_COEFFICIENT,
    );
    assert!((gentle.length() - FRICTION_COEFFICIENT * 0.01).abs() < 1e-6);
}

#[test]
fn separate_spheres_do_not_interact() {
    assert!(
        sphere_pair_correction(
            Vec3::ZERO,
            1.0,
            Vec3::ZERO,
            Vec3::new(3.0, 0.0, 0.0),
            1.0,
            Vec3::ZERO
        )
        .is_none()
    );
}

#[test]
fn overlapping_spheres_split_the_gap_and_stop_closing() {
    let (separation, velocity_change) = sphere_pair_correction(
        Vec3::ZERO,
        1.0,
        Vec3::X,
        Vec3::new(1.5, 0.0, 0.0),
        1.0,
        -Vec3::X,
    )
    .expect("these overlap by 0.5");

    assert!((separation - Vec3::new(-0.25, 0.0, 0.0)).length() < 1e-5);

    assert!((velocity_change - Vec3::new(-1.0, 0.0, 0.0)).length() < 1e-5);
}

#[test]
fn overlapping_but_separating_spheres_keep_their_velocity() {
    let (_, velocity_change) = sphere_pair_correction(
        Vec3::ZERO,
        1.0,
        -Vec3::X,
        Vec3::new(1.5, 0.0, 0.0),
        1.0,
        Vec3::X,
    )
    .expect("these overlap");
    assert_eq!(velocity_change, Vec3::ZERO);
}

#[test]
fn a_body_that_leaves_the_world_is_removed() {
    let mut app = App::new();
    app.add_systems(Update, despawn_fallen_bodies);

    let body = |height: f32| {
        (
            SphereBody {
                radius: 0.5,
                velocity: Vec3::ZERO,
                angular_velocity: Vec3::ZERO,
                orientation: Quat::IDENTITY,
                resting: false,
            },
            Transform::from_xyz(0.0, height, 0.0),
        )
    };
    let resting = app.world_mut().spawn(body(1.0)).id();
    let falling = app.world_mut().spawn(body(-1000.0)).id();

    let deep = app.world_mut().spawn(body(-10.0)).id();

    app.update();

    assert!(app.world().get_entity(resting).is_ok());
    assert!(app.world().get_entity(deep).is_ok());
    assert!(
        app.world().get_entity(falling).is_err(),
        "a body 1000 units under the world was left in the scene"
    );
}
