use bevy::prelude::*;

use crate::command_line;
use crate::game::physics::SphereBody;
use crate::sdf::adf::Adf;
use crate::sdf::dynamic::Dynamic;
use crate::sdf::light::{Light, LightKind};
use crate::sdf::field::MATERIAL_BODY;
use crate::sdf::scenes::{self, Scene};

pub(crate) struct ScenePlugin;

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            populate
                .run_if(resource_changed::<Adf>)
                .run_if(field_is_baked),
        );
    }
}

const BODIES: usize = 6;

fn field_is_baked(field: Res<Adf>) -> bool {
    field.used > 0
}

fn populate(mut commands: Commands, field: Res<Adf>) {
    let (low, high) = field.bounds();
    let centre = (low + high) * 0.5;
    let reach = (high - low).max_element();

    commands.spawn((
        Light {
            kind: LightKind::Directional,
            colour: Vec3::new(1.0, 0.94, 0.82),
            intensity: 1.3,
            shadow: true,
            softness: 24.0,
            ..default()
        },
        Transform::default().looking_at(Vec3::new(-0.55, -0.7, -0.45), Vec3::Y),
    ));
    commands.spawn((
        Light {
            kind: LightKind::Directional,
            colour: Vec3::new(0.30, 0.40, 0.62),
            intensity: 0.35,
            ..default()
        },
        Transform::default().looking_at(Vec3::new(0.5, -0.4, 0.6), Vec3::Y),
    ));
    commands.spawn((
        Light {
            kind: LightKind::Spot {
                inner: 0.30,
                outer: 0.55,
            },
            colour: Vec3::new(1.0, 0.55, 0.25),
            intensity: 2.5,
            range: reach * 0.35,
            ..default()
        },
        Transform::from_translation(centre + Vec3::new(0.0, reach * 0.2, reach * 0.12))
            .looking_at(centre, Vec3::Y),
    ));
    commands.spawn((
        Light {
            kind: LightKind::Point,
            colour: Vec3::new(0.6, 0.8, 1.0),
            intensity: 2.0,
            range: reach * 0.12,
            ..default()
        },
        Transform::from_translation(centre + Vec3::new(reach * 0.1, reach * 0.04, -reach * 0.08)),
    ));

    let resting = match scenes::chosen() {
        Scene::Gym | Scene::Zoo => 0,
        Scene::Play | Scene::Museum => BODIES,
    };
    let count = command_line::value("--bodies").map_or(resting, |value| value as usize);
    if count == 0 {
        return;
    }
    let radius = (reach * 0.012).min(field.range() * 0.9);
    for index in 0..count {
        let turn = index as f32 / count as f32 * std::f32::consts::TAU;
        let place = Vec3::new(
            centre.x + turn.cos() * reach * 0.15,
            high.y - radius,
            centre.z + turn.sin() * reach * 0.15,
        );
        commands.spawn((
            SphereBody { radius, ..default() },
            Dynamic::ball(place, radius, MATERIAL_BODY),
            Transform::from_translation(place),
        ));
    }
}
