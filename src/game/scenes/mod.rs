pub mod gym;
pub mod museum;
pub mod showcase;
pub mod zoo;

use bevy::prelude::*;

use crate::command_line;
use crate::sdf::brush::Brush;
use crate::sdf::render::MainCamera;

#[derive(Component, Debug, Clone)]
pub(crate) struct Exhibit {
    pub(crate) name: String,
    pub(crate) note: String,
}

impl Exhibit {
    pub(crate) fn new(name: &str, note: &str) -> Self {
        Exhibit {
            name: name.to_string(),
            note: note.to_string(),
        }
    }
}

#[derive(Component, Default, Clone)]
pub(crate) struct SdfWorld;

#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Scene {
    #[default]
    Showcase,
    Zoo,
    Museum,
    Gym,
}

pub(crate) fn requested() -> Scene {
    match command_line::text("--scene").as_deref() {
        None | Some("showcase") => Scene::Showcase,
        Some("zoo") => Scene::Zoo,
        Some("museum") => Scene::Museum,
        Some("gym") => Scene::Gym,
        Some(other) => {
            eprintln!("scene: unknown {other:?}, expected showcase, zoo, museum or gym");
            std::process::exit(2);
        }
    }
}

impl Scene {
    pub(crate) fn viewpoint(self) -> Transform {
        match self {
            Scene::Showcase => Transform::from_xyz(0.0, 2.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y),
            Scene::Zoo => {
                Transform::from_xyz(0.0, 13.0, 24.0).looking_at(Vec3::new(0.0, 0.0, 2.0), Vec3::Y)
            }
            Scene::Museum => {
                Transform::from_xyz(0.0, 7.0, 19.0).looking_at(Vec3::new(0.0, 1.0, 1.0), Vec3::Y)
            }
            Scene::Gym => {
                Transform::from_xyz(0.0, 9.0, 17.0).looking_at(Vec3::new(0.0, 0.0, -3.0), Vec3::Y)
            }
        }
    }
}

fn place_camera(scene: Res<Scene>, camera: Single<&mut Transform, With<MainCamera>>) {
    *camera.into_inner() = scene.viewpoint();
}

pub(crate) struct ScenePlugin(pub(crate) Scene);

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.0)
            .add_systems(PostStartup, place_camera);
        match self.0 {
            Scene::Showcase => {
                app.add_systems(Startup, (showcase::spawn, showcase::spawn_lights));
            }
            Scene::Zoo => {
                app.add_systems(Startup, (zoo::spawn, zoo::spawn_lights));
            }
            Scene::Museum => {
                app.add_systems(Startup, (museum::spawn, museum::spawn_lights));
            }
            Scene::Gym => {
                app.add_systems(Startup, (gym::spawn, gym::spawn_lights));
            }
        }
    }
}

pub(crate) fn nearest_exhibit(
    point: Vec3,
    exhibits: &Query<(&Exhibit, &GlobalTransform)>,
) -> Option<(String, String, f32)> {
    exhibits
        .iter()
        .map(|(exhibit, placement)| {
            (
                exhibit.name.clone(),
                exhibit.note.clone(),
                placement.translation().distance(point),
            )
        })
        .min_by(|a, b| a.2.total_cmp(&b.2))
}

pub(crate) fn floor(half: Vec3, height: f32) -> impl Bundle {
    (
        Brush,
        Transform::from_xyz(0.0, height - half.y, 0.0).with_scale(half),
        crate::sdf::brush::Albedo(Vec3::new(0.30, 0.32, 0.35)),
    )
}
