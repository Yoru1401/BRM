use bevy::{
    prelude::*,
    render::{
        render_resource::{ShaderType, encase::internal::WriteInto},
        storage::ShaderBuffer,
    },
};

use crate::command_line;
use crate::game::world::SdfWorld;
use crate::sdf::render::{Quad, RenderParams, SdfMaterial};

pub(crate) struct FieldPlugin;

impl Plugin for FieldPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SdfScene>()
            .init_resource::<GridSettings>()
            .add_systems(Update, sync_shapes_to_gpu);
    }
}

pub(crate) const MAX_MARCH_DISTANCE: f32 = 100.0;

const SURFACE_EPSILON: f32 = 0.0005;

const MIN_RADIUS: f32 = 1e-5;

pub(crate) const MAX_SHAPES: usize = 256;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Brush;

#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub(crate) struct Modifiers {
    pub(crate) round: f32,

    pub(crate) bevel: f32,

    pub(crate) thickness: f32,

    pub(crate) cone: f32,
}

impl Default for Modifiers {
    fn default() -> Self {
        Modifiers {
            round: 0.0,
            bevel: 0.0,
            thickness: 1.0,
            cone: 0.0,
        }
    }
}

#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub(crate) struct CsgOperation {
    pub(crate) mode: u32,
    pub(crate) chamfer: bool,
    pub(crate) radius: f32,
    pub(crate) strength: f32,
}

impl Default for CsgOperation {
    fn default() -> Self {
        CsgOperation {
            mode: GPU_MODE_ADD,
            chamfer: false,
            radius: 0.0,
            strength: 0.0,
        }
    }
}

#[derive(ShaderType, Debug, Clone, PartialEq, Default)]
pub(crate) struct GpuBlend {
    pub(crate) mode: u32,

    pub(crate) radius: f32,

    pub(crate) strength: f32,

    pub(crate) chamfer: u32,
}

#[derive(ShaderType, Debug, Clone, PartialEq, Default)]
pub(crate) struct GpuShape {
    pub(crate) center: Vec3,

    pub(crate) wall_thickness: f32,

    pub(crate) half_size: Vec3,

    pub(crate) side_radius: f32,

    pub(crate) inverse_rotation: Vec4,

    pub(crate) albedo: Vec3,

    pub(crate) cap_radius: f32,

    pub(crate) cull_extent: Vec3,

    pub(crate) cull_scale: f32,

    pub(crate) taper: f32,
    pub(crate) padding_one: f32,
    pub(crate) padding_two: f32,
    pub(crate) padding_three: f32,
    pub(crate) blend: GpuBlend,
}

pub(crate) const GPU_MODE_ADD: u32 = 0;
pub(crate) const GPU_MODE_SUBTRACT: u32 = 1;
pub(crate) const GPU_MODE_INTERSECT: u32 = 2;
pub(crate) const GPU_MODE_PAINT: u32 = 3;
pub(crate) const GPU_MODE_PUSH: u32 = 4;
pub(crate) const GPU_MODE_AVOID: u32 = 5;
pub(crate) const GPU_MODE_EMBOSS: u32 = 6;
pub(crate) const GPU_MODE_DEBOSS: u32 = 7;
pub(crate) const GPU_MODE_SHELL: u32 = 8;

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

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct SphereBody {
    pub(crate) radius: f32,
    pub(crate) velocity: Vec3,
    pub(crate) angular_velocity: Vec3,

    pub(crate) orientation: Quat,

    pub(crate) resting: bool,
}

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct Albedo(pub(crate) Vec3);

impl Default for Albedo {
    fn default() -> Self {
        Albedo(DEFAULT_ALBEDO)
    }
}

const DEFAULT_ALBEDO: Vec3 = Vec3::new(1.0, 0.4, 0.2);

