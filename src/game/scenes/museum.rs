use bevy::prelude::*;

use crate::game::scenes::{Exhibit, SdfWorld, floor};
use crate::sdf::brush::{
    Albedo, Brush, CsgOperation, GPU_MODE_ADD, GPU_MODE_AVOID, GPU_MODE_DEBOSS, GPU_MODE_EMBOSS,
    GPU_MODE_PAINT, GPU_MODE_PUSH, GPU_MODE_SUBTRACT, Modifiers,
};
use crate::sdf::light::{Light, LightKind};

const SPACING: f32 = 4.0;
const STAND: f32 = 1.2;
const STRENGTH: f32 = 0.25;
const SPHERE: f32 = 0.6;

const STAND_ALBEDO: Vec3 = Vec3::new(0.72, 0.36, 0.22);
const INCOMING_ALBEDO: Vec3 = Vec3::new(0.28, 0.48, 0.30);

struct Mode {
    name: &'static str,
    note: &'static str,
    mode: u32,
}

const MODES: [Mode; 7] = [
    Mode {
        name: "add",
        note: "the nearer of the two, so the sphere simply joins the box",
        mode: GPU_MODE_ADD,
    },
    Mode {
        name: "subtract",
        note: "the sphere carves its own half out of the box and leaves a bowl",
        mode: GPU_MODE_SUBTRACT,
    },
    Mode {
        name: "paint",
        note: "the field is returned untouched; only the albedo lookup changes, \
               so the green patch has no geometry behind it",
        mode: GPU_MODE_PAINT,
    },
    Mode {
        name: "push",
        note: "the sphere is added and a gap one strength wide is carved around it, \
               so it sits in the box without touching it",
        mode: GPU_MODE_PUSH,
    },
    Mode {
        name: "avoid",
        note: "push with the roles swapped: the sphere is solid only where it is \
               more than one strength away from what was already there",
        mode: GPU_MODE_AVOID,
    },
    Mode {
        name: "emboss",
        note: "raises a lip one strength proud of the box wherever the sphere reaches it",
        mode: GPU_MODE_EMBOSS,
    },
    Mode {
        name: "deboss",
        note: "the same offset the other way: a groove one strength deep",
        mode: GPU_MODE_DEBOSS,
    },
];

const SOLID: Modifiers = Modifiers {
    round: 0.0,
    bevel: 0.0,
    thickness: 1.0,
    cone: 0.0,
};

fn stand(x: f32) -> impl Bundle {
    (
        Brush,
        SOLID,
        Albedo(STAND_ALBEDO),
        Transform::from_xyz(x, STAND, 0.0),
    )
}

fn incoming(x: f32, mode: u32) -> impl Bundle {
    (
        Brush,
        Modifiers {
            round: 1.0,
            ..SOLID
        },
        CsgOperation {
            mode,
            strength: STRENGTH,
            ..default()
        },
        Albedo(INCOMING_ALBEDO),
        Transform::from_xyz(x, STAND + 0.15, 1.0).with_scale(Vec3::splat(SPHERE)),
    )
}

pub(crate) fn spawn(mut commands: Commands) {
    let span = (MODES.len() - 1) as f32 * SPACING;

    commands
        .spawn((SdfWorld, Transform::default()))
        .with_children(|root| {
            root.spawn(floor(Vec3::new(span * 0.7, 0.5, 14.0), 0.0));

            for (index, entry) in MODES.iter().enumerate() {
                let x = index as f32 * SPACING - span * 0.5;
                root.spawn(stand(x));
                root.spawn(incoming(x, entry.mode));
                root.spawn((
                    Exhibit::new(entry.name, entry.note),
                    Transform::from_xyz(x, STAND, 0.0),
                ));
            }

            root.spawn((
                Exhibit::new(
                    "intersect and shell are not on show",
                    "both return the incoming shape far away from it, so each one folds \
                     the entire field and erases every exhibit declared before it. \
                     A scene can hold one of them or the other, and nothing else",
                ),
                Transform::from_xyz(-span * 0.5 - SPACING, STAND, 0.0),
            ));

            let far = 7.0;
            root.spawn((
                Brush,
                Modifiers { cone: 1.0, ..SOLID },
                Albedo(Vec3::new(0.85, 0.70, 0.35)),
                Transform::from_xyz(-6.0, 1.5, far).with_scale(Vec3::splat(1.5)),
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
                Transform::from_xyz(0.0, 1.5, far).with_scale(Vec3::splat(1.5)),
            ));
            root.spawn((
                Exhibit::new(
                    "a wall runs through the height",
                    "thickness hollows laterally and leaves both ends open: a tube, not a cup. \
                     The bore is empty to the marcher and solid to the shadow proxy, \
                     which is why no light reaches down it",
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
