//! The signed distance field: the shapes, how they pack, and how to evaluate
//! them on the CPU.
//!
//! This is the module the others are built on. `render` draws what is here and
//! `physics` queries it; nothing here knows about either.
//!
//! Every function under "the field" is mirrored by one of the same name in
//! `assets/shaders/sdf.wgsl`. Both read the **same packed [`GpuShape`] bytes**,
//! so only the arithmetic can drift. Change one, change the other.

use bevy::{
    prelude::*,
    render::{
        render_resource::{ShaderType, encase::internal::WriteInto},
        storage::ShaderBuffer,
    },
};

use crate::args;
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

/// Must match MAX_MARCH_DISTANCE in sdf.wgsl - it is the empty-scene distance.
pub(crate) const MAX_MARCH_DISTANCE: f32 = 100.0;
/// Must match NORMAL_EPSILON in sdf.wgsl.
const SURFACE_EPSILON: f32 = 0.0005;
/// Smallest radius a curved primitive may shrink to before its distance
/// estimate divides by zero. Mirrored by `MIN_RADIUS` in sdf.wgsl.
const MIN_RADIUS: f32 = 1e-5;
/// The storage buffer is allocated once at this size and never resized. A
/// resize makes Bevy rebuild the GPU buffer while the material's bind group
/// still points at the old one, so the shader reads a stale, truncated scene.
/// ponytail: fixed ceiling. Raise it, or grow-and-rebind, when scenes get big.
pub(crate) const MAX_SHAPES: usize = 256;

// ----------------------------------------------------------------- components

/// One brush in the field. A brush is always a box; its size is the entity's
/// `Transform` scale and its shape is its [`Modifiers`].
///
/// There is one primitive on purpose. Four modifiers over a box reach every
/// shape the field needs, and two of them land exactly:
///
/// | round | bevel | cone | shape |
/// |-------|-------|------|-------|
/// | 0 | 0 | 0 | box |
/// | **1** | - | 0 | sphere, exactly |
/// | 0 | **1** | 0 | cylinder, exactly |
/// | 0 | 0 | **1** | pyramid |
/// | 0 | **1** | **1** | cone |
///
/// A second primitive would be a second distance function in both evaluators,
/// and the sphere and cylinder it replaced were the two most expensive things
/// in the field.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Brush;

/// What a box is bent into, each normalised 0..1.
///
/// They compose, and they compete for the same footprint: asking for a full
/// round and a full taper at once clamps rather than collapsing the shape.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub(crate) struct Modifiers {
    /// Rounds every edge at once. At full strength a box becomes a sphere and
    /// an oblong box becomes a capsule.
    pub(crate) round: f32,
    /// Rounds the four vertical edges only, leaving the top and bottom flat.
    /// At full strength the cross-section is a circle: a cylinder.
    pub(crate) bevel: f32,
    /// 1.0 is solid; below that the shape is a shell of that fraction.
    pub(crate) thickness: f32,
    /// Narrows the shape towards its top. At full strength the top closes to a
    /// point.
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

/// How a shape combines with everything declared before it. Absent means a
/// hard union.
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

// ------------------------------------------------------------ the packed form

/// How a brush combines with the field built before it. One 16-byte row of
/// [`GpuShape`].
#[derive(ShaderType, Debug, Clone, PartialEq, Default)]
pub(crate) struct GpuBlend {
    pub(crate) mode: u32,
    /// Width of the smooth or chamfered seam. Zero is a hard union.
    pub(crate) radius: f32,
    /// How far the offset-based modes push, emboss and deboss the field. The
    /// other modes ignore it.
    pub(crate) strength: f32,
    /// Squares the seam off instead of rounding it.
    pub(crate) chamfer: u32,
}

