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
    render::{render_resource::ShaderType, storage::ShaderBuffer},
};

use crate::args;
use crate::game::world::SdfWorld;
use crate::sdf::render::{Quad, SdfMaterial};

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

/// Which primitive a shape is. The unit shape only; its size lives in the
/// entity's `Transform`, exactly as SDF Modeler stores scale separately.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SdfShape {
    Sphere,
    #[default]
    Cube,
    Cylinder,
}

/// SDF Modeler's surface modifiers, each normalised 0..1. Which ones apply
/// depends on the brush: the editor offers Sharpen only on a sphere, Round only
/// on a cylinder, and all four of Thickness / Cone / Bevel / Round on a cube.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub(crate) struct Modifiers {
    pub(crate) round: f32,
    pub(crate) bevel: f32,
    /// 1.0 is solid; below that the shape is a shell of that fraction.
    pub(crate) thickness: f32,
    pub(crate) cone: f32,
    pub(crate) sharpen: f32,
}

impl Default for Modifiers {
    fn default() -> Self {
        Modifiers {
            round: 0.0,
            bevel: 0.0,
            thickness: 1.0,
            cone: 0.0,
            sharpen: 0.0,
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

/// The blend half of `GpuShape`, split out so both halves stay readable and the
/// struct keeps its 16-byte rows.
#[derive(ShaderType, Debug, Clone, PartialEq, Default)]
pub(crate) struct GpuBlend {
    pub(crate) mode: u32,
    /// Smoothing width, `k` in the editor's shaders.
    pub(crate) radius: f32,
    /// `r` in the editor's shaders. Only the offset-based modes read it.
    pub(crate) strength: f32,
    pub(crate) padding: f32,
}

/// Storage-buffer layout. Must match `struct Shape` in sdf.wgsl.
#[derive(ShaderType, Debug, Clone, PartialEq, Default)]
pub(crate) struct GpuShape {
    pub(crate) center: Vec3,
    pub(crate) brush: u32,
    /// `xyz` are world-space half sizes with scale baked in. `w` depends on the
    /// brush: wall thickness for a cube, the superellipsoid exponent for a
    /// sphere, the rim radius for a cylinder.
    pub(crate) s: Vec4,
    /// Cube only, the uber primitive's radii: `x` rounds the vertical edges,
    /// `y` the horizontal ones, `z` is the taper.
    pub(crate) r: Vec4,
    /// Rotates a world-space offset into the shape's local frame, so every
    /// distance function stays written in its own axis-aligned terms.
    pub(crate) inverse_rotation: Vec4,
    /// Linear RGB.
    pub(crate) albedo: Vec3,
    pub(crate) chamfer: u32,
    /// Half the axis-aligned box used to reject this shape cheaply. Not the
    /// shape's own bounding box: it is inflated so that `cull_scale` times the
    /// distance to it is a true lower bound on what `shape_distance` returns.
    pub(crate) cull_extent: Vec3,
    /// How much the evaluator can undershoot the real distance. 1.0 where the
    /// primitive is exact; below it where the estimate is deliberately
    /// conservative, as the ellipsoid's is.
    pub(crate) cull_scale: f32,
    pub(crate) blend: GpuBlend,
}

const GPU_BRUSH_SPHERE: u32 = 0;
const GPU_BRUSH_CUBE: u32 = 1;
const GPU_BRUSH_CYLINDER: u32 = 2;

// Blend modes, values as SDF Modeler's common.glsl defines them.
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

impl SdfShape {
    /// Packs one shape the way SDF Modeler feeds its own shader.
    ///
    /// The editor turns the normalised modifiers into shader arguments inside
    /// its binary, not in the GLSL, so this conversion was fitted against
    /// `sdf_uberprim` directly: the numbers below are the ones that make the
    /// field come out with its base at `scale` and its modifiers behaving the
    /// way the editor draws them.
    pub(crate) fn to_gpu(
        self,
        placement: &GlobalTransform,
        modifiers: Option<&Modifiers>,
        operation: Option<&CsgOperation>,
        albedo: Option<&Albedo>,
    ) -> GpuShape {
        let (scale, rotation, translation) = placement.to_scale_rotation_translation();
        let size = scale.abs().max(Vec3::splat(f32::EPSILON));
        let inverse_rotation = rotation.inverse();
        let modifiers = modifiers.copied().unwrap_or_default();
        let operation = operation.copied().unwrap_or_default();

        let (brush, s, r) = match self {
            SdfShape::Sphere => (
                GPU_BRUSH_SPHERE,
                size.extend(sharpen_exponent(modifiers.sharpen)),
                Vec4::ZERO,
            ),
            SdfShape::Cylinder => {
                let rim = modifiers.round * size.min_element();
                (GPU_BRUSH_CYLINDER, size.extend(rim), Vec4::ZERO)
            }
            SdfShape::Cube => {
                let flat = size.x.min(size.z);
                // Round works on every edge at once - at full strength a box
                // becomes a capsule, and a cube a sphere. Bevel only rounds the
                // four vertical ones, so the two share `r.x` and the stronger
                // wins.
                let rounded = modifiers.round * size.min_element();
                let bevelled = modifiers.bevel * flat;
                (
                    GPU_BRUSH_CUBE,
                    Vec4::new(
                        size.x,
                        size.y,
                        size.z,
                        wall_thickness(modifiers.thickness, flat),
                    ),
                    Vec4::new(
                        rounded.max(bevelled).min(flat),
                        rounded.min(size.y),
                        modifiers.cone,
                        0.0,
                    ),
                )
            }
        };

        let mut packed = GpuShape {
            center: translation,
            brush,
            s,
            r,
            inverse_rotation: Vec4::new(
                inverse_rotation.x,
                inverse_rotation.y,
                inverse_rotation.z,
                inverse_rotation.w,
            ),
            albedo: albedo.map(|albedo| albedo.0).unwrap_or(DEFAULT_ALBEDO),
            chamfer: u32::from(operation.chamfer),
            cull_extent: Vec3::ZERO,
            cull_scale: 1.0,
            blend: GpuBlend {
                mode: operation.mode,
                radius: operation.radius,
                strength: operation.strength,
                padding: 0.0,
            },
        };
        (packed.cull_extent, packed.cull_scale) = cull_bound(&packed);
        packed
    }
}

/// The uber primitive's thickness argument: half the wall, because it grows the
/// wall both ways from the surface it offsets. Measured against the footprint,
/// since the bore runs through the shape's height.
///
/// Thickness runs the other way from a wall: 1.0 is solid and 0.0 is as thin as
/// the wall can get. Solid is its own case rather than the limit - a wall that
/// exactly meets itself reads as a surface everywhere inside, and physics and
/// the normals need a real interior. It must not be confused with a wall of
/// zero, which is a real setting meaning the thinnest possible shell.
fn wall_thickness(thickness: f32, flat: f32) -> f32 {
    if thickness >= 1.0 {
        return flat;
    }
    thickness * flat * 0.5
}

/// Sharpen, as a superellipsoid exponent. 0 is a plain ellipsoid; turning it up
/// squares the sphere off while keeping its curvature.
///
/// ponytail: the curve from the slider to the exponent is a guess with the
/// right endpoints. Compare a sharpened sphere against the editor before
/// trusting the middle of the range.
fn sharpen_exponent(sharpen: f32) -> f32 {
    2.0 / (1.0 - sharpen.clamp(0.0, 0.95))
}

/// Everything needed to pack one shape. Named because statics and bodies are
/// queried separately but read identically.
type ShapeQuery = (
    &'static SdfShape,
    &'static GlobalTransform,
    Option<&'static Modifiers>,
    Option<&'static CsgOperation>,
    Option<&'static Albedo>,
);

fn pack_shape(
    (shape, placement, modifiers, operation, albedo): (
        &SdfShape,
        &GlobalTransform,
        Option<&Modifiers>,
        Option<&CsgOperation>,
        Option<&Albedo>,
    ),
) -> GpuShape {
    shape.to_gpu(placement, modifiers, operation, albedo)
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
    shapes: Query<ShapeQuery>,
    bodies: Query<ShapeQuery, With<SphereBody>>,
    quad: Single<&MeshMaterial3d<SdfMaterial>, With<Quad>>,
    mut materials: ResMut<Assets<SdfMaterial>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut scene: ResMut<SdfScene>,
    settings: Res<GridSettings>,
) {
    // In `Children` order, which is the order they were authored in. A plain
    // query would hand them back grouped by archetype instead, and any shape
    // that subtracts or intersects would land in the wrong place in the fold.
    let mut packed: Vec<GpuShape> = world
        .iter()
        .filter_map(|brush| shapes.get(brush).ok())
        .map(pack_shape)
        .collect();
    let mut static_count = packed.len();
    packed.extend(bodies.iter().map(pack_shape));

    if packed.len() > MAX_SHAPES {
        warn!(
            "scene has {} shapes, buffer holds {MAX_SHAPES}; the rest are dropped",
            packed.len()
        );
        packed.truncate(MAX_SHAPES);
        static_count = static_count.min(MAX_SHAPES);
    }

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
    material.render_params.bounds_min = bounds_min;
    material.render_params.bounds_max = bounds_max;
    material.render_params.shape_count = scene.shapes.len() as u32;

    let grid = build_grid(&scene.shapes, bounds_min, bounds_max, settings.resolution);
    material.render_params.grid = u32::from(settings.enabled);
    material.render_params.grid_resolution = grid.resolution;
    material.render_params.grid_origin = grid.origin;
    material.render_params.grid_cell = grid.cell_size;

    let handles = (
        material.shapes.clone(),
        material.grid_cells.clone(),
        material.grid_indices.clone(),
    );
    if let Some(mut buffer) = buffers.get_mut(&handles.0) {
        let mut padded = scene.shapes.clone();
        padded.resize(MAX_SHAPES, GpuShape::default());
        buffer.set_data(padded);
    }
    if let Some(mut buffer) = buffers.get_mut(&handles.1) {
        let mut padded = grid.cells.clone();
        padded.resize(GRID_CELL_WORDS, 0);
        buffer.set_data(padded);
    }
    if let Some(mut buffer) = buffers.get_mut(&handles.2) {
        let mut padded = grid.indices.clone();
        padded.resize(GRID_INDEX_WORDS, 0);
        buffer.set_data(padded);
    }
    scene.grid = grid;
}

// The primitives and blend operations below are ports of SDF Modeler's own
// shaders/sdf.glsl, so a scene authored against the editor evaluates to the
// field it drew. Where a name appears there, it is kept here.

// ------------------------------------------------------------------ the field

/// Superellipsoid. `exponent` 2 is a plain ellipsoid; larger values square it
/// off towards a bevelled box, which is the editor's Sharpen modifier. Scaling
/// by the smallest radius is what keeps it from overshooting.
pub(crate) fn ellipsoid_distance(local_point: Vec3, radii: Vec3, exponent: f32) -> f32 {
    let radii = radii.max(Vec3::splat(MIN_RADIUS));
    let scaled = (local_point / radii).abs().powf(exponent);
    ((scaled.x + scaled.y + scaled.z).powf(1.0 / exponent) - 1.0) * radii.min_element()
}

/// Exact distance to an ellipse, by five Newton steps around the boundary.
/// Exact matters here because it is the cylinder's whole cross-section.
fn ellipse_distance(point: Vec2, radii: Vec2) -> f32 {
    let point = point.abs();
    let radii = radii.max(Vec2::splat(MIN_RADIUS));
    // The iteration has nothing to walk towards from dead centre, and divides
    // by zero when it tries. The answer there is known anyway.
    if point.length_squared() < MIN_RADIUS * MIN_RADIUS {
        return -radii.min_element();
    }
    let offset = radii * (point - radii);
    let mut direction = if offset.x < offset.y {
        Vec2::new(0.01, 1.0)
    } else {
        Vec2::new(1.0, 0.01)
    }
    .normalize();

    for _ in 0..5 {
        let along = radii * direction;
        let across = radii * Vec2::new(-direction.y, direction.x);
        let a = (point - along).dot(across);
        let c = (point - along).dot(along) + across.dot(across);
        let b = (c * c - a * a).max(0.0).sqrt();
        direction = Vec2::new(
            direction.x * b - direction.y * a,
            direction.y * b + direction.x * a,
        ) / c.max(MIN_RADIUS);
    }

    let distance = (point - radii * direction).length();
    if (point / radii).length_squared() > 1.0 {
        distance
    } else {
        -distance
    }
}

/// Elliptical cross-section, axis along Y.
pub(crate) fn cylinder_distance(local_point: Vec3, radii: Vec3) -> f32 {
    let radial = ellipse_distance(local_point.xz(), radii.xz());
    let edge = Vec2::new(radial, local_point.y.abs() - radii.y);
    edge.max_element().min(0.0) + edge.max(Vec2::ZERO).length()
}

/// The editor's uber primitive: one function covering box, rounded box,
/// cylinder, capsule, cone and tube.
///
/// `s.xyz` are half sizes and `s.w` is the wall thickness of the hollow form.
/// `r.x` rounds the four vertical edges, `r.y` the horizontal ones, and `r.z`
/// tapers the top, widening the bottom by that much in the process - which is
/// why `SdfShape::to_gpu` shrinks `s.xz` by the taper before packing.
fn uberprim_distance(local_point: Vec3, s: Vec4, r: Vec3) -> f32 {
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

/// A cube's taper narrows its cross-section with height instead of using the
/// uber primitive's own `r.z`.
///
/// That is not a stylistic choice. `r.z` offsets the cross-section outwards,
/// and offsetting a rectangle outwards rounds its corners, while SDF Modeler
/// keeps a coned cube perfectly square. It also takes the *same amount* off
/// every side rather than scaling them, so a long slab tapers to a ridge
/// instead of shrinking towards a scaled-down copy of its footprint.
///
/// Insetting a rectangle costs one divide by the lateral slope to stay a safe
/// underestimate - a stack of cross-sections overstates the distance by exactly
/// that factor.
///
/// `s.w` is the wall. It goes to the uber primitive as its own thickness
/// argument, which hollows the shape laterally and leaves the ends open - a
/// tube, not a cup. Tapered, that tube becomes a funnel; see `bore`.
fn tapered_uberprim(local_point: Vec3, s: Vec4, r: Vec4) -> f32 {
    let flat = s.x.min(s.z);
    if r.z <= 0.0 {
        return uberprim_distance(
            local_point,
            s.truncate().extend(bore(s.w, 0.0, flat)),
            r.truncate().with_z(0.0),
        );
    }
    let taper = r.z * flat;
    let height_fraction = ((local_point.y / s.y + 1.0) * 0.5).clamp(0.0, 1.0);
    let inset = taper * height_fraction;

    let remaining = (flat - inset).max(0.0);
    let narrowed = Vec4::new(
        s.x - inset,
        s.y,
        s.z - inset,
        bore(s.w, taper - inset, remaining),
    );
    let corner = Vec3::new((r.x - inset).max(0.0), r.y, 0.0);

    let slope = taper / (2.0 * s.y);
    uberprim_distance(local_point, narrowed, corner) / (1.0 + slope * slope).sqrt()
}

/// The uber primitive's thickness argument at one height.
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

pub(crate) fn shape_distance(shape: &GpuShape, world_point: Vec3) -> f32 {
    let inverse_rotation = Quat::from_vec4(shape.inverse_rotation);
    let local_point = inverse_rotation * (world_point - shape.center);
    match shape.brush {
        GPU_BRUSH_SPHERE => ellipsoid_distance(local_point, shape.s.truncate(), shape.s.w),
        GPU_BRUSH_CYLINDER => {
            cylinder_distance(local_point, shape.s.truncate() - shape.s.w) - shape.s.w
        }
        _ => tapered_uberprim(local_point, shape.s, shape.r),
    }
}

// ----------------------------------------------------------- blend operations

/// Every operation takes the incoming shape first and the field built so far
/// second, matching `blend_op_ex` in the editor's sdf.glsl.
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
/// boolean they smooth - which is also what the editor draws.
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
    // `s.xyz` holds world-space half sizes for every brush, and a taper only
    // ever narrows the shape from there.
    let local_extent = shape.s.truncate();
    // Rotating a box grows its AABB by |R| * extent, taking each axis's
    // contribution regardless of sign.
    let rotation = Mat3::from_quat(Quat::from_vec4(shape.inverse_rotation).inverse());
    let unsigned = Mat3::from_cols(
        rotation.x_axis.abs(),
        rotation.y_axis.abs(),
        rotation.z_axis.abs(),
    );
    unsigned * local_extent + Vec3::splat(shape.blend.radius)
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
            blend(distance, field, &shape.blend, shape.chamfer != 0)
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
    let local = Quat::from_vec4(shape.inverse_rotation) * (world_point - shape.center);
    let half = shape.s.truncate();

    match shape.brush {
        // The field's own estimate. It is already compiled in for the camera
        // march and costs three `powf`, nothing like the uberprim.
        GPU_BRUSH_SPHERE => ellipsoid_distance(local, half, shape.s.w),
        // A round cross-section at the wider radius, which contains the ellipse
        // the shape actually has - and is exact for a round cylinder, which is
        // most of them. `ellipse_distance` is five Newton steps and is the one
        // thing in the field more expensive than the uberprim.
        GPU_BRUSH_CYLINDER => {
            let radial = local.xz().length() - half.x.max(half.z);
            let edge = Vec2::new(radial, local.y.abs() - half.y);
            edge.max_element().min(0.0) + edge.max(Vec2::ZERO).length()
        }
        // Every cube modifier only ever removes material - round and bevel cut
        // the edges, cone narrows the top, thickness hollows an interior
        // nothing sees from outside. So the plain box contains all of them, and
        // is exact for a cube nobody has touched.
        _ => cull_box_distance(local, half),
    }
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
/// it, so the first shape simply seeds the field - the editor does not apply an
/// operation to it either.
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
            blend(distance, field, &shape.blend, shape.chamfer != 0)
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
    let size = shape.s.truncate();
    let (local_extent, scale) = match shape.brush {
        // The superellipsoid estimate is `(||p/r||e - 1) * min(r)`, which along
        // the longest axis is only `min(r)/max(r)` of the true distance. Any
        // norm is at least the infinity norm, so bounding the box by
        // `sqrt(3) * max(r)` covers every exponent and every direction.
        GPU_BRUSH_SPHERE => {
            let reach = 3.0f32.sqrt() * size.max_element();
            (Vec3::splat(reach), size.min_element() / reach)
        }
        // The taper divides the whole distance by `sqrt(1 + slope^2)` to keep
        // it conservative on the sloped face, so the estimate falls short by
        // exactly that.
        GPU_BRUSH_CUBE => {
            let slope = (shape.r.z * size.x.min(size.z)) / (2.0 * size.y.max(MIN_RADIUS));
            (size, 1.0 / (1.0 + slope * slope).sqrt())
        }
        // Exact: the ellipse is solved by Newton, the rim is an offset.
        _ => (size, 1.0),
    };
    let rotation = Mat3::from_quat(Quat::from_vec4(shape.inverse_rotation).inverse());
    let unsigned = Mat3::from_cols(
        rotation.x_axis.abs(),
        rotation.y_axis.abs(),
        rotation.z_axis.abs(),
    );
    (
        unsigned * local_extent + Vec3::splat(shape.blend.radius / scale),
        scale,
    )
}
