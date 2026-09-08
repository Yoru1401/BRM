use bevy::{
    asset::RenderAssetUsages,
    gltf::GltfAssetLabel,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
    render::storage::ShaderBuffer,
};

use crate::command_line;
use crate::sdf::adf::{self, Adf, Material, Surface};
use crate::sdf::render::{MainCamera, SdfMaterial, spawn_quad};
use crate::sdf::scenes::{self, Scene};
use crate::sdf::solid::{Shape, Solid};

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
struct Pending {
    mesh: Option<Handle<Mesh>>,
    marks: Vec<u8>,
    parts: Vec<u16>,
    solids: Vec<Solid>,
}

fn request_model(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let pending = match command_line::text("--model") {
        Some(path) => Pending {
            mesh: Some(assets.load(
                GltfAssetLabel::Primitive {
                    mesh: 0,
                    primitive: 0,
                }
                .from_asset(path),
            )),
            marks: Vec::new(),
            parts: Vec::new(),
            solids: Vec::new(),
        },
        None => match scenes::chosen() {
            Scene::Play => {
                let (mesh, marks, parts, solids) = open_world();
                Pending {
                    mesh: Some(meshes.add(mesh)),
                    marks,
                    parts,
                    solids,
                }
            }
            scene => Pending {
                mesh: None,
                marks: Vec::new(),
                parts: Vec::new(),
                solids: scenes::build(scene),
            },
        },
    };
    commands.insert_resource(pending);
}

pub(crate) const MATERIAL_TERRAIN: u32 = 0;
pub(crate) const MATERIAL_STONE: u32 = 1;
pub(crate) const MATERIAL_METAL: u32 = 2;
pub(crate) const MATERIAL_CHALK: u32 = 3;
pub(crate) const MATERIAL_CLAY: u32 = 4;
pub(crate) const MATERIAL_GLASS: u32 = 5;
pub(crate) const MATERIAL_BODY: u32 = 6;
pub(crate) const MATERIAL_PLAYER: u32 = 7;

fn world_palette() -> Vec<Material> {
    [
        ([0.42, 0.46, 0.35], 0.02),
        ([0.62, 0.60, 0.57], 0.10),
        ([0.55, 0.57, 0.62], 0.85),
        ([0.88, 0.86, 0.80], 0.05),
        ([0.72, 0.42, 0.28], 0.15),
        ([0.45, 0.62, 0.68], 0.65),
        ([0.90, 0.35, 0.18], 0.30),
        ([0.95, 0.82, 0.25], 0.40),
    ]
    .iter()
    .map(|(albedo, gloss)| Material {
        albedo: Vec3::from(*albedo),
        gloss: *gloss,
    })
    .collect()
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

fn open_world() -> (Mesh, Vec<u8>, Vec<u16>, Vec<Solid>) {
    let world = terrain();
    let marks: Vec<u8> = vec![MATERIAL_TERRAIN as u8; triangle_count(&world)];
    let parts: Vec<u16> = vec![0; marks.len()];

    let mut solids: Vec<Solid> = Vec::new();
    let mut next_part = 1u16;
    let mut plant = |shape: Shape, centre: Vec3, turn: Quat, material: u32| {
        solids.push(Solid::new(shape, centre, turn, material as u8, next_part));
        next_part = next_part.wrapping_add(1);
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
        let scale = 1.3 + random() * random() * 3.4;

        match index % 5 {
            0 | 1 => {
                let height = 6.0 * scale;
                plant(
                    Shape::Box {
                        half: Vec3::new(2.0 * scale, height * 0.5, 2.0 * scale),
                    },
                    Vec3::new(x, floor + height * 0.4, z),
                    Quat::from_rotation_y(random() * 3.0),
                    MATERIAL_STONE,
                );
            }
            2 => {
                let height = 10.0 * scale;
                plant(
                    Shape::Cylinder {
                        radius: 1.4 * scale,
                        half_height: height * 0.5,
                    },
                    Vec3::new(x, floor + height * 0.35, z),
                    Quat::IDENTITY,
                    MATERIAL_METAL,
                );
            }
            3 => plant(
                Shape::Sphere { radius: 2.5 * scale },
                Vec3::new(x, floor + 1.2 * scale, z),
                Quat::IDENTITY,
                MATERIAL_CHALK,
            ),
            _ => plant(
                Shape::Frustum {
                    top: 0.5 * scale,
                    bottom: 3.0 * scale,
                    half_height: 4.0 * scale,
                },
                Vec3::new(x, floor + 3.0 * scale, z),
                Quat::IDENTITY,
                MATERIAL_CLAY,
            ),
        }
    }

    for slot in 0..4 {
        let turn = slot as f32 / 4.0 * std::f32::consts::TAU;
        let (x, z) = (turn.cos() * 60.0, turn.sin() * 60.0);
        plant(
            Shape::Torus {
                major: 7.0,
                minor: 5.0,
            },
            Vec3::new(x, ground(x, z) + 10.0, z),
            Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
            MATERIAL_GLASS,
        );
    }
    (world, marks, parts, solids)
}

fn triangle_count(mesh: &Mesh) -> usize {
    mesh.indices().map_or(0, |list| list.len() / 3)
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
    let (mut triangles, sources) = match &pending.mesh {
        Some(handle) => match meshes.get(handle) {
            Some(mesh) => adf::triangles_of(mesh),
            None => return,
        },
        None => (Vec::new(), Vec::new()),
    };
    let marks: Vec<u8> = sources
        .iter()
        .map(|source| pending.marks.get(*source as usize).copied().unwrap_or(0))
        .collect();
    let parts: Vec<u16> = sources
        .iter()
        .map(|source| pending.parts.get(*source as usize).copied().unwrap_or(0))
        .collect();
    let mut solids = pending.solids.clone();
    commands.remove_resource::<Pending>();
    if triangles.is_empty() && solids.is_empty() {
        error!("scene has no geometry; nothing to bake");
        return;
    }

    adf::fit(
        &mut triangles,
        &mut solids,
        command_line::value("--size")
            .or_else(|| scenes::chosen().size())
            .unwrap_or(match command_line::flag("--play") {
                true => PLAY_SIZE,
                false => MODEL_SIZE,
            }),
    );
    let started = std::time::Instant::now();
    *adf = adf::bake(&Surface::new(&triangles, &marks, &parts).with_solids(&solids));
    adf.palette = world_palette();
    let (low, high) = adf.bounds();
    info!(
        "adf: {} triangles, {} bricks of {}, {:.3} m voxels, {} m across, {} MB atlas, {} MB paint, {} MB page, baked in {:.2} s",
        triangles.len(),
        adf.used,
        adf::brick_budget(),
        adf.voxel,
        (high - low).max_element().round(),
        adf.atlas.len() >> 20,
        adf.paint.len() >> 20,
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
    let eye = command_line::triple("--eye")
        .map_or(centre + Vec3::new(0.0, reach * 0.35, reach * 0.8), Vec3::from);
    let look = command_line::triple("--look").map_or(centre, Vec3::from);
    Transform::from_translation(eye).looking_at(look, Vec3::Y)
}
