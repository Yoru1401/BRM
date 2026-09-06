use bevy::{
    prelude::*,
    render::{
        render_resource::{ShaderType, encase::internal::WriteInto},
        storage::ShaderBuffer,
    },
};

use crate::game::scenes::SdfWorld;
use crate::sdf::blending::blend;
use crate::sdf::bounds::{scene_bounds, shape_cannot_reach};
use crate::sdf::brush::{
    Albedo, Brush, CsgOperation, GpuShape, MAX_SHAPES, Modifiers, SphereBody, pack_brush,
};
use crate::sdf::distance::{MAX_MARCH_DISTANCE, shape_distance};
use crate::sdf::grid::{
    GRID_CELL_WORDS, GRID_INDEX_WORDS, GridSettings, GridWindow, SdfGrid, build_grid,
};
use crate::sdf::render::{MainCamera, Quad, RenderParams, SdfMaterial};

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
    pub(crate) window: GridWindow,

    pub(crate) grid: SdfGrid,
}

impl SdfScene {
    pub(crate) fn static_shapes(&self) -> &[GpuShape] {
        &self.shapes[..self.static_count]
    }
}

pub(crate) type StaticBrushQuery = (
    &'static Brush,
    Ref<'static, GlobalTransform>,
    Option<Ref<'static, Modifiers>>,
    Option<Ref<'static, CsgOperation>>,
    Option<Ref<'static, Albedo>>,
);

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
    statics: Query<StaticBrushQuery>,
    bodies: Query<BrushQuery, With<SphereBody>>,
    eye: Single<&GlobalTransform, With<MainCamera>>,
    quad: Single<&MeshMaterial3d<SdfMaterial>, With<Quad>>,
    mut materials: ResMut<Assets<SdfMaterial>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut scene: ResMut<SdfScene>,
    settings: Res<GridSettings>,
) {
    let following = settings
        .cell_size
        .map(|cell| GridWindow::around(eye.translation(), cell, settings.resolution));

    let statics_moved =
        settings.is_changed() || static_brushes_changed(&world, &statics, scene.static_count);
    let window_moved = following.is_some_and(|window| window != scene.window);
    if !statics_moved && !window_moved && packed_bodies_match(&bodies, &scene) {
        return;
    }
    let packed_bodies = collect_bodies(&bodies);

    let Some(mut material) = materials.get_mut(&quad.0) else {
        return;
    };

    if statics_moved {
        let packed = collect_statics(&world, &statics);
        scene.static_count = packed.len();
        scene.shapes = packed;
    } else {
        let unchanged = scene.static_count;
        scene.shapes.truncate(unchanged);
    }
    scene.shapes.extend(packed_bodies);
    drop_overflowing_brushes(&mut scene);

    let bounds = scene_bounds(&scene.shapes);
    scene.window =
        following.unwrap_or_else(|| GridWindow::covering(bounds.0, bounds.1, settings.resolution));
    scene.grid = build_grid(&scene.shapes, scene.window);
    describe_scene_to_shader(
        &mut material.render_params,
        &scene.shapes,
        bounds,
        &scene.grid,
        &settings,
    );

    let (shapes, cells, indices) = (
        material.shapes.clone(),
        material.grid_cells.clone(),
        material.grid_indices.clone(),
    );
    upload_padded(&mut buffers, &shapes, &scene.shapes, MAX_SHAPES);
    upload_padded(&mut buffers, &cells, &scene.grid.cells, GRID_CELL_WORDS);
    upload_padded(
        &mut buffers,
        &indices,
        &scene.grid.indices,
        GRID_INDEX_WORDS,
    );
}

pub(crate) fn static_brushes_changed(
    world: &Children,
    statics: &Query<StaticBrushQuery>,
    counted_before: usize,
) -> bool {
    let mut count = 0;
    let mut moved = false;
    for brush in world.iter() {
        let Ok((_, placement, modifiers, operation, albedo)) = statics.get(brush) else {
            continue;
        };
        count += 1;
        moved |= placement.is_changed()
            || modifiers.is_some_and(|value| value.is_changed())
            || operation.is_some_and(|value| value.is_changed())
            || albedo.is_some_and(|value| value.is_changed());
    }
    moved || count != counted_before
}

fn collect_statics(world: &Children, statics: &Query<StaticBrushQuery>) -> Vec<GpuShape> {
    world
        .iter()
        .filter_map(|brush| statics.get(brush).ok())
        .map(|(_, placement, modifiers, operation, albedo)| {
            pack_brush(
                &placement,
                modifiers.as_deref(),
                operation.as_deref(),
                albedo.as_deref(),
            )
        })
        .collect()
}

fn packed_bodies_match(bodies: &Query<BrushQuery, With<SphereBody>>, scene: &SdfScene) -> bool {
    collect_bodies(bodies) == scene.shapes[scene.static_count..]
}

fn collect_bodies(bodies: &Query<BrushQuery, With<SphereBody>>) -> Vec<GpuShape> {
    bodies.iter().map(pack_queried_brush).collect()
}

fn drop_overflowing_brushes(scene: &mut SdfScene) {
    if scene.shapes.len() <= MAX_SHAPES {
        return;
    }
    warn!(
        "scene has {} brushes, buffer holds {MAX_SHAPES}; the rest are dropped",
        scene.shapes.len()
    );
    scene.shapes.truncate(MAX_SHAPES);
    scene.static_count = scene.static_count.min(MAX_SHAPES);
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
    params.grid_indexed = grid.indexed as u32;
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
