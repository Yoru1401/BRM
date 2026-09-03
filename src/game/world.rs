use bevy::prelude::*;

use crate::sdf::brush::{Albedo, Brush, CsgOperation, GPU_MODE_SUBTRACT, Modifiers};
use crate::sdf::light::{Light, LightKind};

pub(crate) struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, (spawn_world, spawn_lights));
    }
}

fn spawn_lights(mut commands: Commands) {
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

#[derive(Component, Default, Clone)]
pub(crate) struct SdfWorld;

fn spawn_world(mut commands: Commands) {
    commands.spawn_scene(world_scene());
}

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
