use bevy::{
    prelude::*,
    render::{
        render_resource::{ShaderType, encase::internal::WriteInto},
        storage::ShaderBuffer,
    },
};

use crate::game::world::SdfWorld;
use crate::sdf::blending::blend;
use crate::sdf::bounds::{scene_bounds, shape_cannot_reach};
use crate::sdf::brush::{
    Albedo, Brush, CsgOperation, GpuShape, MAX_SHAPES, Modifiers, SphereBody, pack_brush,
};
use crate::sdf::distance::{MAX_MARCH_DISTANCE, shape_distance};
use crate::sdf::grid::{GRID_CELL_WORDS, GRID_INDEX_WORDS, GridSettings, SdfGrid, build_grid};
use crate::sdf::render::{Quad, RenderParams, SdfMaterial};

pub(crate) struct FieldPlugin;

impl Plugin for FieldPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SdfScene>()
            .init_resource::<GridSettings>()
            .add_systems(Update, sync_shapes_to_gpu);
    }
}

const SURFACE_EPSILON: f32 = 0.0005;

#[derive(Resource, Default)]
pub(crate) struct SdfScene {
    pub(crate) shapes: Vec<GpuShape>,
    pub(crate) static_count: usize,

    pub(crate) grid: SdfGrid,
}

impl SdfScene {
    pub(crate) fn static_shapes(&self) -> &[GpuShape] {
        &self.shapes[..self.static_count]
    }
}

type BrushQuery = (
    &'static Brush,
    &'static GlobalTransform,
    Option<&'static Modifiers>,
    Option<&'static CsgOperation>,
    Option<&'static Albedo>,
);

fn pack_queried_brush(
    (_, placement, modifiers, operation, albedo): (
        &Brush,
        &GlobalTransform,
        Option<&Modifiers>,
        Option<&CsgOperation>,
        Option<&Albedo>,
    ),
) -> GpuShape {
    pack_brush(placement, modifiers, operation, albedo)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn sync_shapes_to_gpu(
    world: Single<&Children, With<SdfWorld>>,
    statics: Query<BrushQuery>,
    bodies: Query<BrushQuery, With<SphereBody>>,
    quad: Single<&MeshMaterial3d<SdfMaterial>, With<Quad>>,
    mut materials: ResMut<Assets<SdfMaterial>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut scene: ResMut<SdfScene>,
    settings: Res<GridSettings>,
) {
    let (packed, static_count) = collect_brushes(&world, &statics, &bodies);

    if packed == scene.shapes && !settings.is_changed() {
        return;
    }
    scene.shapes = packed;
    scene.static_count = static_count;

    let Some(mut material) = materials.get_mut(&quad.0) else {
        return;
    };
    let (bounds_min, bounds_max) = scene_bounds(&scene.shapes);
    let grid = build_grid(&scene.shapes, bounds_min, bounds_max, settings.resolution);
    describe_scene_to_shader(
        &mut material.render_params,
        &scene.shapes,
        (bounds_min, bounds_max),
        &grid,
        &settings,
    );

    let (shape_buffer, cell_buffer, index_buffer) = (
        material.shapes.clone(),
        material.grid_cells.clone(),
        material.grid_indices.clone(),
    );
    upload_padded(&mut buffers, &shape_buffer, &scene.shapes, MAX_SHAPES);
    upload_padded(&mut buffers, &cell_buffer, &grid.cells, GRID_CELL_WORDS);
    upload_padded(&mut buffers, &index_buffer, &grid.indices, GRID_INDEX_WORDS);
    scene.grid = grid;
}

fn collect_brushes(
    world: &Children,
    statics: &Query<BrushQuery>,
    bodies: &Query<BrushQuery, With<SphereBody>>,
) -> (Vec<GpuShape>, usize) {
    let mut packed: Vec<GpuShape> = world
        .iter()
        .filter_map(|brush| statics.get(brush).ok())
        .map(pack_queried_brush)
        .collect();
    let mut static_count = packed.len();
    packed.extend(bodies.iter().map(pack_queried_brush));

    if packed.len() > MAX_SHAPES {
        warn!(
            "scene has {} brushes, buffer holds {MAX_SHAPES}; the rest are dropped",
            packed.len()
        );
        packed.truncate(MAX_SHAPES);
        static_count = static_count.min(MAX_SHAPES);
    }
    (packed, static_count)
}

fn describe_scene_to_shader(
    params: &mut RenderParams,
    shapes: &[GpuShape],
    bounds: (Vec3, Vec3),
    grid: &SdfGrid,
    settings: &GridSettings,
) {
    (params.bounds_min, params.bounds_max) = bounds;
    params.shape_count = shapes.len() as u32;
    params.grid = u32::from(settings.enabled);
    params.grid_resolution = grid.resolution;
    params.grid_origin = grid.origin;
    params.grid_cell = grid.cell_size;
}

fn upload_padded<T>(
    buffers: &mut Assets<ShaderBuffer>,
    handle: &Handle<ShaderBuffer>,
    values: &[T],
    capacity: usize,
) where
    T: Clone + Default,
    Vec<T>: ShaderType + WriteInto,
{
    let Some(mut buffer) = buffers.get_mut(handle) else {
        return;
    };
    let mut padded = values.to_vec();
    padded.resize_with(capacity, T::default);
    buffer.set_data(padded);
}

const TETRAHEDRON_CORNERS: [Vec3; 4] = [
    Vec3::new(1.0, -1.0, -1.0),
    Vec3::new(-1.0, -1.0, 1.0),
    Vec3::new(-1.0, 1.0, -1.0),
    Vec3::new(1.0, 1.0, 1.0),
];

pub(crate) fn scene_normal(shapes: &[GpuShape], world_point: Vec3) -> Vec3 {
    TETRAHEDRON_CORNERS
        .iter()
        .map(|corner| *corner * scene_distance(shapes, world_point + *corner * SURFACE_EPSILON))
        .sum::<Vec3>()
        .normalize_or_zero()
}

pub(crate) fn scene_distance(shapes: &[GpuShape], world_point: Vec3) -> f32 {
    let mut field = MAX_MARCH_DISTANCE;
    for (index, shape) in shapes.iter().enumerate() {
        if index > 0 && shape_cannot_reach(shape, world_point, field) {
            continue;
        }
        let distance = shape_distance(shape, world_point);
        field = if index == 0 {
            distance
        } else {
            blend(distance, field, &shape.blend, shape.blend.chamfer != 0)
        };
    }
    field
}
