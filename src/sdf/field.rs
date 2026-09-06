use bevy::{
    asset::RenderAssetUsages,
    gltf::GltfAssetLabel,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
    render::storage::ShaderBuffer,
};

use crate::command_line;
use crate::sdf::adf::{self, Adf};
use crate::sdf::render::{MainCamera, SdfMaterial, spawn_quad};

pub(crate) struct FieldPlugin;

impl Plugin for FieldPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Adf>()
            .add_systems(Startup, request_model)
            .add_systems(Update, bake_when_ready.run_if(resource_exists::<Pending>));
    }
}

const MODEL_SIZE: f32 = 1000.0;
const PLAY_SIZE: f32 = 200.0;

#[derive(Resource)]
struct Pending(Handle<Mesh>);

fn request_model(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let handle = match command_line::text("--model") {
        Some(path) => assets.load(
            GltfAssetLabel::Primitive {
                mesh: 0,
                primitive: 0,
            }
            .from_asset(path),
        ),
        None => meshes.add(placeholder()),
    };
    commands.insert_resource(Pending(handle));
}

const TERRAIN_QUADS: usize = 192;
const TERRAIN_HALF: f32 = 150.0;
const TERRAIN_RELIEF: f32 = 22.0;
const TERRAIN_FLOOR: f32 = -12.0;
const STRUCTURES: usize = 220;

fn ground(x: f32, z: f32) -> f32 {
    let (mut height, mut amplitude, mut frequency) = (0.0, 1.0, 0.011);
    for _ in 0..5 {
        height += amplitude
            * ((x * frequency * 0.9).sin() * (z * frequency * 1.1 + 0.7).cos()
                + (x * frequency * 1.7 + 2.0).sin() * (z * frequency * 0.6 - 1.0).sin() * 0.6);
        amplitude *= 0.5;
        frequency *= 2.1;
    }
    height * TERRAIN_RELIEF * 0.6
}

fn terrain() -> Mesh {
    let side = TERRAIN_QUADS + 1;
    let step = TERRAIN_HALF * 2.0 / TERRAIN_QUADS as f32;
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(side * side);
    for row in 0..side {
        for column in 0..side {
            let x = -TERRAIN_HALF + column as f32 * step;
            let z = -TERRAIN_HALF + row as f32 * step;
            positions.push([x, ground(x, z), z]);
        }
    }

    let at = |row: usize, column: usize| (row * side + column) as u32;
    let mut indices: Vec<u32> = Vec::with_capacity(TERRAIN_QUADS * TERRAIN_QUADS * 6);
    for row in 0..TERRAIN_QUADS {
        for column in 0..TERRAIN_QUADS {
            let (a, b, c, d) = (
                at(row, column),
                at(row, column + 1),
                at(row + 1, column + 1),
                at(row + 1, column),
            );
            indices.extend_from_slice(&[a, c, b, a, d, c]);
        }
    }

    let mut skirt = |edge: Vec<u32>| {
        for pair in edge.windows(2) {
            let (top_one, top_two) = (pair[0], pair[1]);
            let low_one = positions.len() as u32;
            let low_two = low_one + 1;
            positions.push([
                positions[top_one as usize][0],
                TERRAIN_FLOOR,
                positions[top_one as usize][2],
            ]);
            positions.push([
                positions[top_two as usize][0],
                TERRAIN_FLOOR,
                positions[top_two as usize][2],
            ]);
            indices.extend_from_slice(&[top_one, low_one, low_two, top_one, low_two, top_two]);
        }
    };
    skirt((0..side).map(|column| at(0, column)).collect());
    skirt((0..side).rev().map(|column| at(side - 1, column)).collect());
    skirt((0..side).rev().map(|row| at(row, 0)).collect());
    skirt((0..side).map(|row| at(row, side - 1)).collect());

    let base = positions.len() as u32;
    for (x, z) in [
        (-TERRAIN_HALF, -TERRAIN_HALF),
        (TERRAIN_HALF, -TERRAIN_HALF),
        (TERRAIN_HALF, TERRAIN_HALF),
        (-TERRAIN_HALF, TERRAIN_HALF),
    ] {
        positions.push([x, TERRAIN_FLOOR, z]);
    }
    indices.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);

    let count = positions.len();
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 1.0, 0.0]; count]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32, 0.0]; count]);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