pub(crate) fn pack_brush(
    placement: &GlobalTransform,
    modifiers: Option<&Modifiers>,
    operation: Option<&CsgOperation>,
    albedo: Option<&Albedo>,
) -> GpuShape {
    let (scale, rotation, translation) = placement.to_scale_rotation_translation();
    let half_size = scale.abs().max(Vec3::splat(f32::EPSILON));
    let modifiers = modifiers.copied().unwrap_or_default();
    let operation = operation.copied().unwrap_or_default();
    let (side_radius, cap_radius) = corner_radii(&modifiers, half_size);

    let mut packed = GpuShape {
        center: translation,
        wall_thickness: wall_thickness(modifiers.thickness, footprint_of(half_size)),
        half_size,
        side_radius,
        inverse_rotation: quaternion_words(rotation.inverse()),
        albedo: albedo.map_or(DEFAULT_ALBEDO, |albedo| albedo.0),
        cap_radius,
        cull_extent: Vec3::ZERO,
        cull_scale: 1.0,
        taper: modifiers.cone,
        padding_one: 0.0,
        padding_two: 0.0,
        padding_three: 0.0,
        blend: GpuBlend {
            mode: operation.mode,
            radius: operation.radius,
            strength: operation.strength,
            chamfer: u32::from(operation.chamfer),
        },
    };
    (packed.cull_extent, packed.cull_scale) = cull_bound(&packed);
    packed
}

fn footprint_of(half_size: Vec3) -> f32 {
    half_size.x.min(half_size.z)
}

fn corner_radii(modifiers: &Modifiers, half_size: Vec3) -> (f32, f32) {
    let footprint = footprint_of(half_size);
    let rounded = modifiers.round * half_size.min_element();
    let bevelled = modifiers.bevel * footprint;
    (
        rounded.max(bevelled).min(footprint),
        rounded.min(half_size.y),
    )
}

fn quaternion_words(rotation: Quat) -> Vec4 {
    Vec4::new(rotation.x, rotation.y, rotation.z, rotation.w)
}