/// Storage-buffer layout. Must match `struct Shape` in sdf.wgsl.
///
/// Every field is a named scalar rather than a packed vector, and the rows are
/// arranged so each `Vec3` starts on a 16-byte boundary. Both evaluators index
/// this by name, which is the only defence against the two of them drifting.
#[derive(ShaderType, Debug, Clone, PartialEq, Default)]
pub(crate) struct GpuShape {
    pub(crate) center: Vec3,
    /// Half the wall of a hollow shape, in world units. Equal to the footprint
    /// when the shape is solid - see [`wall_thickness`].
    pub(crate) wall_thickness: f32,
    /// World-space half extent, scale baked in.
    pub(crate) half_size: Vec3,
    /// Rounds the four vertical edges, so it is the corner radius of the
    /// cross-section. Equal to the footprint makes that cross-section a circle.
    pub(crate) side_radius: f32,
    /// Rotates a world-space offset into the brush's local frame, so every
    /// distance function stays written in its own axis-aligned terms.
    pub(crate) inverse_rotation: Vec4,
    /// Linear RGB.
    pub(crate) albedo: Vec3,
    /// Rounds the top and bottom rim.
    pub(crate) cap_radius: f32,
    /// Half the axis-aligned box used to reject this brush cheaply. Not its
    /// bounding box: it is inflated so that `cull_scale` times the distance to
    /// it is a true lower bound on what [`shape_distance`] returns.
    pub(crate) cull_extent: Vec3,
    /// How much the evaluator can undershoot the real distance. 1.0 where the
    /// shape is exact, below it where the estimate is deliberately
    /// conservative - as a taper's is.
    pub(crate) cull_scale: f32,
    /// How much of the footprint the top loses, as a fraction. 1.0 closes it to
    /// a point.
    pub(crate) taper: f32,
    pub(crate) padding_one: f32,
    pub(crate) padding_two: f32,
    pub(crate) padding_three: f32,
    pub(crate) blend: GpuBlend,
}

// Blend modes, mirrored by the `MODE_` constants in sdf.wgsl.
pub(crate) const GPU_MODE_ADD: u32 = 0;
pub(crate) const GPU_MODE_SUBTRACT: u32 = 1;
pub(crate) const GPU_MODE_INTERSECT: u32 = 2;
pub(crate) const GPU_MODE_PAINT: u32 = 3;
pub(crate) const GPU_MODE_PUSH: u32 = 4;
pub(crate) const GPU_MODE_AVOID: u32 = 5;
pub(crate) const GPU_MODE_EMBOSS: u32 = 6;
pub(crate) const GPU_MODE_DEBOSS: u32 = 7;
pub(crate) const GPU_MODE_SHELL: u32 = 8;

/// The packed scene, exactly as the GPU sees it. Physics and any other CPU
/// query read these same bytes, so there is one scene, not two.
///
/// Static brushes are packed first. Bodies are drawn but must not collide with
/// themselves, so collision queries stop at `static_count`.
#[derive(Resource, Default)]
pub(crate) struct SdfScene {
    pub(crate) shapes: Vec<GpuShape>,
    pub(crate) static_count: usize,
    /// The render-side acceleration grid, kept here so it is built once
    /// alongside the packing. Physics does not use it: the CPU field stays
    /// exact, and the grid only ever returns a more conservative answer.
    pub(crate) grid: SdfGrid,
}

impl SdfScene {
    pub(crate) fn static_shapes(&self) -> &[GpuShape] {
        &self.shapes[..self.static_count]
    }
}

/// A sphere that falls, collides with the static field, and collides with other
/// bodies. Drawn by being part of the same field, so it carries an `SdfShape`
/// and `Transform` like any other shape - there is no separate body renderer.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct SphereBody {
    pub(crate) radius: f32,
    pub(crate) velocity: Vec3,
    pub(crate) angular_velocity: Vec3,
    /// Integrated from `angular_velocity`. A sphere looks identical however it
    /// is turned, so this exists to be drawn, not to shape the field.
    pub(crate) orientation: Quat,
    /// Parked on a surface. Skips integration entirely until the ground goes away.
    pub(crate) resting: bool,
}

