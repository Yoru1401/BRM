use bevy::prelude::*;

use crate::game::scenes::{Exhibit, SdfWorld, floor};
use crate::sdf::brush::{
    Albedo, Brush, CsgOperation, GPU_MODE_ADD, GPU_MODE_AVOID, GPU_MODE_DEBOSS, GPU_MODE_EMBOSS,
    GPU_MODE_INTERSECT, GPU_MODE_PAINT, GPU_MODE_PUSH, GPU_MODE_SHELL, GPU_MODE_SUBTRACT,
    Modifiers,
};
use crate::sdf::light::{Light, LightKind};

const SPACING: f32 = 4.0;
const STAND: f32 = 1.2;

struct Mode {
    name: &'static str,
    note: &'static str,
    mode: u32,
}

const GLOBAL: [Mode; 2] = [
    Mode {
        name: "intersect (folds the whole field)",
        note: "max of the two, so far from the incoming shape it returns the shape, not the                field: everything folded before it is gone. That is why it is first here.",
        mode: GPU_MODE_INTERSECT,
    },
    Mode {
        name: "shell (folds the whole field)",
        note: "built on intersect, so it consumes the field the same way",
        mode: GPU_MODE_SHELL,
    },
];

const MODES: [Mode; 7] = [
    Mode {
        name: "add",
        note: "the nearer of the two; the only mode the cull reject is sound for",
        mode: GPU_MODE_ADD,
    },
    Mode {
        name: "subtract",
        note: "carves the incoming shape out of the field",
        mode: GPU_MODE_SUBTRACT,
    },
    Mode {
        name: "paint",
        note: "recolours and leaves the field exactly as it was",
        mode: GPU_MODE_PAINT,
    },
    Mode {
        name: "push",
        note: "offsets the field outwards where the shape reaches",
        mode: GPU_MODE_PUSH,
    },
    Mode {
        name: "avoid",
        note: "pushes the field away from the shape instead of into it",
        mode: GPU_MODE_AVOID,
    },
    Mode {
        name: "emboss",
        note: "raises a lip where the shape meets the surface",
        mode: GPU_MODE_EMBOSS,
    },
    Mode {
        name: "deboss",
        note: "sinks a groove where the shape meets the surface",
        mode: GPU_MODE_DEBOSS,
    },
];

const SOLID: Modifiers = Modifiers {
    round: 0.0,
    bevel: 0.0,
    thickness: 1.0,
    cone: 0.0,
};

pub(crate) fn spawn(mut commands: Commands) {
    let span = (MODES.len() - 1) as f32 * SPACING;

    commands
        .spawn((SdfWorld, Transform::default()))
        .with_children(|root| {
            for (index, entry) in GLOBAL.iter().enumerate() {
                let z = -8.0 - index as f32 * 4.0;
                root.spawn((
                    Brush,
                    SOLID,
                    Albedo(Vec3::new(0.72, 0.36, 0.22)),
                    Transform::from_xyz(0.0, STAND, z),
                ));
                root.spawn((
                    Brush,
                    Modifiers {
                        round: 1.0,
                        ..SOLID
                    },
                    CsgOperation {
                        mode: entry.mode,
                        radius: 0.25,
                        strength: 0.35,
                        ..default()
                    },
                    Albedo(Vec3::new(0.28, 0.48, 0.30)),
                    Transform::from_xyz(0.7, STAND + 0.5, z + 0.4).with_scale(Vec3::splat(0.9)),
                ));
                root.spawn((
                    Exhibit::new(entry.name, entry.note),
                    Transform::from_xyz(0.0, STAND, z),
                ));
            }

            root.spawn(floor(Vec3::new(span * 0.7, 0.5, 14.0), 0.0));

            for (index, entry) in MODES.iter().enumerate() {
                let x = index as f32 * SPACING - span * 0.5;

                root.spawn((
                    Brush,
                    SOLID,
                    Albedo(Vec3::new(0.72, 0.36, 0.22)),
                    Transform::from_xyz(x, STAND, 0.0).with_scale(Vec3::new(1.0, 1.0, 1.0)),
                ));
                root.spawn((
                    Brush,
                    Modifiers {
                        round: 1.0,
                        ..SOLID
                    },
                    CsgOperation {
                        mode: entry.mode,
                        radius: 0.25,
                        strength: 0.25,
                        ..default()
                    },
                    Albedo(Vec3::new(0.28, 0.48, 0.30)),
                    Transform::from_xyz(x + 0.7, STAND + 0.7, 0.6).with_scale(Vec3::splat(0.75)),
                ));
                root.spawn((
                    Exhibit::new(entry.name, entry.note),
                    Transform::from_xyz(x, STAND, 0.0),
                ));
            }

            let far = 7.0;
            root.spawn((
                Brush,
                Modifiers { cone: 1.0, ..SOLID },
                Albedo(Vec3::new(0.85, 0.70, 0.35)),
                Transform::from_xyz(-6.0, 1.5, far).with_scale(Vec3::new(1.5, 1.5, 1.5)),
            ));
            root.spawn((
                Exhibit::new(
                    "the taper is the one estimate that is not exact",
                    "its distance is divided by sqrt(1 + slope^2) to stay an underestimate",
                ),
                Transform::from_xyz(-6.0, 1.5, far),
            ));

            root.spawn((
                Brush,
                Modifiers {
                    bevel: 1.0,
                    thickness: 0.35,
                    ..SOLID
                },
                Albedo(Vec3::new(0.55, 0.55, 0.62)),
                Transform::from_xyz(0.0, 1.5, far).with_scale(Vec3::new(1.5, 1.5, 1.5)),
            ));
            root.spawn((
                Exhibit::new(
                    "a wall runs through the height",
                    "thickness hollows laterally and leaves both ends open: a tube, not a cup",
                ),
                Transform::from_xyz(0.0, 1.5, far),
            ));

            root.spawn((
                Brush,
                Modifiers {
                    cone: 0.6,
                    thickness: 0.3,
                    ..SOLID
                },
                Albedo(Vec3::new(0.62, 0.50, 0.75)),
                Transform::from_xyz(6.0, 1.5, far).with_scale(Vec3::new(1.6, 1.5, 1.6)),
            ));
            root.spawn((
                Exhibit::new(
                    "a tapered shell is a funnel",
                    "the wall carries the unspent taper, so the bore is a slit at the narrow end",
                ),
                Transform::from_xyz(6.0, 1.5, far),
            ));
        });
}

pub(crate) fn spawn_lights(mut commands: Commands) {
    commands.spawn((
        Light {
            kind: LightKind::Directional,
            colour: Vec3::new(1.0, 0.96, 0.90),
            intensity: 0.95,
            shadow: true,
            softness: 12.0,
            ..default()
        },
        Transform::from_xyz(4.0, 12.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        Light {
            kind: LightKind::Point,
            colour: Vec3::new(0.55, 0.65, 1.0),
            intensity: 3.0,
            range: 18.0,
            ..default()
        },
        Transform::from_xyz(0.0, 4.0, -5.0),
    ));
}