fn wall_thickness(thickness: f32, footprint: f32) -> f32 {
    if thickness >= 1.0 {
        return footprint;
    }
    thickness * footprint * 0.5
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

pub(crate) fn shape_distance(shape: &GpuShape, world_point: Vec3) -> f32 {
    let local_point = Quat::from_vec4(shape.inverse_rotation) * (world_point - shape.center);
    tapered_box_distance(local_point, shape)
}

fn tapered_box_distance(local_point: Vec3, shape: &GpuShape) -> f32 {
    let footprint = footprint_of(shape.half_size);
    if shape.taper <= 0.0 {
        return rounded_box_distance(
            local_point,
            shape.half_size,
            bore(shape.wall_thickness, 0.0, footprint),
            shape.side_radius,
            shape.cap_radius,
        );
    }

    let taper = shape.taper * footprint;
    let height_fraction = ((local_point.y / shape.half_size.y + 1.0) * 0.5).clamp(0.0, 1.0);
    let inset = taper * height_fraction;
    let remaining = (footprint - inset).max(0.0);

    let narrowed = Vec3::new(
        shape.half_size.x - inset,
        shape.half_size.y,
        shape.half_size.z - inset,
    );
    let distance = rounded_box_distance(
        local_point,
        narrowed,
        bore(shape.wall_thickness, taper - inset, remaining),
        (shape.side_radius - inset).max(0.0),
        shape.cap_radius,
    );

    let slope = taper / (2.0 * shape.half_size.y);
    distance / (1.0 + slope * slope).sqrt()
}

fn rounded_box_distance(
    local_point: Vec3,
    half_size: Vec3,
    wall: f32,
    side_radius: f32,
    cap_radius: f32,
) -> f32 {
    let inner = Vec3::new(
        half_size.x - side_radius,
        half_size.y - cap_radius,
        half_size.z - side_radius,
    );

    let side_radius = side_radius - wall;
    let wall = wall - cap_radius;

    let corner = local_point.abs() - inner;
    let flat = Vec2::new(corner.x, corner.z);
    let cross_section = flat.max(Vec2::ZERO).length() + flat.max_element().min(0.0) - side_radius;

    let profile = Vec2::new(cross_section.abs() - wall, corner.y);
    profile.max_element().min(0.0) + profile.max(Vec2::ZERO).length() - cap_radius
}

pub(crate) fn bore(wall: f32, unspent_taper: f32, remaining: f32) -> f32 {
    (wall + unspent_taper).min(remaining)
}

#[cfg(test)]
pub(crate) fn legacy_combined_primitive(local_point: Vec3, s: Vec4, r: Vec3) -> f32 {
    let mut s = s;
    let mut r = r;
    s.x -= r.x;
    s.z -= r.x;
    r.x -= s.w;
    s.w -= r.y;
    s.y -= r.y;

    let bevel_axis = Vec2::new(r.z, -2.0 * s.y);
    let squared = bevel_axis.dot(bevel_axis);
    let along = if squared > 0.0 {
        bevel_axis / squared
    } else {
        Vec2::ZERO
    };

    let corner = local_point.abs() - s.truncate();
    let flat = Vec2::new(corner.x, corner.z);
    let mut radial = flat.max(Vec2::ZERO).length() + flat.max_element().min(0.0) - r.x;
    radial = radial.abs() - s.w;

    let profile = Vec2::new(radial, local_point.y - s.y);
    let diagonal = profile - Vec2::new(r.z, bevel_axis.y) * profile.dot(along).clamp(0.0, 1.0);
    let bottom = Vec2::new((radial - r.z).max(0.0), local_point.y + s.y);
    let top = Vec2::new(radial.max(0.0), local_point.y - s.y);

    let nearest = diagonal
        .length_squared()
        .min(bottom.length_squared())
        .min(top.length_squared());
    let outside = profile.dot(Vec2::new(-along.y, along.x)).max(corner.y);
    nearest.sqrt() * outside.signum() - r.y
}

#[cfg(test)]
pub(crate) fn rounded_box_in_legacy_terms(local_point: Vec3, s: Vec4, r: Vec3) -> f32 {
    rounded_box_distance(local_point, s.truncate(), s.w, r.x, r.y)
}

fn union_smooth(shape: f32, field: f32, radius: f32) -> f32 {
    let mix = (0.5 + 0.5 * (field - shape) / radius).clamp(0.0, 1.0);
    field.lerp(shape, mix) - radius * mix * (1.0 - mix)
}

fn subtract_smooth(shape: f32, field: f32, radius: f32) -> f32 {
    let mix = (0.5 - 0.5 * (field + shape) / radius).clamp(0.0, 1.0);
    field.lerp(-shape, mix) + radius * mix * (1.0 - mix)
}

fn intersect_smooth(shape: f32, field: f32, radius: f32) -> f32 {
    let mix = (0.5 - 0.5 * (field - shape) / radius).clamp(0.0, 1.0);
    field.lerp(shape, mix) + radius * mix * (1.0 - mix)
}

fn op_union(shape: f32, field: f32, radius: f32, chamfer: bool) -> f32 {
    if chamfer {
        return shape.min(field).min((shape - 0.5 * radius + field) * 0.5);
    }
    if radius > 0.0 {
        return union_smooth(shape, field, radius);
    }
    shape.min(field)
}

fn op_intersect(shape: f32, field: f32, radius: f32, chamfer: bool) -> f32 {
    if chamfer {
        return shape.max(field).max((field + 0.5 * radius + shape) * 0.5);
    }
    if radius > 0.0 {
        return intersect_smooth(shape, field, radius);
    }
    shape.max(field)
}

fn op_subtract(shape: f32, field: f32, radius: f32, chamfer: bool) -> f32 {
    if chamfer {
        return op_intersect(-shape, field, radius, true);
    }
    if radius > 0.0 {
        return subtract_smooth(shape, field, radius);
    }
    field.max(-shape)
}

pub(crate) fn blend(shape: f32, field: f32, blend: &GpuBlend, chamfer: bool) -> f32 {
    let (radius, strength) = (blend.radius, blend.strength);
    match blend.mode {
        GPU_MODE_SUBTRACT => op_subtract(shape, field, radius, chamfer),
        GPU_MODE_INTERSECT => op_intersect(shape, field, radius, chamfer),

        GPU_MODE_PAINT => field,
        GPU_MODE_PUSH => op_subtract(shape - strength, field, radius, chamfer).min(shape),
        GPU_MODE_AVOID => op_subtract(field - strength, shape, radius, chamfer).min(field),
        GPU_MODE_EMBOSS => op_union(
            field,
            op_intersect(shape, field - strength, radius, chamfer),
            radius,
            chamfer,
        ),
        GPU_MODE_DEBOSS => op_subtract(
            op_subtract(field + strength, shape, radius, chamfer),
            field,
            radius,
            chamfer,
        ),
        GPU_MODE_SHELL => op_intersect(shape, (field + strength).abs() - strength, radius, chamfer),
        _ => op_union(shape, field, radius, chamfer),
    }
}

fn shape_half_extent(shape: &GpuShape) -> Vec3 {
    world_aligned_extent(shape.half_size, shape.inverse_rotation) + Vec3::splat(shape.blend.radius)
}

fn world_aligned_extent(local_extent: Vec3, inverse_rotation: Vec4) -> Vec3 {
    let rotation = Mat3::from_quat(Quat::from_vec4(inverse_rotation).inverse());
    let unsigned = Mat3::from_cols(
        rotation.x_axis.abs(),
        rotation.y_axis.abs(),
        rotation.z_axis.abs(),
    );
    unsigned * local_extent
}

const BOUNDS_SLACK: f32 = 0.05;

pub(crate) fn scene_bounds(shapes: &[GpuShape]) -> (Vec3, Vec3) {
    let mut minimum = Vec3::splat(f32::MAX);
    let mut maximum = Vec3::splat(f32::MIN);
    for shape in shapes {
        if matches!(
            shape.blend.mode,
            GPU_MODE_SUBTRACT | GPU_MODE_INTERSECT | GPU_MODE_PAINT
        ) {
            continue;
        }
        let half_extent = shape_half_extent(shape);
        minimum = minimum.min(shape.center - half_extent);
        maximum = maximum.max(shape.center + half_extent);
    }
    if minimum.cmpgt(maximum).any() {
        return (Vec3::ZERO, Vec3::ZERO);
    }
    (
        minimum - Vec3::splat(BOUNDS_SLACK),
        maximum + Vec3::splat(BOUNDS_SLACK),
    )
}

pub(crate) const GRID_DEFAULT_RESOLUTION: u32 = 16;
pub(crate) const GRID_MAX_RESOLUTION: u32 = 32;
const GRID_MAX_CELLS: usize =
    (GRID_MAX_RESOLUTION * GRID_MAX_RESOLUTION * GRID_MAX_RESOLUTION) as usize;

pub(crate) const GRID_CELL_WORDS: usize = GRID_MAX_CELLS * 2;
pub(crate) const GRID_INDEX_WORDS: usize = GRID_MAX_ENTRIES;

const GRID_MAX_ENTRIES: usize = 1 << 18;

pub(crate) const GRID_CELL_FULL: u32 = u32::MAX;

#[derive(Resource, Debug, Clone, Copy)]
pub(crate) struct GridSettings {
    pub(crate) resolution: u32,
    pub(crate) enabled: bool,
}

impl Default for GridSettings {
    fn default() -> Self {
        GridSettings {
            resolution: command_line::value("--grid")
                .map_or(GRID_DEFAULT_RESOLUTION, |cells| cells as u32),
            enabled: !command_line::flag("--no-grid"),
        }
    }
}

#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub(crate) struct SdfGrid {
    pub(crate) origin: Vec3,

    pub(crate) cell_size: Vec3,

    pub(crate) resolution: UVec3,

    pub(crate) cells: Vec<u32>,
    pub(crate) indices: Vec<u32>,
}