/// Surface colour, linear RGB. Absent means the default clay.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct Albedo(pub(crate) Vec3);

impl Default for Albedo {
    fn default() -> Self {
        Albedo(DEFAULT_ALBEDO)
    }
}

const DEFAULT_ALBEDO: Vec3 = Vec3::new(1.0, 0.4, 0.2);

// -------------------------------------------------------------------- packing

/// Packs one brush into the bytes both evaluators read.
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

/// The narrower of the two horizontal half extents. Round, bevel and taper are
/// all measured against it, so a plate tapers to a ridge rather than to a
/// smaller plate of the same proportions.
fn footprint_of(half_size: Vec3) -> f32 {
    half_size.x.min(half_size.z)
}

/// Round and bevel, resolved into the two corner radii the distance function
/// takes: one for the vertical edges, one for the top and bottom rim.
///
/// They share the vertical-edge radius and the stronger wins, which is what
/// makes a full round a sphere and a full bevel a cylinder. Both are clamped so
/// a radius can never exceed the shape it is rounding.
fn corner_radii(modifiers: &Modifiers, half_size: Vec3) -> (f32, f32) {
    let footprint = footprint_of(half_size);
    let rounded = modifiers.round * half_size.min_element();
    let bevelled = modifiers.bevel * footprint;
    (
        rounded.max(bevelled).min(footprint),
        rounded.min(half_size.y),
    )
}

/// A quaternion as the four words the shader reads it back as.
fn quaternion_words(rotation: Quat) -> Vec4 {
    Vec4::new(rotation.x, rotation.y, rotation.z, rotation.w)
}

/// Half the wall of a hollow shape, because the wall grows both ways from the
/// surface it offsets. Measured against the footprint, since the bore runs
/// through the shape's height.
///
/// Thickness runs the other way from a wall: 1.0 is solid and 0.0 is as thin as
/// the wall can get. Solid is its own case rather than the limit - a wall that
/// exactly meets itself reads as a surface everywhere inside, and physics and
/// the normals need a real interior. It must not be confused with a wall of
/// zero, which is a real setting meaning the thinnest possible shell.
fn wall_thickness(thickness: f32, footprint: f32) -> f32 {
    if thickness >= 1.0 {
        return footprint;
    }
    thickness * footprint * 0.5
}

/// Everything needed to pack one brush. Named because statics and bodies are
/// queried separately but read identically.
type BrushQuery = (
    &'static Brush,
    &'static GlobalTransform,
    Option<&'static Modifiers>,
    Option<&'static CsgOperation>,
    Option<&'static Albedo>,
);

/// The query row, in the argument order [`pack_brush`] wants.
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

/// Mirrors every `SdfShape` entity into the material's storage buffer, and
/// recomputes the scene bounds the shader uses to cull rays.
///
/// Statics are packed first so `SdfScene::static_shapes` can hand physics a
/// prefix that excludes the bodies themselves. Packing runs every frame because
/// it is cheap; the upload and the bind-group rebuild are gated on the packed
/// data actually differing.
#[allow(clippy::too_many_arguments)] // one system, one job: pack and upload
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

    // Repacking is cheap; the upload, the grid rebuild and the bind-group
    // rebuild are not. A still scene should cost none of them.
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

/// Every brush in blend order, followed by the bodies, and how many of them are
/// static.
///
/// The statics come from the root's `Children`, which is the order they were
/// authored in. A plain query hands them back grouped by archetype instead, and
/// any brush that subtracts or intersects would land in the wrong place in the
/// fold.
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

/// Everything the shader needs to know about the scene as a whole, as opposed
/// to about one brush.
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

/// Writes `values` into a fixed-size storage buffer, zero-filling the rest.
///
/// The buffers are never resized. A resize makes Bevy rebuild the GPU buffer
/// while the material's bind group still points at the old one, so the shader
/// reads a stale, truncated scene.
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

