use bevy::prelude::*;

use crate::game::scenes::{Exhibit, SdfWorld, floor};
use crate::sdf::brush::{Albedo, Brush, Modifiers};
use crate::sdf::light::{Light, LightKind};

const STEPS: usize = 7;
const SPACING: f32 = 2.6;
const SPECIMEN: f32 = 0.8;
const ROW_GAP: f32 = 3.4;

struct Axis {
    name: &'static str,
    note: &'static str,
    apply: fn(f32) -> Modifiers,
    albedo: Vec3,
}

const AXES: [Axis; 4] = [
    Axis {
        name: "round",
        note: "every edge at once; 1.0 on a cube is an exact sphere",
        apply: |t| Modifiers { round: t, ..SOLID },
        albedo: Vec3::new(0.85, 0.45, 0.30),
    },
    Axis {
        name: "bevel",
        note: "the four vertical edges only; 1.0 is an exact cylinder",
        apply: |t| Modifiers { bevel: t, ..SOLID },
        albedo: Vec3::new(0.35, 0.60, 0.85),
    },
    Axis {
        name: "cone",
        note: "narrows the top; 1.0 closes it to a point, so a box becomes a pyramid",
        apply: |t| Modifiers { cone: t, ..SOLID },
        albedo: Vec3::new(0.90, 0.75, 0.30),
    },
    Axis {
        name: "thickness",
        note: "1.0 is solid, below that a shell; the bore runs through the height",
        apply: |t| Modifiers {
            thickness: 1.0 - t,
            ..SOLID
        },
        albedo: Vec3::new(0.45, 0.75, 0.45),
    },
];

const SOLID: Modifiers = Modifiers {
    round: 0.0,
    bevel: 0.0,
    thickness: 1.0,
    cone: 0.0,
};

fn fraction(step: usize) -> f32 {
    step as f32 / (STEPS - 1) as f32
}

pub(crate) fn spawn(mut commands: Commands) {
    let span = (STEPS - 1) as f32 * SPACING;
    let rows = AXES.len() as f32;

    commands
        .spawn((SdfWorld, Transform::default()))
        .with_children(|root| {
            root.spawn(floor(Vec3::new(span * 0.75, 0.5, 20.0), 0.0));

            for (row, axis) in AXES.iter().enumerate() {
                let z = row as f32 * ROW_GAP - (rows - 1.0) * ROW_GAP * 0.5;
                for step in 0..STEPS {
                    let t = fraction(step);
                    let x = step as f32 * SPACING - span * 0.5;
                    root.spawn((
                        Brush,
                        (axis.apply)(t),
                        Albedo(axis.albedo),
                        Transform::from_xyz(x, SPECIMEN, z).with_scale(Vec3::splat(SPECIMEN)),
                    ));
                }
                root.spawn((
                    Exhibit::new(axis.name, axis.note),
                    Transform::from_xyz(-span * 0.5 - SPACING, SPECIMEN, z),
                ));
            }

            let plane_z = rows * ROW_GAP * 0.5 + ROW_GAP * 0.6;
            let side = 5;
            for row in 0..side {
                for column in 0..side {
                    let round = row as f32 / (side - 1) as f32;
                    let cone = column as f32 / (side - 1) as f32;
                    root.spawn((
                        Brush,
                        Modifiers {
                            round,
                            cone,
                            ..SOLID
                        },
                        Albedo(Vec3::new(0.70, 0.70, 0.75)),
                        Transform::from_xyz(
                            column as f32 * SPACING - (side - 1) as f32 * SPACING * 0.5,
                            SPECIMEN,
                            plane_z + row as f32 * SPACING,
                        )
                        .with_scale(Vec3::splat(SPECIMEN)),
                    ));
                }
            }
            root.spawn((
                Exhibit::new(
                    "round x cone",
                    "the two that compete for the same footprint; asking for both clamps",
                ),
                Transform::from_xyz(
                    -(side as f32) * SPACING * 0.5 - SPACING,
                    SPECIMEN,
                    plane_z + 2.0 * SPACING,
                ),
            ));
        });
}

pub(crate) fn spawn_lights(mut commands: Commands) {
    commands.spawn((
        Light {
            kind: LightKind::Directional,
            colour: Vec3::new(1.0, 0.97, 0.92),
            intensity: 1.0,
            shadow: true,
            softness: 14.0,
            ..default()
        },
        Transform::from_xyz(6.0, 14.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        Light {
            kind: LightKind::Directional,
            colour: Vec3::new(0.45, 0.55, 0.75),
            intensity: 0.35,
            ..default()
        },
        Transform::from_xyz(-8.0, 6.0, -6.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}
