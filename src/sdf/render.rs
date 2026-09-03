use bevy::{
    prelude::*,
    reflect::TypePath,
    render::{
        render_resource::{AsBindGroup, ShaderType},
        storage::ShaderBuffer,
    },
    shader::ShaderRef,
};

use crate::command_line;
use crate::game::input::Action;
use crate::sdf::brush::{GpuShape, MAX_SHAPES};
use crate::sdf::grid::{GRID_CELL_WORDS, GRID_INDEX_WORDS};
use crate::sdf::light::{GpuLight, MAX_LIGHTS};

pub(crate) struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<SdfMaterial>::default())
            .add_systems(Startup, (load_shader_modules, spawn_camera))
            .add_systems(Update, (fit_quad, toggle_debug_view, toggle_quad));
    }
}

const SHADER_PATH: &str = "shaders/sdf.wgsl";

const SHADER_MODULES: [&str; 6] = [
    "shaders/bindings.wgsl",
    "shaders/shapes.wgsl",
    "shaders/operations.wgsl",
    "shaders/scene.wgsl",
    "shaders/marching.wgsl",
    "shaders/lighting.wgsl",
];

#[derive(Resource)]
struct ShaderModules(
    #[expect(dead_code, reason = "held to keep the assets alive")] Vec<Handle<Shader>>,
);

fn load_shader_modules(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(ShaderModules(
        SHADER_MODULES
            .iter()
            .map(|path| assets.load(*path))
            .collect(),
    ));
}
const QUAD_DIST: f32 = 1.0;
const QUAD_OVERSCAN: f32 = 1.01;

pub(crate) const OMEGA: f32 = 1.2;

pub(crate) const SHADOW_STEPS: u32 = 48;
pub(crate) const DETAIL: f32 = 1.0;

#[derive(Component)]
pub(crate) struct Quad;

#[derive(ShaderType, Debug, Clone, Default)]
pub(crate) struct RenderParams {
    pub(crate) bounds_min: Vec3,
    pub(crate) tan_half_fov: f32,
    pub(crate) bounds_max: Vec3,
    pub(crate) padding_one: f32,

    pub(crate) shape_count: u32,

    pub(crate) debug_view: u32,

    pub(crate) cull: u32,

    pub(crate) omega: f32,

    pub(crate) grid: u32,
    pub(crate) grid_padding: u32,
    pub(crate) grid_padding_two: u32,

    pub(crate) grid_origin: Vec3,
    pub(crate) grid_padding_three: f32,
    pub(crate) grid_cell: Vec3,
    pub(crate) grid_padding_four: f32,
    pub(crate) grid_resolution: UVec3,

    pub(crate) light_count: u32,

    pub(crate) shadow_steps: u32,
    pub(crate) detail: f32,
    pub(crate) shadow_padding_two: u32,
    pub(crate) shadow_padding_three: u32,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone, Default)]
pub(crate) struct SdfMaterial {
    #[uniform(0)]
    pub(crate) render_params: RenderParams,
    #[storage(1, read_only)]
    pub(crate) shapes: Handle<ShaderBuffer>,

    #[storage(2, read_only)]
    pub(crate) grid_cells: Handle<ShaderBuffer>,
    #[storage(3, read_only)]
    pub(crate) grid_indices: Handle<ShaderBuffer>,
    #[storage(4, read_only)]
    pub(crate) lights: Handle<ShaderBuffer>,
}

impl Material for SdfMaterial {
    fn vertex_shader() -> ShaderRef {
        SHADER_PATH.into()
    }
    fn fragment_shader() -> ShaderRef {
        SHADER_PATH.into()
    }
}

fn spawn_camera(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<SdfMaterial>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
) {
    let shapes = buffers.add(ShaderBuffer::from(vec![GpuShape::default(); MAX_SHAPES]));

    let grid_cells = buffers.add(ShaderBuffer::from(vec![0u32; GRID_CELL_WORDS]));
    let grid_indices = buffers.add(ShaderBuffer::from(vec![0u32; GRID_INDEX_WORDS]));
    let lights = buffers.add(ShaderBuffer::from(vec![GpuLight::default(); MAX_LIGHTS]));
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 2.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y),
        children![(
            Quad,
            Mesh3d(meshes.add(Plane3d::new(Vec3::Z, Vec2::splat(1.0)))),
            MeshMaterial3d(
                materials.add(SdfMaterial {
                    render_params: RenderParams {
                        cull: u32::from(!command_line::flag("--no-cull")),
                        omega: command_line::value("--omega").unwrap_or(OMEGA),
                        shadow_steps: command_line::value("--shadow-steps")
                            .map_or(SHADOW_STEPS, |steps| steps as u32),
                        detail: command_line::value("--detail").unwrap_or(DETAIL),

                        grid: 1,
                        ..default()
                    },
                    shapes: shapes.clone(),
                    grid_cells: grid_cells.clone(),
                    grid_indices: grid_indices.clone(),
                    lights: lights.clone(),
                })
            ),
            Transform::from_xyz(0.0, 0.0, -QUAD_DIST),
        )],
    ));
}

fn toggle_debug_view(
    actions: Res<ButtonInput<Action>>,
    quad: Single<&MeshMaterial3d<SdfMaterial>, With<Quad>>,
    mut materials: ResMut<Assets<SdfMaterial>>,
) {
    if !actions.just_pressed(Action::ToggleDebugView) {
        return;
    }
    if let Some(mut material) = materials.get_mut(&quad.0) {
        material.render_params.debug_view = 1 - material.render_params.debug_view;
    }
}

fn toggle_quad(actions: Res<ButtonInput<Action>>, visibility: Single<&mut Visibility, With<Quad>>) {
    if !actions.just_pressed(Action::ToggleQuad) {
        return;
    }
    let mut visibility = visibility.into_inner();
    *visibility = match *visibility {
        Visibility::Hidden => Visibility::Inherited,
        _ => Visibility::Hidden,
    };
}

fn fit_quad(
    proj: Single<&Projection, With<Camera3d>>,
    quad: Single<(&mut Mesh3d, &MeshMaterial3d<SdfMaterial>), With<Quad>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<SdfMaterial>>,
    mut last_aspect: Local<f32>,
) {
    let Projection::Perspective(perspective) = &*proj else {
        return;
    };
    if (perspective.aspect_ratio - *last_aspect).abs() < 1e-6 {
        return;
    }
    *last_aspect = perspective.aspect_ratio;

    let tan_half_fov = (perspective.fov * 0.5).tan();
    let half_height = tan_half_fov * QUAD_DIST * QUAD_OVERSCAN;
    let half_width = half_height * perspective.aspect_ratio;

    let (mut mesh, quad_material) = quad.into_inner();
    mesh.0 = meshes.add(Plane3d::new(Vec3::Z, Vec2::new(half_width, half_height)));
    if let Some(mut material) = materials.get_mut(&quad_material.0) {
        material.render_params.tan_half_fov = tan_half_fov;
    }
}
