//! The authored world.
//!
//! Written in `bsn!` rather than imported: a brush is an entity, so it can
//! carry anything an entity can. Children blend in the order they appear.

use bevy::prelude::*;

use crate::field::{Albedo, CsgOperation, Modifiers, SdfShape, GPU_MODE_SUBTRACT};

pub(crate) struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_world);
    }
}

/// Root of the authored world. Its `Children` are the brushes, in the order
/// they blend.
#[derive(Component, Default, Clone)]
pub(crate) struct SdfWorld;

fn spawn_world(mut commands: Commands) {
    commands.spawn_scene(world_scene());
}

// ============================================================ authored world

/// The world, written as a scene rather than imported.
///
/// Children blend in the order they appear: each one combines with everything
/// above it, and the first simply seeds the field. Moving a shape up or down
/// this list is a real edit, not a reordering.
pub(crate) fn world_scene() -> impl Scene {
    let stone = Vec3::new(0.30, 0.32, 0.35);
    let clay = Vec3::new(0.72, 0.36, 0.22);
    let moss = Vec3::new(0.28, 0.48, 0.30);

    bsn! {
        // The root needs a Transform of its own: propagation only reaches
        // children through a parent that has one, and without it every child's
        // GlobalTransform stays identity - the whole world stacked at origin.
        SdfWorld Transform
        Children [
            // Floor first: it seeds the field, and it is what the bodies land
            // on. Without something under them they fall forever, and because
            // bodies are shapes the culling box follows them down.
            (template_value(SdfShape::Cube)
             Transform { translation: {Vec3::new(0.0, -0.5, 0.0)}, scale: {Vec3::new(12.0, 0.5, 12.0)} }
             Albedo({stone})),

            // Two walls, softly welded to the floor.
            (template_value(SdfShape::Cube)
             Transform { translation: {Vec3::new(0.0, 1.0, -6.0)}, scale: {Vec3::new(12.0, 1.5, 0.5)} }
             CsgOperation { radius: 0.4 }
             Albedo({stone})),
            (template_value(SdfShape::Cube)
             Transform { translation: {Vec3::new(-6.0, 1.0, 0.0)}, scale: {Vec3::new(0.5, 1.5, 12.0)} }
             CsgOperation { radius: 0.4 }
             Albedo({stone})),

            // A rounded pillar.
            (template_value(SdfShape::Cube)
             Transform { translation: {Vec3::new(-3.0, 1.2, -3.0)}, scale: {Vec3::new(0.8, 1.2, 0.8)} }
             Modifiers { round: 0.35 }
             Albedo({clay})),

            // A tube: hollow, open at both ends.
            (template_value(SdfShape::Cube)
             Transform { translation: {Vec3::new(0.0, 1.0, -3.0)}, scale: {Vec3::new(1.0, 1.0, 1.0)} }
             Modifiers { bevel: 1.0, thickness: 0.4 }
             Albedo({clay})),

            // A funnel: tapered and hollow, so the bore is a slit at the top
            // and the base is solid.
            (template_value(SdfShape::Cube)
             Transform { translation: {Vec3::new(3.0, 1.0, -3.0)}, scale: {Vec3::new(1.2, 1.0, 1.2)} }
             Modifiers { cone: 0.6, thickness: 0.3 }
             Albedo({clay})),

            // A sphere welded into a slab with a wide blend.
            (template_value(SdfShape::Cube)
             Transform { translation: {Vec3::new(3.0, 0.4, 2.0)}, scale: {Vec3::new(1.6, 0.4, 1.0)} }
             Albedo({moss})),
            (template_value(SdfShape::Sphere)
             Transform { translation: {Vec3::new(3.0, 0.9, 2.0)}, scale: {Vec3::splat(0.7)} }
             CsgOperation { radius: 0.5 }
             Albedo({moss})),

            // A cylinder, then a sphere subtracted out of it. The subtract has
            // to come after its target to have anything to carve.
            (template_value(SdfShape::Cylinder)
             Transform { translation: {Vec3::new(-3.0, 0.8, 2.0)}, scale: {Vec3::new(0.9, 0.8, 0.9)} }
             Modifiers { round: 0.2 }
             Albedo({moss})),
            (template_value(SdfShape::Sphere)
             Transform { translation: {Vec3::new(-3.0, 1.5, 2.0)}, scale: {Vec3::splat(0.6)} }
             CsgOperation { mode: {GPU_MODE_SUBTRACT}, radius: 0.15 }
             Albedo({moss})),
        ]
    }
}
