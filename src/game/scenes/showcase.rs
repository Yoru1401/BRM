use bevy::prelude::*;

use crate::game::scenes::SdfWorld;
use crate::sdf::brush::{Albedo, Brush, CsgOperation, GPU_MODE_SUBTRACT, Modifiers, SphereBody};
use crate::sdf::light::{Light, LightKind};

pub(crate) fn spawn_lights(mut commands: Commands) {
    commands.spawn((
        Light {
            kind: LightKind::Directional,
            colour: Vec3::new(1.0, 0.96, 0.88),
            intensity: 0.9,
            shadow: true,
            softness: 12.0,
            ..default()
        },
        Transform::from_xyz(0.0, 10.0, 0.0).looking_at(Vec3::new(-3.0, 0.0, 1.0), Vec3::Y),
    ));
    commands.spawn((
        Light {
            kind: LightKind::Point,
            colour: Vec3::new(1.0, 0.55, 0.2),
            intensity: 4.0,
            range: 9.0,
            ..default()
        },
        Transform::from_xyz(3.0, 2.2, 3.0),
    ));
    commands.spawn((
        Light {
            kind: LightKind::Spot {
                inner: 0.25,
                outer: 0.45,
            },
            colour: Vec3::new(0.45, 0.7, 1.0),
            intensity: 12.0,
            range: 14.0,
            ..default()
        },
        Transform::from_xyz(-4.0, 6.0, 4.0).looking_at(Vec3::new(-3.0, 0.8, 2.0), Vec3::Y),
    ));
}

pub(crate) fn spawn(mut commands: Commands) {
    commands.spawn_scene(world_scene());
    for (position, radius) in DROPS {
        commands.spawn((
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
                ..default()
            },
            Albedo(BODY_ALBEDO),
            Transform::from_translation(position).with_scale(Vec3::splat(radius)),
        ));
    }
}

const BODY_ALBEDO: Vec3 = Vec3::new(0.95, 0.85, 0.25);

const DROPS: [(Vec3, f32); 14] = [
    (Vec3::new(-3.00, 4.0, 2.00), 0.30),
    (Vec3::new(-2.85, 5.5, 2.10), 0.28),
    (Vec3::new(-3.15, 7.0, 1.90), 0.32),
    (Vec3::new(-3.00, 8.5, 2.15), 0.26),
    (Vec3::new(-2.90, 10.0, 1.85), 0.30),
    (Vec3::new(-3.10, 11.5, 2.00), 0.28),
    (Vec3::new(0.00, 5.0, -3.00), 0.35),
    (Vec3::new(0.15, 6.5, -2.90), 0.30),
    (Vec3::new(-0.10, 8.0, -3.10), 0.38),
    (Vec3::new(0.05, 9.5, -3.00), 0.32),
    (Vec3::new(3.00, 5.0, -3.00), 0.40),
    (Vec3::new(3.20, 6.5, -2.85), 0.35),
    (Vec3::new(2.80, 8.0, -3.20), 0.45),
    (Vec3::new(3.05, 9.5, -3.00), 0.38),
];

pub(crate) fn world_scene() -> impl Scene {
    let stone = Vec3::new(0.30, 0.32, 0.35);
    let clay = Vec3::new(0.72, 0.36, 0.22);
    let moss = Vec3::new(0.28, 0.48, 0.30);

    bsn! {
        SdfWorld Transform
        Children [

            (template_value(Brush)
             Transform { translation: {Vec3::new(0.0, -0.5, 0.0)}, scale: {Vec3::new(12.0, 0.5, 12.0)} }
             Albedo({stone})),

            (template_value(Brush)
             Transform { translation: {Vec3::new(0.0, 1.0, -6.0)}, scale: {Vec3::new(12.0, 1.5, 0.5)} }
             CsgOperation { radius: 0.4 }
             Albedo({stone})),
            (template_value(Brush)
             Transform { translation: {Vec3::new(-6.0, 1.0, 0.0)}, scale: {Vec3::new(0.5, 1.5, 12.0)} }
             CsgOperation { radius: 0.4 }
             Albedo({stone})),

            (template_value(Brush)
             Transform { translation: {Vec3::new(-3.0, 1.2, -3.0)}, scale: {Vec3::new(0.8, 1.2, 0.8)} }
             Modifiers { round: 0.35 }
             Albedo({clay})),

            (template_value(Brush)
             Transform { translation: {Vec3::new(0.0, 1.0, -3.0)}, scale: {Vec3::new(1.0, 1.0, 1.0)} }
             Modifiers { bevel: 1.0, thickness: 0.4 }
             Albedo({clay})),

            (template_value(Brush)
             Transform { translation: {Vec3::new(3.0, 1.0, -3.0)}, scale: {Vec3::new(1.2, 1.0, 1.2)} }
             Modifiers { cone: 0.6, thickness: 0.3 }
             Albedo({clay})),

            (template_value(Brush)
             Transform { translation: {Vec3::new(3.0, 0.4, 2.0)}, scale: {Vec3::new(1.6, 0.4, 1.0)} }
             Albedo({moss})),
            (template_value(Brush)
             Transform { translation: {Vec3::new(3.0, 0.9, 2.0)}, scale: {Vec3::splat(0.7)} }
             Modifiers { round: 1.0 }
             CsgOperation { radius: 0.5 }
             Albedo({moss})),

            (template_value(Brush)
             Transform { translation: {Vec3::new(-3.0, 0.8, 2.0)}, scale: {Vec3::new(0.9, 0.8, 0.9)} }
             Modifiers { bevel: 1.0, round: 0.2 }
             Albedo({moss})),
            (template_value(Brush)
             Transform { translation: {Vec3::new(-3.0, 1.5, 2.0)}, scale: {Vec3::splat(0.6)} }
             Modifiers { round: 1.0 }
             CsgOperation { mode: {GPU_MODE_SUBTRACT}, radius: 0.15 }
             Albedo({moss})),
        ]
    }
}
