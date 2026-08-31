//! Drawing the field: the material, the frustum-fitted quad, and the debug
//! toggles that make measurement possible.

use bevy::{
    prelude::*,
    reflect::TypePath,
    render::{render_resource::{AsBindGroup, ShaderType}, storage::ShaderBuffer},
    shader::ShaderRef,
};

use crate::input::Action;
use crate::field::{GRID_CELL_WORDS, GRID_INDEX_WORDS, GpuShape, MAX_SHAPES};

pub(crate) struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<SdfMaterial>::default())
            .add_systems(Startup, spawn_camera)
            .add_systems(Update, (fit_quad, toggle_debug_view, toggle_quad));
    }
}

const SHADER_PATH: &str = "shaders/sdf.wgsl";
const QUAD_DIST: f32 = 1.0; // quad sits this far in front of the camera
const QUAD_OVERSCAN: f32 = 1.01; // hair of slack so no gap at screen edge
/// Over-relaxation factor. 1.0 is plain sphere tracing. Keinert et al. report
/// 1.2 as a good default; `bench --omega <n>` is how it gets checked here.
pub(crate) const OMEGA: f32 = 1.2;

/// The screen-filling quad. Rebuilt whenever the aspect ratio changes.
#[derive(Component)]
pub(crate) struct Quad;

/// Field order is chosen so the vec3s land on 16-byte boundaries. Must match
/// `struct RenderParams` in sdf.wgsl exactly.
#[derive(ShaderType, Debug, Clone, Default)]
pub(crate) struct RenderParams {
    /// Corners of the box holding every shape. A ray that misses it hits
    /// nothing, and a ray that leaves it can stop.
    pub(crate) bounds_min: Vec3,
    pub(crate) tan_half_fov: f32,
    pub(crate) bounds_max: Vec3,
    pub(crate) padding_one: f32,
    /// How much of the fixed-size buffer actually holds shapes.
    pub(crate) shape_count: u32,
    /// 0 = shaded, 1 = march-step heatmap.
    pub(crate) debug_view: u32,
    /// 0 turns the per-shape box reject off, so a bench run can measure what it
    /// is worth. Default on - `Default` gives 0, so every construction site has
    /// to say so explicitly.
    pub(crate) cull: u32,
    /// Over-relaxation factor for the march. 1.0 is plain sphere tracing;
    /// the paper's default is 1.2. Clamped to at least 1.0 in the shader.
    pub(crate) omega: f32,
    /// 0 turns the acceleration grid off, for measuring it.
    pub(crate) grid: u32,
    pub(crate) grid_padding: u32,
    pub(crate) grid_padding_two: u32,
    /// Corner of the grid, the size of one (cubic) cell, and how many cells
    /// there are along each axis.
    pub(crate) grid_origin: Vec3,
    pub(crate) grid_padding_three: f32,
    pub(crate) grid_cell: Vec3,
    pub(crate) grid_padding_four: f32,
    pub(crate) grid_resolution: UVec3,
    pub(crate) grid_padding_five: u32,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone, Default)]
pub(crate) struct SdfMaterial {
    #[uniform(0)]
    pub(crate) render_params: RenderParams,
    #[storage(1, read_only)]
    pub(crate) shapes: Handle<ShaderBuffer>,
    /// Two `u32` per cell: offset into `grid_indices`, then count.
    #[storage(2, read_only)]
    pub(crate) grid_cells: Handle<ShaderBuffer>,
    #[storage(3, read_only)]
    pub(crate) grid_indices: Handle<ShaderBuffer>,
}

impl Material for SdfMaterial {
    fn vertex_shader() -> ShaderRef {
        SHADER_PATH.into()
    }
    fn fragment_shader() -> ShaderRef {
        SHADER_PATH.into()
    }
}

/// The camera, and the quad glued one unit in front of it that the whole field
/// is drawn on.
fn spawn_camera(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<SdfMaterial>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
) {
    let shapes = buffers.add(ShaderBuffer::from(vec![GpuShape::default(); MAX_SHAPES]));
    // Same fixed-size rule as the shape buffer: a resize rebuilds the GPU
    // buffer while the bind group still points at the old one.
    let grid_cells = buffers.add(ShaderBuffer::from(vec![0u32; GRID_CELL_WORDS]));
    let grid_indices = buffers.add(ShaderBuffer::from(vec![0u32; GRID_INDEX_WORDS]));
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 2.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y),
        children![(
            Quad,
            Mesh3d(meshes.add(Plane3d::new(Vec3::Z, Vec2::splat(1.0)))), // resized by fit_quad
            MeshMaterial3d(materials.add(SdfMaterial {
                render_params: RenderParams {
                    cull: 1,
                    omega: OMEGA,
                    grid: 1,
                    ..default()
                },
                shapes: shapes.clone(),
                grid_cells: grid_cells.clone(),
                grid_indices: grid_indices.clone(),
            })),
            Transform::from_xyz(0.0, 0.0, -QUAD_DIST),
        )],
    ));
}

/// `Action::ToggleDebugView`: swap between the shaded image and a heatmap of marching steps, so the
/// pixels that actually cost something are visible instead of guessed at.
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

/// `Action::ToggleQuad`: hide the quad entirely. What is left is Bevy's own per-frame cost -
/// the floor every render measurement sits on top of.
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

/// Rebuild the quad so it exactly covers the frustum, with square cells.
/// Only runs when the aspect ratio actually changed.
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

    // Two triangles. The mesh was subdivided into a grid of cells when a coarse
    // cone-march ran per vertex; nothing reads vertex position now except the
    // ray direction, which interpolates exactly over one quad.
    //
    // Assigning a new handle drops the old mesh, and Bevy frees it. No leak.
    let (mut mesh, quad_material) = quad.into_inner();
    mesh.0 = meshes.add(Plane3d::new(Vec3::Z, Vec2::new(half_width, half_height)));
    if let Some(mut material) = materials.get_mut(&quad_material.0) {
        material.render_params.tan_half_fov = tan_half_fov;
    }
}