#[allow(dead_code)]
impl SdfGrid {
    fn cell_count(&self) -> usize {
        (self.resolution.x * self.resolution.y * self.resolution.z) as usize
    }

    pub(crate) fn cell_of(&self, point: Vec3) -> usize {
        let slot = self.slot_of(point);
        (slot.x as usize)
            + (slot.y as usize) * self.resolution.x as usize
            + (slot.z as usize) * (self.resolution.x * self.resolution.y) as usize
    }

    fn slot_of(&self, point: Vec3) -> Vec3 {
        let last = (self.resolution - UVec3::ONE).as_vec3();
        ((point - self.origin) / self.cell_size)
            .floor()
            .clamp(Vec3::ZERO, last)
    }

    pub(crate) fn holds(&self, point: Vec3) -> bool {
        let high = self.origin + self.cell_size * self.resolution.as_vec3();
        point.cmpge(self.origin).all() && point.cmple(high).all()
    }

    pub(crate) fn overlap(cell_size: Vec3) -> Vec3 {
        cell_size * 0.5
    }

    pub(crate) fn exit_distance(&self, point: Vec3) -> f32 {
        let slot = self.slot_of(point);
        let overlap = Self::overlap(self.cell_size);
        let low = self.origin + slot * self.cell_size - overlap;
        let high = self.origin + (slot + Vec3::ONE) * self.cell_size + overlap;
        let to_wall = (point - low).min(high - point);
        to_wall.min_element().max(0.0)
    }
}

