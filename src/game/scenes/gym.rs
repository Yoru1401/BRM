use bevy::prelude::*;

use crate::game::scenes::{Exhibit, SdfWorld, floor};
use crate::sdf::brush::{Albedo, Brush, Modifiers, SphereBody};
use crate::sdf::light::{Light, LightKind};

const SOLID: Modifiers = Modifiers {
    round: 0.0,
    bevel: 0.0,
    thickness: 1.0,
    cone: 0.0,
};

const RAMP_ANGLES: [f32; 5] = [10.0, 20.0, 30.0, 40.0, 50.0];
const DROP_HEIGHTS: [f32; 4] = [2.0, 4.0, 6.0, 8.0];
const BODY_ALBEDO: Vec3 = Vec3::new(0.95, 0.85, 0.25);

fn body(position: Vec3, radius: f32) -> impl Bundle {
    (
        SphereBody {
            radius,
            velocity: Vec3::ZERO,
            angular_velocity: Vec3::ZERO,
            orientation: Quat::IDENTITY,
            resting: false,
        },
        Brush,
        Modifiers {
            round: 1.0,
            ..SOLID
        },
        Albedo(BODY_ALBEDO),
        Transform::from_translation(position).with_scale(Vec3::splat(radius)),
    )
}

pub(crate) fn spawn(mut commands: Commands) {
    commands
        .spawn((SdfWorld, Transform::default()))
        .with_children(|root| {
            root.spawn(floor(Vec3::new(22.0, 0.5, 14.0), 0.0));

            for (index, degrees) in RAMP_ANGLES.iter().enumerate() {
                let x = index as f32 * 5.0 - 10.0;
                root.spawn((
                    Brush,
                    SOLID,
                    Albedo(Vec3::new(0.40, 0.44, 0.50)),
                    Transform {
                        translation: Vec3::new(x, 1.2, -6.0),
                        rotation: Quat::from_rotation_z(degrees.to_radians()),
                        scale: Vec3::new(2.0, 0.25, 1.6),
                    },
                ));
                root.spawn((
                    Exhibit::new(
                        "ramp",
                        "friction is capped at FRICTION_COEFFICIENT times the normal impulse, \
                         so a steep enough ramp slides instead of gripping",
                    ),
                    Transform::from_xyz(x, 2.0, -6.0),
                ));
            }

            for index in 0..DROP_HEIGHTS.len() {
                let x = index as f32 * 3.0 - 4.5;
                root.spawn((
                    Brush,
                    SOLID,
                    Albedo(Vec3::new(0.30, 0.34, 0.38)),
                    Transform::from_xyz(x, 0.1, 2.0).with_scale(Vec3::new(0.9, 0.1, 0.9)),
                ));
                root.spawn((
                    Exhibit::new("drop lane", "restitution is zero, so nothing bounces"),
                    Transform::from_xyz(x, 1.0, 2.0),
                ));
            }

            root.spawn((
                Brush,
                Modifiers {
                    bevel: 1.0,
                    thickness: 0.35,
                    ..SOLID
                },
                Albedo(Vec3::new(0.55, 0.40, 0.30)),
                Transform::from_xyz(9.0, 1.4, 3.0).with_scale(Vec3::new(1.6, 1.4, 1.6)),
            ));
            root.spawn((
                Exhibit::new(
                    "the tube",
                    "bodies read the same packed bytes the renderer draws, \
                     so they fall through what looks open",
                ),
                Transform::from_xyz(9.0, 1.4, 3.0),
            ));

            root.spawn((
                Exhibit::new(
                    "the sleep pad",
                    "below SLEEP_SPEED and SLEEP_SPIN a touching body parks, \
                     which is what stops the gravity-push-out jitter",
                ),
                Transform::from_xyz(-9.0, 1.0, 4.0),
            ));
        });

    for (index, height) in DROP_HEIGHTS.iter().enumerate() {
        let x = index as f32 * 3.0 - 4.5;
        commands.spawn(body(Vec3::new(x, *height, 2.0), 0.35));
    }
    for (index, _) in RAMP_ANGLES.iter().enumerate() {
        let x = index as f32 * 5.0 - 10.0;
        commands.spawn(body(Vec3::new(x - 0.8, 4.0, -6.0), 0.3));
    }
    for step in 0..4 {
        commands.spawn(body(
            Vec3::new(9.0 + step as f32 * 0.12, 4.0 + step as f32 * 1.3, 3.0),
            0.28,
        ));
    }
    commands.spawn(body(Vec3::new(-9.0, 1.5, 4.0), 0.4));
}

pub(crate) fn spawn_lights(mut commands: Commands) {
    commands.spawn((
        Light {
            kind: LightKind::Directional,
            colour: Vec3::new(1.0, 0.96, 0.90),
            intensity: 1.0,
            shadow: true,
            softness: 12.0,
            ..default()
        },
        Transform::from_xyz(6.0, 14.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        Light {
            kind: LightKind::Directional,
            colour: Vec3::new(0.40, 0.50, 0.70),
            intensity: 0.3,
            ..default()
        },
        Transform::from_xyz(-6.0, 8.0, -8.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}