fn placeholder() -> Mesh {
    let mut world = terrain();
    let mut add = |part: Mesh, placement: Transform| {
        let _ = world.merge(&part.transformed_by(placement));
    };

    let mut seed = 0x9e37_79b9u32;
    let mut random = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed >> 8) as f32 / 16_777_216.0
    };

    for index in 0..STRUCTURES {
        let x = (random() - 0.5) * TERRAIN_HALF * 1.9;
        let z = (random() - 0.5) * TERRAIN_HALF * 1.9;
        let floor = ground(x, z);
        let scale = 0.6 + random() * random() * 4.0;

        match index % 5 {
            0 | 1 => {
                let height = 6.0 * scale;
                add(
                    Mesh::from(Cuboid::new(4.0 * scale, height, 4.0 * scale)),
                    Transform::from_xyz(x, floor + height * 0.4, z)
                        .with_rotation(Quat::from_rotation_y(random() * 3.0)),
                );
            }
            2 => {
                let height = 10.0 * scale;
                add(
                    Cylinder::new(1.4 * scale, height).mesh().resolution(20).build(),
                    Transform::from_xyz(x, floor + height * 0.35, z),
                );
            }
            3 => add(
                Sphere::new(2.5 * scale).mesh().ico(3).unwrap(),
                Transform::from_xyz(x, floor + 1.2 * scale, z),
            ),
            _ => add(
                Mesh::from(ConicalFrustum {
                    radius_top: 0.5 * scale,
                    radius_bottom: 3.0 * scale,
                    height: 8.0 * scale,
                }),
                Transform::from_xyz(x, floor + 3.0 * scale, z),
            ),
        }
    }

    for slot in 0..4 {
        let turn = slot as f32 / 4.0 * std::f32::consts::TAU;
        let (x, z) = (turn.cos() * 60.0, turn.sin() * 60.0);
        add(
            Torus::new(2.0, 12.0)
                .mesh()
                .major_resolution(48)
                .minor_resolution(16)
                .build(),
            Transform::from_xyz(x, ground(x, z) + 10.0, z)
                .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
        );
    }
    world
}

#[allow(clippy::too_many_arguments)]
fn bake_when_ready(
    mut commands: Commands,
    pending: Res<Pending>,
    camera: Single<(Entity, &mut Transform), With<MainCamera>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<SdfMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut adf: ResMut<Adf>,
) {
    let Some(mesh) = meshes.get(&pending.0) else {
        return;
    };
    let mut triangles = adf::triangles_of(mesh);
    commands.remove_resource::<Pending>();
    if triangles.is_empty() {
        error!("model has no triangles; nothing to bake");
        return;
    }

    adf::fit(
        &mut triangles,
        command_line::value("--size").unwrap_or(match command_line::flag("--play") {
            true => PLAY_SIZE,
            false => MODEL_SIZE,
        }),
    );
    let started = std::time::Instant::now();
    *adf = adf::bake(&triangles);
    let (low, high) = adf.bounds();
    info!(
        "adf: {} triangles, {} bricks of {}, {:.3} m voxels, {} m across, {} MB atlas, {} MB page, baked in {:.2} s",
        triangles.len(),
        adf.used,
        adf::brick_budget(),
        adf.voxel,
        (high - low).max_element().round(),
        adf.atlas.len() >> 20,
        (adf.page.len() * 4) >> 20,
        started.elapsed().as_secs_f32()
    );

    let (entity, mut placement) = camera.into_inner();
    *placement = viewpoint(&adf);
    spawn_quad(
        &mut commands,
        entity,
        &adf,
        &mut meshes,
        &mut materials,
        &mut images,
        &mut buffers,
    );
}

fn viewpoint(adf: &Adf) -> Transform {
    let (low, high) = adf.bounds();
    let centre = (low + high) * 0.5;
    let reach = (high - low).max_element();
    Transform::from_translation(centre + Vec3::new(0.0, reach * 0.35, reach * 0.8))
        .looking_at(centre, Vec3::Y)
}