pub(crate) fn build_grid(
    shapes: &[GpuShape],
    bounds_min: Vec3,
    bounds_max: Vec3,
    resolution: u32,
) -> SdfGrid {
    let requested = resolution.clamp(1, GRID_MAX_RESOLUTION);
    let span = (bounds_max - bounds_min).max(Vec3::splat(MIN_RADIUS));

    let side = span.max_element() / requested as f32;
    let cell_size = Vec3::splat(side);
    let resolution = (span / side)
        .ceil()
        .as_uvec3()
        .max(UVec3::ONE)
        .min(UVec3::splat(GRID_MAX_RESOLUTION));
    let cells_total = (resolution.x * resolution.y * resolution.z) as usize;

    let mut grid = SdfGrid {
        origin: bounds_min,
        cell_size,
        resolution,
        cells: vec![0; cells_total * 2],
        indices: Vec::new(),
    };
    if shapes.is_empty() {
        return grid;
    }

    let margin = SdfGrid::overlap(cell_size);

    let range_of = |index: usize, shape: &GpuShape| -> (Vec3, Vec3) {
        if index == 0 || shape.blend.mode != GPU_MODE_ADD {
            return (bounds_min, bounds_max);
        }
        (
            shape.center - shape.cull_extent - margin,
            shape.center + shape.cull_extent + margin,
        )
    };
    let last = (resolution - UVec3::ONE).as_vec3();
    let slots = |corner: Vec3| -> [usize; 3] {
        let slot = ((corner - bounds_min) / cell_size)
            .floor()
            .clamp(Vec3::ZERO, last);
        [slot.x as usize, slot.y as usize, slot.z as usize]
    };

    let mut counts = vec![0u32; cells_total];
    let mut total = 0usize;
    for (index, shape) in shapes.iter().enumerate() {
        let (low, high) = range_of(index, shape);
        let (from, to) = (slots(low), slots(high));
        for z in from[2]..=to[2] {
            for y in from[1]..=to[1] {
                for x in from[0]..=to[0] {
                    let cell =
                        x + y * resolution.x as usize + z * (resolution.x * resolution.y) as usize;
                    counts[cell] += 1;
                    total += 1;
                }
            }
        }
    }

    if total > GRID_MAX_ENTRIES {
        for cell in 0..cells_total {
            grid.cells[cell * 2 + 1] = GRID_CELL_FULL;
        }
        return grid;
    }

    let mut offset = 0u32;
    for (cell, count) in counts.iter().enumerate() {
        grid.cells[cell * 2] = offset;
        offset += count;
    }
    grid.indices = vec![0; total];

    let mut written = vec![0u32; cells_total];
    for (index, shape) in shapes.iter().enumerate() {
        let (low, high) = range_of(index, shape);
        let (from, to) = (slots(low), slots(high));
        for z in from[2]..=to[2] {
            for y in from[1]..=to[1] {
                for x in from[0]..=to[0] {
                    let cell =
                        x + y * resolution.x as usize + z * (resolution.x * resolution.y) as usize;
                    let at = (grid.cells[cell * 2] + written[cell]) as usize;
                    grid.indices[at] = index as u32;
                    written[cell] += 1;
                }
            }
        }
    }
    for (cell, count) in written.iter().enumerate() {
        grid.cells[cell * 2 + 1] = *count;
    }
    grid
}