// ------------------------------------------------------------------ the field

/// Distance to one brush: a box, with its edges rounded, its top narrowed and
/// its inside hollowed out, in whatever combination the modifiers asked for.
///
/// Mirrored by `shape_distance` in sdf.wgsl.
pub(crate) fn shape_distance(shape: &GpuShape, world_point: Vec3) -> f32 {
    let local_point = Quat::from_vec4(shape.inverse_rotation) * (world_point - shape.center);
    tapered_box_distance(local_point, shape)
}

/// A box whose cross-section shrinks with height.
///
/// The taper is applied by insetting the cross-section per height rather than
/// by the offset a rounded box would use. Offsetting a rectangle outwards
/// rounds its corners, and a taper has to leave a square base square. It also
/// takes the *same amount* off every side rather than scaling them, so a long
/// slab tapers to a ridge instead of to a scaled-down copy of its footprint.
///
/// Insetting costs one divide by the lateral slope to stay a safe
/// underestimate: a stack of cross-sections overstates the distance by exactly
/// that factor, and an overstatement is what lets a march step through a
/// surface.
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

/// A box with its four vertical edges rounded by `side_radius`, its top and
/// bottom rim by `cap_radius`, and its inside hollowed out to leave a wall of
/// `wall`.
///
/// One function reaches every shape the field has: a box at both radii zero, a
/// sphere when both equal the half extent, a cylinder at `side_radius` equal to
/// the footprint and `cap_radius` zero, and a tube whenever the wall is less
/// than the footprint.
///
/// The construction is the usual one. Shrink the box by its radii, measure the
/// distance to what is left, then add the radius back - that rounds every
/// corner the shrunken box has. `abs()` on the cross-section is what hollows
/// it: it folds the solid into a wall either side of the old surface.
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
    // Both radii are taken off the shape *and* off each other's arguments, so
    // a shape rounded on every edge at once still closes to a sphere rather
    // than over-rounding into nothing.
    let side_radius = side_radius - wall;
    let wall = wall - cap_radius;

    let corner = local_point.abs() - inner;
    let flat = Vec2::new(corner.x, corner.z);
    let cross_section = flat.max(Vec2::ZERO).length() + flat.max_element().min(0.0) - side_radius;

    let profile = Vec2::new(cross_section.abs() - wall, corner.y);
    profile.max_element().min(0.0) + profile.max(Vec2::ZERO).length() - cap_radius
}

/// The wall at one height.
///
/// A tapered shell is a funnel, not a tube of constant wall: the bore closes
/// off towards the wide end, so the wall thickens as the shape widens and the
/// hole is a slit at the narrow end. The wall therefore carries whatever taper
/// has not been spent yet - all of it at the base, none of it at the top, where
/// the wall matches an untapered shape exactly.
///
/// Never more than there is room for, or the two sides pass through each other.
pub(crate) fn bore(wall: f32, unspent_taper: f32, remaining: f32) -> f32 {
    (wall + unspent_taper).min(remaining)
}

/// The combined primitive the rounded-box kernel replaced, kept only so
/// `the_kernel_matches_the_uberprim_it_replaced` can check they agree.
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

/// The simplified kernel, in the legacy one's argument shape, so the test can
/// drive both from the same random inputs.
#[cfg(test)]
pub(crate) fn rounded_box_in_legacy_terms(local_point: Vec3, s: Vec4, r: Vec3) -> f32 {
    rounded_box_distance(local_point, s.truncate(), s.w, r.x, r.y)
}

// ----------------------------------------------------------- blend operations

/// Every operation takes the incoming shape first and the field built so far
/// second. Mirrored by the same names in sdf.wgsl.
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

/// The three booleans, each picking its variant: chamfered, smoothed, or plain.
/// Written the same way as `op_union` and friends in sdf.wgsl so the two
/// evaluators read alike.
///
/// At radius zero the smooth ops divide by zero, so they fall back to the plain
/// boolean they smooth.
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