#[allow(dead_code)]
pub(crate) fn scene_distance_gridded(
    shapes: &[GpuShape],
    grid: &SdfGrid,
    world_point: Vec3,
) -> f32 {
    if grid.cells.len() < grid.cell_count() * 2 || shapes.is_empty() || !grid.holds(world_point) {
        return scene_distance(shapes, world_point);
    }
    let cell = grid.cell_of(world_point);
    let count = grid.cells[cell * 2 + 1];
    if count == GRID_CELL_FULL {
        return scene_distance(shapes, world_point);
    }

    let offset = grid.cells[cell * 2] as usize;
    let mut field = MAX_MARCH_DISTANCE;
    let mut evaluated = 0;
    for slot in 0..count as usize {
        let shape = &shapes[grid.indices[offset + slot] as usize];
        if evaluated > 0 && shape_cannot_reach(shape, world_point, field) {
            continue;
        }
        let distance = shape_distance(shape, world_point);
        field = if evaluated == 0 {
            distance
        } else {
            blend(distance, field, &shape.blend, shape.blend.chamfer != 0)
        };
        evaluated += 1;
    }

    if count as usize == shapes.len() {
        return field;
    }
    field.min(grid.exit_distance(world_point))
}

#[allow(dead_code)]
pub(crate) fn shadow_proxy_bound(shape: &GpuShape, world_point: Vec3) -> f32 {
    if shape.blend.mode != GPU_MODE_ADD {
        return MAX_MARCH_DISTANCE;
    }
    let local_point = Quat::from_vec4(shape.inverse_rotation) * (world_point - shape.center);

    rounded_box_distance(
        local_point,
        shape.half_size,
        footprint_of(shape.half_size),
        shape.side_radius,
        shape.cap_radius,
    )
}

#[allow(dead_code)]
pub(crate) fn shadow_proxy_distance(shapes: &[GpuShape], grid: &SdfGrid, world_point: Vec3) -> f32 {
    let bound = |shape: &GpuShape| shadow_proxy_bound(shape, world_point);
    let everything = || shapes.iter().map(bound).fold(MAX_MARCH_DISTANCE, f32::min);

    if grid.cells.len() < grid.cell_count() * 2 || shapes.is_empty() || !grid.holds(world_point) {
        return everything();
    }
    let cell = grid.cell_of(world_point);
    let count = grid.cells[cell * 2 + 1];
    if count == GRID_CELL_FULL {
        return everything();
    }

    let offset = grid.cells[cell * 2] as usize;
    let field = (0..count as usize)
        .map(|slot| bound(&shapes[grid.indices[offset + slot] as usize]))
        .fold(MAX_MARCH_DISTANCE, f32::min);

    if count as usize == shapes.len() {
        return field;
    }
    field.min(grid.exit_distance(world_point))
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

pub(crate) fn cull_box_distance(offset: Vec3, half_extent: Vec3) -> f32 {
    (offset.abs() - half_extent).max(Vec3::ZERO).length()
}

pub(crate) fn shape_cannot_reach(shape: &GpuShape, world_point: Vec3, field: f32) -> bool {
    shape.blend.mode == GPU_MODE_ADD
        && shape.cull_scale * cull_box_distance(world_point - shape.center, shape.cull_extent)
            >= field
}

fn cull_bound(shape: &GpuShape) -> (Vec3, f32) {
    let footprint = footprint_of(shape.half_size);
    let slope = (shape.taper * footprint) / (2.0 * shape.half_size.y.max(MIN_RADIUS));
    let scale = 1.0 / (1.0 + slope * slope).sqrt();
    (
        world_aligned_extent(shape.half_size, shape.inverse_rotation)
            + Vec3::splat(shape.blend.radius / scale),
        scale,
    )
}