/// Combines one shape with everything already in the field. `radius` is the
/// smoothing width and `strength` the offset the offset-based modes work over.
pub(crate) fn blend(shape: f32, field: f32, blend: &GpuBlend, chamfer: bool) -> f32 {
    let (radius, strength) = (blend.radius, blend.strength);
    match blend.mode {
        GPU_MODE_SUBTRACT => op_subtract(shape, field, radius, chamfer),
        GPU_MODE_INTERSECT => op_intersect(shape, field, radius, chamfer),
        // Paint only recolours, so the field is left exactly as it was.
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

// --------------------------------------------------------------- scene bounds

/// Half the world-space size of a shape's axis-aligned bounding box, plus the
/// slack a smooth blend can push the surface outwards by.
fn shape_half_extent(shape: &GpuShape) -> Vec3 {
    // A taper only ever narrows the shape from its half size, so the untapered
    // box bounds every shape a brush can be.
    world_aligned_extent(shape.half_size, shape.inverse_rotation) + Vec3::splat(shape.blend.radius)
}

/// Half the world-axis-aligned box holding a rotated local box.
///
/// Rotating a box grows its AABB by `|R| * extent`, taking each axis's
/// contribution regardless of sign.
fn world_aligned_extent(local_extent: Vec3, inverse_rotation: Vec4) -> Vec3 {
    let rotation = Mat3::from_quat(Quat::from_vec4(inverse_rotation).inverse());
    let unsigned = Mat3::from_cols(
        rotation.x_axis.abs(),
        rotation.y_axis.abs(),
        rotation.z_axis.abs(),
    );
    unsigned * local_extent
}

/// Slack on the culling box. A surface sitting exactly on the boundary is only
/// grazed by the rays that should hit it, and the slab test rejects them, which
/// eats the face of whichever shape defines the bound.
const BOUNDS_SLACK: f32 = 0.05;

/// Box holding every shape that adds material. The modes that only remove it
/// cannot push the bounds outwards and are skipped.
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

// ------------------------------------------------------------------- the grid

/// Cells along each axis of the acceleration grid, and the ceiling the buffers
/// are sized for. `bench --grid <n>` sweeps the live value.
pub(crate) const GRID_DEFAULT_RESOLUTION: u32 = 16;
pub(crate) const GRID_MAX_RESOLUTION: u32 = 32;
const GRID_MAX_CELLS: usize =
    (GRID_MAX_RESOLUTION * GRID_MAX_RESOLUTION * GRID_MAX_RESOLUTION) as usize;
/// Words in the cell table: offset and count for every cell the buffer could
/// ever hold.
pub(crate) const GRID_CELL_WORDS: usize = GRID_MAX_CELLS * 2;
pub(crate) const GRID_INDEX_WORDS: usize = GRID_MAX_ENTRIES;
/// Ceiling on total cell memberships. A cell that would push past it is marked
/// [`GRID_CELL_FULL`] instead, which is slower but never wrong.
const GRID_MAX_ENTRIES: usize = 1 << 18;
/// A cell that gave up indexing: evaluate every shape here.
pub(crate) const GRID_CELL_FULL: u32 = u32::MAX;

/// How the grid is built and whether it is used at all.
///
/// `--grid <n>` and `--no-grid` on any run. Finer culls better and makes empty
/// space cost more steps, so the useful value is measured rather than reasoned
/// about - which is the reason it is a knob and not a constant.
#[derive(Resource, Debug, Clone, Copy)]
pub(crate) struct GridSettings {
    pub(crate) resolution: u32,
    pub(crate) enabled: bool,
}

impl Default for GridSettings {
    fn default() -> Self {
        GridSettings {
            resolution: args::value("--grid").map_or(GRID_DEFAULT_RESOLUTION, |cells| cells as u32),
            enabled: !args::flag("--no-grid"),
        }
    }
}

/// Which shapes each cell has to evaluate.
///
/// A cell lists, in blend order:
///
/// - shape 0, always, because it seeds the field
/// - every shape whose mode is **not** ADD, always: subtract, intersect, push,
///   avoid, emboss, deboss and shell all read the field itself, so one of them
///   on the far side of the level still changes the answer here
/// - every ADD shape whose cull box overlaps the cell
///
/// Storing indices rather than a bitmask is what makes the march loop short
/// instead of merely cheap per shape.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub(crate) struct SdfGrid {
    pub(crate) origin: Vec3,
    /// Cubic. Dividing every axis by the same count instead gives pancake
    /// cells on a flat scene, and the thinnest axis then sets the step length
    /// for every ray - which is a crawl, not an acceleration.
    pub(crate) cell_size: Vec3,
    /// Cells along each axis. Derived from the bounds, so a wide flat world
    /// gets many cells across and few up.
    pub(crate) resolution: UVec3,
    /// Two entries per cell: offset into `indices`, then count - or
    /// [`GRID_CELL_FULL`] as the count.
    pub(crate) cells: Vec<u32>,
    pub(crate) indices: Vec<u32>,
}

// The grid's own lookups exist twice, like everything else in the field: here
// as the reference the tests can run, and in sdf.wgsl as the one that ships.
// Nothing on the CPU marches, so outside tests these are unused by design.
#[allow(dead_code)]
impl SdfGrid {
    fn cell_count(&self) -> usize {
        (self.resolution.x * self.resolution.y * self.resolution.z) as usize
    }

    /// Cell holding a point, clamped to the grid. Mirrors `grid_cell` in
    /// sdf.wgsl.
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

    /// Whether a point is inside the grid volume at all. Mirrors
    /// `grid_holds` in sdf.wgsl.
    ///
    /// Outside it there is no cell to clamp to: the lookup would land in an
    /// edge cell whose wall the point is already past, report a wall distance
    /// of zero, and the march would take that as a surface. The camera usually
    /// starts outside the scene bounds, so this is the common case, not a
    /// corner one.
    pub(crate) fn holds(&self, point: Vec3) -> bool {
        let high = self.origin + self.cell_size * self.resolution.as_vec3();
        point.cmpge(self.origin).all() && point.cmple(high).all()
    }

    /// How far each cell reaches past its own walls. Cells overlap by this
    /// much, so the box a lookup measures against is bigger than the box that
    /// chose it.
    ///
    /// Half a cell, not a sliver. A thin margin makes a ray travelling *along*
    /// a wall - which is what every ray does when the camera sits on a cell
    /// boundary - see a wall distance near zero at every step, crawl, run out
    /// of budget and die. That drew as a slice of the world missing down the
    /// middle of the screen.
    ///
    /// With half a cell of overlap the smallest distance any point in a cell
    /// can report is half a cell, so a march always makes real progress. The
    /// price is memberships: eight times as many, since every shape now
    /// rasterises into twice the cells per axis.
    ///
    /// Mirrors `grid_overlap` in sdf.wgsl.
    pub(crate) fn overlap(cell_size: Vec3) -> Vec3 {
        cell_size * 0.5
    }

    /// Distance from a point to the wall of its own cell. Mirrors
    /// `grid_exit_distance` in sdf.wgsl.
    ///
    /// This is the load-bearing part. A cell only knows its own shapes, so the
    /// distance it reports may be far too large - the next cell could hold a
    /// surface one step away. Clamping to the cell wall makes the answer
    /// conservative again: nothing outside can be reached without crossing it.
    pub(crate) fn exit_distance(&self, point: Vec3) -> f32 {
        let slot = self.slot_of(point);
        let overlap = Self::overlap(self.cell_size);
        let low = self.origin + slot * self.cell_size - overlap;
        let high = self.origin + (slot + Vec3::ONE) * self.cell_size + overlap;
        let to_wall = (point - low).min(high - point);
        to_wall.min_element().max(0.0)
    }
}

/// Builds the grid by rasterising each shape's cull box into cells.
///
/// Two passes and a prefix sum rather than a vector per cell: a moving body
/// rebuilds this every frame, so allocation churn would show up in the frame
/// time it is meant to save.
pub(crate) fn build_grid(
    shapes: &[GpuShape],
    bounds_min: Vec3,
    bounds_max: Vec3,
    resolution: u32,
) -> SdfGrid {
    let requested = resolution.clamp(1, GRID_MAX_RESOLUTION);
    let span = (bounds_max - bounds_min).max(Vec3::splat(MIN_RADIUS));
    // One cubic cell size, from the longest axis. The other axes then get
    // however many cells they need, which is what keeps a flat world from
    // being sliced into pancakes.
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

    // Inflated by the same overlap `exit_distance` measures against, so a cell
    // knows every shape inside the box it reports distances to.
    let margin = SdfGrid::overlap(cell_size);
    // Which cells one shape belongs to: everything, or the box it covers.
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

    // Pass one: count.
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

    // Over the ceiling: every cell evaluates everything. Slower than a grid,
    // exactly as correct, and it cannot silently drop a shape.
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

    // Pass two: fill. Shapes go in ascending order, so every cell's list is
    // already in blend order - which is the whole reason this is not a set.
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

/// The field, evaluated through the grid. Mirrors `scene_distance_gridded` in
/// sdf.wgsl.
///
/// Returns a value that is never larger than [`scene_distance`] would give, so
/// a march using it can never step through a surface.
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

    // Only a cell that already holds every shape may report its field
    // unclamped. Any other has to admit it cannot see past its own walls.
    if count as usize == shapes.len() {
        return field;
    }
    field.min(grid.exit_distance(world_point))
}

/// A lower bound on the field, built from the shapes' cull boxes and nothing
/// else. Mirrors `shadow_proxy_distance` in sdf.wgsl.
///
/// The shadow march reads this instead of the field. A second call to
/// `shape_distance` anywhere in the shader costs 13.9 ms on `spread:80` through
/// register pressure alone - whether or not anything casts a shadow, and
/// whether it marches 12 steps or 48. Measured 2026-09-01, table in
/// memory/reference/lights.md.
///
/// Wrong in two visible ways, both of them shape rather than size. A cube with
/// a heavy round or taper casts the shadow of the box it started as. And only
/// ADD contributes, so a hole carved by SUBTRACT does not let light through.
///
/// ponytail: the hole needs the real field, which is the thing this avoids.
/// One shape's contribution to [`shadow_proxy_distance`]: the cheapest shape
/// that still *contains* the real one, in the real one's own frame. Mirrors
/// `shadow_proxy_bound` in sdf.wgsl.
///
/// Containment is the whole argument. A shape that swallows another is never
/// further from a point than the shape inside it, so this can only report a
/// shorter distance than the field - which a march may act on, because acting
/// early is stopping early.
///
/// Deliberately not [`GpuShape::cull_extent`]: that box carries the blend
/// radius and is axis-aligned about `center`, which drew shadows several times
/// the size of what cast them and slabs where a rotated plate stood.
///
/// Non-ADD modes sit this out. They read the field rather than adding to it, so
/// nothing about their own extent bounds what they do to it.
#[allow(dead_code)]
pub(crate) fn shadow_proxy_bound(shape: &GpuShape, world_point: Vec3) -> f32 {
    if shape.blend.mode != GPU_MODE_ADD {
        return MAX_MARCH_DISTANCE;
    }
    let local_point = Quat::from_vec4(shape.inverse_rotation) * (world_point - shape.center);
    // Solid and untapered. Both a wall and a taper only ever remove material,
    // so the shape without them contains the shape with them - and for a brush
    // that has neither this is not a bound at all but the field itself.
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

// ------------------------------------------------------------------ the scene

/// The four corners of a tetrahedron. Sampling the field at each and weighting
/// by its direction gives the gradient in 4 taps instead of 6.
const TETRAHEDRON_CORNERS: [Vec3; 4] = [
    Vec3::new(1.0, -1.0, -1.0),
    Vec3::new(-1.0, -1.0, 1.0),
    Vec3::new(-1.0, 1.0, -1.0),
    Vec3::new(1.0, 1.0, 1.0),
];

/// Surface normal of the field. Mirrors `surface_normal` in sdf.wgsl.
pub(crate) fn scene_normal(shapes: &[GpuShape], world_point: Vec3) -> Vec3 {
    TETRAHEDRON_CORNERS
        .iter()
        .map(|corner| *corner * scene_distance(shapes, world_point + *corner * SURFACE_EPSILON))
        .sum::<Vec3>()
        .normalize_or_zero()
}

/// Distance to the whole scene. Mirrors `scene_distance` in sdf.wgsl.
///
/// Shapes are applied in order and each one blends against everything before
/// it, so the first shape simply seeds the field, with nothing to blend
/// against.
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

// -------------------------------------------------------------------- culling

/// Distance to a shape's cull box. Zero inside it, where no useful bound
/// exists.
///
/// Mirrors `cull_box_distance` in sdf.wgsl.
pub(crate) fn cull_box_distance(offset: Vec3, half_extent: Vec3) -> f32 {
    (offset.abs() - half_extent).max(Vec3::ZERO).length()
}

/// Whether a shape is far enough away that blending it in would leave the field
/// exactly as it is - in which case the expensive evaluation can be skipped.
///
/// Only sound for `ADD`. A union takes the nearer of the two, so a shape that
/// cannot get nearer changes nothing; the box is already inflated by the blend
/// radius, which is the reach of a smooth or chamfered union. Every other mode
/// needs its own proof: subtract compares `-shape` against the field, and
/// intersect, push, avoid, emboss, deboss and shell each read the field
/// through a different formula. Widening this is a per-mode job with a parity
/// test, not a guess.
///
/// Mirrors `shape_cannot_reach` in sdf.wgsl.
pub(crate) fn shape_cannot_reach(shape: &GpuShape, world_point: Vec3, field: f32) -> bool {
    shape.blend.mode == GPU_MODE_ADD
        && shape.cull_scale * cull_box_distance(world_point - shape.center, shape.cull_extent)
            >= field
}

/// The cull box and its scale, so that for every point outside the box
///
/// ```text
/// cull_scale * distance_to_box <= shape_distance(shape, point)
/// ```
///
/// A geometric bounding box is **not** enough. `shape_distance` does not
/// always return the true distance - `ellipsoid_distance` deliberately
/// underestimates, by as much as the ratio of the longest radius to the
/// shortest - and a bound the evaluator undercuts culls shapes that were still
/// about to change the field.
///
/// Each brush therefore contributes two things: a box its own estimate is
/// never nearer than, and the factor by which that estimate can fall short.
/// The box is then inflated by `blend.radius / cull_scale`, which is what makes
/// the ADD predicate hold for a smooth or chamfered union as well as a hard
/// one.
fn cull_bound(shape: &GpuShape) -> (Vec3, f32) {
    // The taper divides the whole distance by `sqrt(1 + slope^2)` to stay
    // conservative on the sloped face, so the estimate falls short by exactly
    // that and the box has to be inflated by the same factor.
    let footprint = footprint_of(shape.half_size);
    let slope = (shape.taper * footprint) / (2.0 * shape.half_size.y.max(MIN_RADIUS));
    let scale = 1.0 / (1.0 + slope * slope).sqrt();
    (
        world_aligned_extent(shape.half_size, shape.inverse_rotation)
            + Vec3::splat(shape.blend.radius / scale),
        scale,
    )
}
