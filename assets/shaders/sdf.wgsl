// Signed distance field renderer: ray marching in the fragment stage, one draw
// call over a frustum-fitted quad, against a uniform grid of shape lists.
//
// Every function under "the field" is mirrored by one of the same name in
// src/sdf/field.rs, which the physics uses. Both read the same packed `Shape`
// values, so only the arithmetic can drift. Change one, change the other.
//
// The exception is `ray_march`: it relaxes its hit threshold with distance
// because sub-pixel precision is invisible, while the CPU copy stays exact.

#import bevy_pbr::mesh_functions::{get_world_from_local, mesh_position_local_to_world}
#import bevy_pbr::mesh_view_bindings::view

struct RenderParams {
    bounds_min: vec3<f32>,
    tan_half_fov: f32,
    bounds_max: vec3<f32>,
    padding_one: f32,
    shape_count: u32, // the buffer is a fixed size; only this many are real
    debug_view: u32, // 0 = shaded, 1 = march-step heatmap
    cull: u32, // 0 turns the per-shape box reject off, for measuring it
    // Over-relaxation factor for the march. 1.0 is plain sphere tracing.
    omega: f32,
    grid: u32, // 0 turns the acceleration grid off, for measuring it
    grid_padding: u32,
    grid_padding_two: u32,
    grid_origin: vec3<f32>,
    grid_padding_three: f32,
    grid_cell: vec3<f32>, // cubic, so a flat world is not sliced into pancakes
    grid_padding_four: f32,
    grid_resolution: vec3<u32>, // cells along each axis
    light_count: u32, // how many of the fixed-size light buffer are real
    shadow_steps: u32,
    shadow_padding: u32,
    shadow_padding_two: u32,
    shadow_padding_three: u32,
};

/// One primitive, packed by `GpuShape::to_gpu` on the Rust side. Byte layout
/// must match `struct GpuShape` in src/sdf/field.rs exactly.
struct Shape {
    center: vec3<f32>,
    brush: u32,
    // xyz are world-space half sizes with scale baked in. w depends on the
    // brush: wall thickness for a cube, superellipsoid exponent for a sphere,
    // rim radius for a cylinder.
    s: vec4<f32>,
    // Cube only. The uber primitive's radii: x rounds the vertical edges, y the
    // horizontal ones, z is the taper.
    r: vec4<f32>,
    inverse_rotation: vec4<f32>, // quaternion, xyzw
    albedo: vec3<f32>, // linear RGB
    chamfer: u32,
    // Half the axis-aligned box used to reject this shape cheaply. Not the
    // shape's own bounding box: it is inflated so that cull_scale times the
    // distance to it is a true lower bound on what shape_distance returns.
    cull_extent: vec3<f32>,
    // How much the evaluator can undershoot the real distance. 1.0 where the
    // primitive is exact, below it where the estimate is conservative.
    cull_scale: f32,
    blend: Blend,
};

/// One light. Must match `struct GpuLight` in src/sdf/light.rs exactly.
struct GpuLight {
    position: vec3<f32>,
    kind: u32,
    direction: vec3<f32>, // unit, where it points
    range: f32,
    colour: vec3<f32>,
    intensity: f32,
    cos_inner: f32, // cosines, so the cone test is a dot product
    cos_outer: f32,
    shadow: u32,
    softness: f32,
};

struct Blend {
    mode: u32,
    radius: f32,   // k in SDF Modeler's shaders
    strength: f32, // r in SDF Modeler's shaders
    padding: f32,
};

const BRUSH_SPHERE: u32 = 0u;
const BRUSH_CUBE: u32 = 1u;
const BRUSH_CYLINDER: u32 = 2u;

// Blend modes, values as SDF Modeler's common.glsl defines them.
const MODE_ADD: u32 = 0u;
const MODE_SUBTRACT: u32 = 1u;
const MODE_INTERSECT: u32 = 2u;
const MODE_PAINT: u32 = 3u;
const MODE_PUSH: u32 = 4u;
const MODE_AVOID: u32 = 5u;
const MODE_EMBOSS: u32 = 6u;
const MODE_DEBOSS: u32 = 7u;
const MODE_SHELL: u32 = 8u;

/// A cell that gave up indexing: evaluate every shape in it.
const GRID_CELL_FULL: u32 = 4294967295u;

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> render_params: RenderParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var<storage, read> shapes: array<Shape>;
// Two words per cell: offset into grid_indices, then count.
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var<storage, read> grid_cells: array<u32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var<storage, read> grid_indices: array<u32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var<storage, read> lights: array<GpuLight>;

const MAX_MARCH_STEPS: i32 = 128;
const MAX_MARCH_DISTANCE: f32 = 100.0;
const SURFACE_THRESHOLD: f32 = 0.001; // floor for the distance-scaled threshold
const NORMAL_EPSILON: f32 = 0.0005; // mirrored by SURFACE_EPSILON in field.rs
const MIN_RADIUS: f32 = 1e-5; // mirrored by MIN_RADIUS in field.rs
const AMBIENT: f32 = 0.05;

const LIGHT_DIRECTIONAL: u32 = 0u;
const LIGHT_POINT: u32 = 1u;
const LIGHT_SPOT: u32 = 2u;

/// Where a shadow ray starts, along the normal. Below this the surface shadows
/// itself: the ray begins inside its own geometry and reports an immediate hit,
/// which draws as a black stipple over everything lit.
const SHADOW_BIAS: f32 = 0.02;
// Steps come from render_params.shadow_steps: a shadow that gives up reads as
// lit, which is the forgiving direction, so the count is a knob rather than a
// correctness matter.

// ------------------------------------------------------------------ the field

/// Rotates a vector by a quaternion without building a matrix: two cross
/// products beat nine multiplies and a normalize.
fn rotate_by_quaternion(offset: vec3<f32>, rotation: vec4<f32>) -> vec3<f32> {
    return offset + 2.0 * cross(rotation.xyz, cross(rotation.xyz, offset) + rotation.w * offset);
}

/// Superellipsoid. `exponent` 2 is a plain ellipsoid; larger values square it
/// off towards a bevelled box, which is the editor's Sharpen modifier. Scaling
/// by the smallest radius is what keeps it from overshooting.
fn ellipsoid_distance(local_position: vec3<f32>, unclamped_radii: vec3<f32>, exponent: f32) -> f32 {
    let radii = max(unclamped_radii, vec3<f32>(MIN_RADIUS));
    let scaled = pow(abs(local_position / radii), vec3<f32>(exponent));
    return (pow(scaled.x + scaled.y + scaled.z, 1.0 / exponent) - 1.0)
        * min(radii.x, min(radii.y, radii.z));
}

/// Exact distance to an ellipse, by five Newton steps around the boundary.
/// Exact matters here because it is the cylinder's whole cross-section.
fn ellipse_distance(unsigned_point: vec2<f32>, unclamped_radii: vec2<f32>) -> f32 {
    let point = abs(unsigned_point);
    let radii = max(unclamped_radii, vec2<f32>(MIN_RADIUS));
    // The iteration has nothing to walk towards from dead centre, and divides
    // by zero when it tries. The answer there is known anyway.
    if dot(point, point) < MIN_RADIUS * MIN_RADIUS {
        return -min(radii.x, radii.y);
    }
    let offset = radii * (point - radii);
    var direction = normalize(select(vec2<f32>(1.0, 0.01), vec2<f32>(0.01, 1.0), offset.x < offset.y));

    for (var step = 0; step < 5; step++) {
        let along = radii * direction;
        let across = radii * vec2<f32>(-direction.y, direction.x);
        let a = dot(point - along, across);
        let c = dot(point - along, along) + dot(across, across);
        let b = sqrt(max(c * c - a * a, 0.0));
        direction = vec2<f32>(
            direction.x * b - direction.y * a,
            direction.y * b + direction.x * a,
        ) / max(c, MIN_RADIUS);
    }

    let distance = length(point - radii * direction);
    return select(-distance, distance, dot(point / radii, point / radii) > 1.0);
}

/// Elliptical cross-section, axis along Y.
fn cylinder_distance(local_position: vec3<f32>, radii: vec3<f32>) -> f32 {
    let radial = ellipse_distance(local_position.xz, radii.xz);
    let edge = vec2<f32>(radial, abs(local_position.y) - radii.y);
    return min(max(edge.x, edge.y), 0.0) + length(max(edge, vec2<f32>(0.0)));
}

/// The editor's uber primitive: one function covering box, rounded box,
/// cylinder, capsule, cone and tube.
///
/// `s.xyz` are half sizes and `s.w` is the wall thickness of the hollow form.
/// `r.x` rounds the four vertical edges, `r.y` the horizontal ones, and `r.z`
/// tapers the top, widening the bottom by that much in the process - which is
/// why the pack shrinks `s.xz` by the taper first.
fn uberprim_distance(local_position: vec3<f32>, packed_s: vec4<f32>, packed_r: vec3<f32>) -> f32 {
    var s = packed_s;
    var r = packed_r;
    s.x -= r.x;
    s.z -= r.x;
    r.x -= s.w;
    s.w -= r.y;
    s.y -= r.y;

    let bevel_axis = vec2<f32>(r.z, -2.0 * s.y);
    let squared = dot(bevel_axis, bevel_axis);
    let along = select(vec2<f32>(0.0), bevel_axis / squared, squared > 0.0);

    let corner = abs(local_position) - s.xyz;
    let flat = vec2<f32>(corner.x, corner.z);
    var radial = length(max(flat, vec2<f32>(0.0))) + min(max(flat.x, flat.y), 0.0) - r.x;
    radial = abs(radial) - s.w;

    let profile = vec2<f32>(radial, local_position.y - s.y);
    let diagonal = profile - vec2<f32>(r.z, bevel_axis.y) * clamp(dot(profile, along), 0.0, 1.0);
    let bottom = vec2<f32>(max(radial - r.z, 0.0), local_position.y + s.y);
    let top = vec2<f32>(max(radial, 0.0), local_position.y - s.y);

    let nearest = min(dot(diagonal, diagonal), min(dot(bottom, bottom), dot(top, top)));
    let outside = max(dot(profile, vec2<f32>(-along.y, along.x)), corner.y);
    return sqrt(nearest) * sign(outside) - r.y;
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
fn bore(wall: f32, unspent_taper: f32, remaining: f32) -> f32 {
    return min(wall + unspent_taper, remaining);
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
fn tapered_uberprim(local_position: vec3<f32>, s: vec4<f32>, r: vec4<f32>) -> f32 {
    let flat = min(s.x, s.z);
    if r.z <= 0.0 {
        return uberprim_distance(
            local_position,
            vec4<f32>(s.x, s.y, s.z, bore(s.w, 0.0, flat)),
            vec3<f32>(r.x, r.y, 0.0),
        );
    }
    let taper = r.z * flat;
    let height_fraction = clamp((local_position.y / s.y + 1.0) * 0.5, 0.0, 1.0);
    let inset = taper * height_fraction;

    let remaining = max(flat - inset, 0.0);
    let narrowed = vec4<f32>(s.x - inset, s.y, s.z - inset, bore(s.w, taper - inset, remaining));
    let corner = vec3<f32>(max(r.x - inset, 0.0), r.y, 0.0);

    let slope = taper / (2.0 * s.y);
    return uberprim_distance(local_position, narrowed, corner) / sqrt(1.0 + slope * slope);
}

fn shape_distance(shape: Shape, world_position: vec3<f32>) -> f32 {
    let local_position =
        rotate_by_quaternion(world_position - shape.center, shape.inverse_rotation);

    if shape.brush == BRUSH_SPHERE {
        return ellipsoid_distance(local_position, shape.s.xyz, shape.s.w);
    }
    if shape.brush == BRUSH_CYLINDER {
        return cylinder_distance(local_position, shape.s.xyz - shape.s.w) - shape.s.w;
    }
    return tapered_uberprim(local_position, shape.s, shape.r);
}

// ----------------------------------------------------------- blend operations

// Every operation takes the incoming shape first and the field built so far
// second, matching blend_op_ex in the editor's sdf.glsl.

fn union_smooth(shape: f32, field: f32, radius: f32) -> f32 {
    let mixture = clamp(0.5 + 0.5 * (field - shape) / radius, 0.0, 1.0);
    return mix(field, shape, mixture) - radius * mixture * (1.0 - mixture);
}

fn subtract_smooth(shape: f32, field: f32, radius: f32) -> f32 {
    let mixture = clamp(0.5 - 0.5 * (field + shape) / radius, 0.0, 1.0);
    return mix(field, -shape, mixture) + radius * mixture * (1.0 - mixture);
}

fn intersect_smooth(shape: f32, field: f32, radius: f32) -> f32 {
    let mixture = clamp(0.5 - 0.5 * (field - shape) / radius, 0.0, 1.0);
    return mix(field, shape, mixture) + radius * mixture * (1.0 - mixture);
}

fn op_union(shape: f32, field: f32, radius: f32, chamfer: bool) -> f32 {
    if chamfer {
        return min(min(shape, field), (shape - 0.5 * radius + field) * 0.5);
    }
    if radius > 0.0 {
        return union_smooth(shape, field, radius);
    }
    return min(shape, field);
}

fn op_intersect(shape: f32, field: f32, radius: f32, chamfer: bool) -> f32 {
    if chamfer {
        return max(max(shape, field), (field + 0.5 * radius + shape) * 0.5);
    }
    if radius > 0.0 {
        return intersect_smooth(shape, field, radius);
    }
    return max(shape, field);
}

fn op_subtract(shape: f32, field: f32, radius: f32, chamfer: bool) -> f32 {
    if chamfer {
        return op_intersect(-shape, field, radius, true);
    }
    if radius > 0.0 {
        return subtract_smooth(shape, field, radius);
    }
    return max(field, -shape);
}

fn blend_shape(shape: f32, field: f32, blend: Blend, chamfer: bool) -> f32 {
    let radius = blend.radius;
    let strength = blend.strength;
    switch blend.mode {
        case 1u: { return op_subtract(shape, field, radius, chamfer); }
        case 2u: { return op_intersect(shape, field, radius, chamfer); }
        // Paint only recolours, so the field is left exactly as it was.
        case 3u: { return field; }
        case 4u: { return min(op_subtract(shape - strength, field, radius, chamfer), shape); }
        case 5u: { return min(op_subtract(field - strength, shape, radius, chamfer), field); }
        case 6u: {
            return op_union(
                field,
                op_intersect(shape, field - strength, radius, chamfer),
                radius,
                chamfer,
            );
        }
        case 7u: {
            return op_subtract(
                op_subtract(field + strength, shape, radius, chamfer),
                field,
                radius,
                chamfer,
            );
        }
        case 8u: {
            return op_intersect(shape, abs(field + strength) - strength, radius, chamfer);
        }
        default: { return op_union(shape, field, radius, chamfer); }
    }
}

// ------------------------------------------------------------------ the scene

/// Distance to a shape's cull box. Zero inside it, where no useful bound
/// exists.
///
/// Mirrors `cull_box_distance` in field.rs.
fn cull_box_distance(offset: vec3<f32>, half_extent: vec3<f32>) -> f32 {
    return length(max(abs(offset) - half_extent, vec3<f32>(0.0)));
}

/// Whether a shape is far enough away that blending it in would leave the field
/// exactly as it is, so the expensive evaluation can be skipped.
///
/// Only sound for MODE_ADD. A union takes the nearer of the two, so a shape
/// that cannot get nearer changes nothing; the box is already inflated by
/// blend.radius / cull_scale, which covers the reach of a smooth or chamfered
/// union. Every other mode reads the field through a different formula and
/// needs its own proof. The box and the scale are both built on the CPU by
/// `cull_bound` in field.rs - the shader only reads them.
///
/// Mirrors `shape_cannot_reach` in field.rs.
fn shape_cannot_reach(shape: Shape, world_position: vec3<f32>, field: f32) -> bool {
    return render_params.cull != 0u
        && shape.blend.mode == MODE_ADD
        && shape.cull_scale * cull_box_distance(world_position - shape.center, shape.cull_extent)
            >= field;
}

/// Every shape, in blend order, with the box reject in front of the expensive
/// part. Each blends against everything before it, so the first simply seeds
/// the field - the editor does not apply an operation to it either.
///
/// `scene_distance_gridded` is what the camera march calls; this is the
/// fallback for a point outside the grid, and the definition the grid must
/// agree with.
///
/// ponytail: linear over the whole scene. The grid is what keeps that off the
/// hot path, so the ceiling here is a cell that lists most of the scene - a
/// level built as one giant brush, or too many non-ADD modes, which are in
/// every cell by construction.
fn scene_distance(world_position: vec3<f32>) -> f32 {
    var field = MAX_MARCH_DISTANCE;
    for (var i = 0u; i < render_params.shape_count; i++) {
        let shape = shapes[i];
        if i > 0u && shape_cannot_reach(shape, world_position, field) {
            continue;
        }
        let distance = shape_distance(shape, world_position);
        if i == 0u {
            field = distance;
        } else {
            field = blend_shape(distance, field, shape.blend, shape.chamfer != 0u);
        }
    }
    return field;
}

// ------------------------------------------------------------------- the grid

/// Cell holding a point, clamped to the grid. Mirrors `SdfGrid::cell_of`.
fn grid_slot(world_position: vec3<f32>) -> vec3<f32> {
    return clamp(
        floor((world_position - render_params.grid_origin) / render_params.grid_cell),
        vec3<f32>(0.0),
        vec3<f32>(render_params.grid_resolution - vec3<u32>(1u)),
    );
}

fn grid_cell(world_position: vec3<f32>) -> u32 {
    let slot = grid_slot(world_position);
    return u32(slot.x)
        + u32(slot.y) * render_params.grid_resolution.x
        + u32(slot.z) * render_params.grid_resolution.x * render_params.grid_resolution.y;
}

/// Whether a point is inside the grid volume at all. Mirrors `SdfGrid::holds`.
///
/// Outside it there is no cell to clamp to: the lookup lands in an edge cell
/// whose wall the point is already past, the wall distance comes out zero, and
/// the march reads that as a surface. The camera usually starts outside the
/// scene bounds, so this is the common case.
fn grid_holds(world_position: vec3<f32>) -> bool {
    let high = render_params.grid_origin
        + render_params.grid_cell * vec3<f32>(render_params.grid_resolution);
    return all(world_position >= render_params.grid_origin) && all(world_position <= high);
}

/// Distance from a point to the wall of its own cell. Mirrors
/// `SdfGrid::exit_distance`.
///
/// Load-bearing. A cell only knows its own shapes, so its field can be far too
/// large - the next cell may hold a surface one step away. Clamping to the wall
/// makes the answer conservative again, because nothing outside the cell can be
/// reached without crossing it.
fn grid_exit_distance(world_position: vec3<f32>) -> f32 {
    let slot = grid_slot(world_position);
    // Cells overlap by half a cell: the box measured against is bigger than
    // the box that chose it. A thin margin instead would let a ray travelling
    // *along* a wall report almost zero at every step, crawl, run out of budget
    // and vanish - which drew as a slice missing down the middle of the screen.
    // `build_grid` rasterises every shape into the same overlapping boxes.
    let overlap = render_params.grid_cell * 0.5;
    let low = render_params.grid_origin + slot * render_params.grid_cell - overlap;
    let high = render_params.grid_origin
        + (slot + vec3<f32>(1.0)) * render_params.grid_cell + overlap;
    let to_wall = min(world_position - low, high - world_position);
    return max(min(to_wall.x, min(to_wall.y, to_wall.z)), 0.0);
}

/// The field, evaluated through the grid. Mirrors `scene_distance_gridded` in
/// field.rs.
///
/// A cell lists, in blend order, shape 0, every shape whose mode is not ADD,
/// and every ADD shape whose cull box overlaps it. The non-ADD modes are in
/// every cell because they read the field itself - one of them on the far side
/// of the level still changes the answer here.
fn scene_distance_gridded(world_position: vec3<f32>) -> f32 {
    if render_params.grid == 0u || !grid_holds(world_position) {
        return scene_distance(world_position);
    }
    let cell = grid_cell(world_position);
    let count = grid_cells[cell * 2u + 1u];
    // A cell that gave up indexing evaluates everything: slower, never wrong.
    if count == GRID_CELL_FULL {
        return scene_distance(world_position);
    }

    let offset = grid_cells[cell * 2u];
    var field = MAX_MARCH_DISTANCE;
    var evaluated = 0u;
    for (var slot = 0u; slot < count; slot++) {
        let shape = shapes[grid_indices[offset + slot]];
        if evaluated > 0u && shape_cannot_reach(shape, world_position, field) {
            continue;
        }
        let distance = shape_distance(shape, world_position);
        if evaluated == 0u {
            field = distance;
        } else {
            field = blend_shape(distance, field, shape.blend, shape.chamfer != 0u);
        }
        evaluated++;
    }

    // Only a cell already holding every shape may report its field unclamped.
    if count == render_params.shape_count {
        return field;
    }
    return min(field, grid_exit_distance(world_position));
}

/// One shape's contribution to the shadow proxy: the cheapest shape that still
/// *contains* the real one, in the real one's own frame.
///
/// Containment is the whole argument. A shape that swallows another is never
/// further from a point than the shape inside it, so this can only report a
/// shorter distance than the field - which a march may act on, because acting
/// early is stopping early.
///
/// Deliberately not `cull_extent`: that box carries the blend radius and is
/// axis-aligned about `center`, which drew shadows several times the size of
/// what cast them and slabs where a rotated plate stood.
///
/// Non-ADD modes sit this out. They read the field rather than adding to it, so
/// nothing about their own extent bounds what they do to it.
fn shadow_proxy_bound(shape: Shape, world_position: vec3<f32>) -> f32 {
    if shape.blend.mode != MODE_ADD {
        return MAX_MARCH_DISTANCE;
    }
    let local = rotate_by_quaternion(world_position - shape.center, shape.inverse_rotation);

    if shape.brush == BRUSH_SPHERE {
        // The field's own estimate. It is already compiled in for the camera
        // march and costs three `pow`, nothing like the uberprim.
        return ellipsoid_distance(local, shape.s.xyz, shape.s.w);
    }
    if shape.brush == BRUSH_CYLINDER {
        // A round cross-section at the wider radius, which contains the ellipse
        // the shape actually has - and is exact for a round cylinder, which is
        // most of them. `ellipse_distance` is five Newton steps and is the one
        // thing in the field more expensive than the uberprim.
        let radial = length(local.xz) - max(shape.s.x, shape.s.z);
        let edge = vec2<f32>(radial, abs(local.y) - shape.s.y);
        return min(max(edge.x, edge.y), 0.0) + length(max(edge, vec2<f32>(0.0)));
    }
    // Every cube modifier only ever removes material - round and bevel cut the
    // edges, cone narrows the top, thickness hollows an interior nothing sees
    // from outside. So the plain box contains all of them, and is exact for a
    // cube nobody has touched.
    return cull_box_distance(local, shape.s.xyz);
}

/// A lower bound on the field, from cull boxes alone. Mirrors
/// `shadow_proxy_distance` in field.rs.
///
/// This exists so that `shape_distance` is called from **one** place in this
/// shader. A second call site costs 13.9 ms on `spread:80` through register
/// pressure alone - whether or not a light casts, and whether the shadow march
/// takes 12 steps or 48. Measured 2026-09-01, table in memory/reference/lights.md.
///
/// Still wrong in two visible ways, both of them shape rather than size. A cube
/// with a heavy round or taper casts the shadow of the box it started as. And
/// only ADD contributes, so a hole carved by SUBTRACT does not let light
/// through - which needs the field, and the field is what this avoids.
fn shadow_proxy_distance(world_position: vec3<f32>) -> f32 {
    var field = MAX_MARCH_DISTANCE;
    var count = 0u;
    var offset = 0u;
    var indexed = false;

    if render_params.grid != 0u && grid_holds(world_position) {
        let cell = grid_cell(world_position);
        count = grid_cells[cell * 2u + 1u];
        if count != GRID_CELL_FULL {
            offset = grid_cells[cell * 2u];
            indexed = true;
        }
    }

    if !indexed {
        for (var i = 0u; i < render_params.shape_count; i++) {
            field = min(field, shadow_proxy_bound(shapes[i], world_position));
        }
        return field;
    }

    for (var slot = 0u; slot < count; slot++) {
        let shape = shapes[grid_indices[offset + slot]];
        field = min(field, shadow_proxy_bound(shape, world_position));
    }
    if count == render_params.shape_count {
        return field;
    }
    return min(field, grid_exit_distance(world_position));
}

// ------------------------------------------------------------ surface queries

/// Colour of the nearest shape that puts material there. Kept separate from
/// `scene_distance` on purpose: marching only needs the distance, and carrying
/// a colour through every step of every ray costs far more than one lookup at
/// the surface.
fn scene_albedo(world_position: vec3<f32>) -> vec3<f32> {
    var nearest = MAX_MARCH_DISTANCE;
    var colour = vec3<f32>(0.5);
    for (var i = 0u; i < render_params.shape_count; i++) {
        let shape = shapes[i];
        if shape.blend.mode == MODE_SUBTRACT || shape.blend.mode == MODE_INTERSECT {
            continue;
        }
        let distance = shape_distance(shape, world_position);
        if distance < nearest {
            nearest = distance;
            colour = shape.albedo;
        }
    }
    return colour;
}

/// Surface normal of the field, from four taps arranged as a tetrahedron
/// instead of six along the axes. Mirrors `scene_normal` in field.rs.
fn surface_normal(surface_point: vec3<f32>) -> vec3<f32> {
    let offset = vec2<f32>(1.0, -1.0) * NORMAL_EPSILON;
    return normalize(
        offset.xyy * scene_distance_gridded(surface_point + offset.xyy) +
        offset.yyx * scene_distance_gridded(surface_point + offset.yyx) +
        offset.yxy * scene_distance_gridded(surface_point + offset.yxy) +
        offset.xxx * scene_distance_gridded(surface_point + offset.xxx)
    );
}

/// Where the ray enters and leaves the box holding the whole scene. Slab test.
/// Exit below entry means the ray misses everything, and marching it at all is
/// wasted work - which is most of the sky.
fn scene_bounds_span(ray_origin: vec3<f32>, ray_direction: vec3<f32>) -> vec2<f32> {
    let inverse_direction = 1.0 / ray_direction;
    let to_min = (render_params.bounds_min - ray_origin) * inverse_direction;
    let to_max = (render_params.bounds_max - ray_origin) * inverse_direction;
    let nearest = min(to_min, to_max);
    let farthest = max(to_min, to_max);
    let entry = max(max(nearest.x, nearest.y), nearest.z);
    let exit = min(min(farthest.x, farthest.y), farthest.z);
    return vec2<f32>(max(entry, 0.0), min(exit, MAX_MARCH_DISTANCE));
}

// -------------------------------------------------------- vertex: just a quad

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
};

struct QuadVertex {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
};

// A coarse cone-march used to run here, one ray per cell, handing each pixel a
// head start. The acceleration grid made the per-pixel march cheap enough that
// it was worth 4% on a sparse scene and nothing on a dense one - against a
// whole vertex stage, and two of the worst bugs this project has had. Both came
// from the same root: a vertex value is *interpolated* across its cell, so it
// has to be conservative for every pixel in that cell, and nothing here can
// test that. Deleted 2026-08-31; see memory/decision/cone-marching.md.
@vertex
fn vertex(vertex: Vertex) -> QuadVertex {
    var result: QuadVertex;
    result.world_position = mesh_position_local_to_world(
        get_world_from_local(vertex.instance_index),
        vec4<f32>(vertex.position, 1.0),
    );
    result.clip_position = view.clip_from_world * result.world_position;
    return result;
}

// ---------------------------------------------------- fragment: ray per pixel

// Half a pixel, measured at one unit of distance. Anything smaller than this
// cannot be seen, so chasing it is wasted marching - which is exactly what a
// grazing ray along a flat surface does with a fixed threshold.
fn pixel_radius_per_unit() -> f32 {
    return render_params.tan_half_fov / max(view.viewport.w, 1.0);
}

/// Returns distance travelled and steps spent.
fn ray_march(
    start_distance: f32,
    step_budget: i32,
    ray_origin: vec3<f32>,
    ray_direction: vec3<f32>,
    stop_distance: f32,
) -> vec2<f32> {
    let precision_per_unit = pixel_radius_per_unit();
    var travelled = start_distance;
    var step = 0;

    // Over-relaxation (Keinert et al., Enhanced Sphere Tracing): step further
    // than the unbounding sphere allows, then check afterwards that it was
    // safe. Two consecutive spheres that overlap prove nothing was skipped
    // between them; two that do not, prove nothing at all - so that is the
    // failure test, and it is exact wherever the field is a true bound.
    var relaxation = max(render_params.omega, 1.0);
    var previous_distance = 0.0;
    var step_length = 0.0;

    loop {
        // Out of budget is a **miss**, not a surface. Returning wherever the
        // ray happened to stop makes the fragment stage shade an arbitrary
        // point in mid-air, which draws as banding across the whole image. It
        // was always wrong; the grid made it common by crawling cell to cell.
        if step >= step_budget {
            return vec2<f32>(stop_distance, f32(step));
        }
        let distance = scene_distance_gridded(ray_origin + ray_direction * travelled);
        let overshot = relaxation > 1.0 && (abs(distance) + previous_distance) < step_length;
        let close_enough = max(SURFACE_THRESHOLD, travelled * precision_per_unit);
        step++;

        // A hit is only believable when the last step was safe. After an
        // overshoot this point may be *past* a surface, not on one.
        //
        // And a small distance is not proof of a surface: the grid clamps to
        // the cell wall, so a point near a wall reports the margin, not the
        // geometry. Confirm against the exact field before stopping. It costs
        // one full evaluation, and only where a hit already looks likely.
        if !overshot && distance < close_enough
            && scene_distance(ray_origin + ray_direction * travelled) < close_enough {
            travelled += distance;
            break;
        }

        if overshot {
            // Undo the part of the last step the spheres did not cover, and
            // finish the ray at plain sphere tracing.
            step_length = step_length * (1.0 - relaxation);
            relaxation = 1.0;
        } else {
            step_length = distance * relaxation;
        }
        previous_distance = abs(distance);
        travelled += step_length;
        if travelled >= stop_distance {
            break;
        }
    }
    return vec2<f32>(travelled, f32(step));
}

/// Cheap blue-green-yellow-red ramp for the step heatmap.
fn heat(fraction: f32) -> vec3<f32> {
    let t = clamp(fraction, 0.0, 1.0);
    return clamp(
        vec3<f32>(3.0 * t - 1.2, 1.6 - abs(3.0 * t - 1.6), 1.2 - 3.0 * t),
        vec3<f32>(0.0),
        vec3<f32>(1.0),
    );
}

/// How much of the light reaches a point, 0 shadowed to 1 clear.
///
/// Inigo Quilez's penumbra trick: march toward the light, and track the
/// smallest ratio of distance to travelled distance. What it marches against is
/// [`shadow_proxy_distance`], not the field - see there for why, and for what
/// that costs in accuracy. A ray that squeezes
/// past an edge returns a partial value, which is a soft shadow for the price
/// of the march that was already happening. `softness` scales that ratio -
/// higher is sharper, so it is inverted from what the name suggests on the CPU
/// side, where it reads as penumbra width.
fn shadow_factor(origin: vec3<f32>, direction: vec3<f32>, far: f32, softness: f32) -> f32 {
    var shade = 1.0;
    var travelled = SHADOW_BIAS;
    for (var step = 0u; step < render_params.shadow_steps; step++) {
        if travelled >= far {
            break;
        }
        let distance = shadow_proxy_distance(origin + direction * travelled);
        if distance < SURFACE_THRESHOLD {
            return 0.0;
        }
        shade = min(shade, softness * distance / travelled);
        travelled += distance;
    }
    return clamp(shade, 0.0, 1.0);
}

/// Everything one light adds at a point, shadow included.
fn light_contribution(light: GpuLight, surface_point: vec3<f32>, normal: vec3<f32>) -> vec3<f32> {
    var to_light = -light.direction;
    var distance_to_light = MAX_MARCH_DISTANCE;
    var attenuation = 1.0;

    if light.kind != LIGHT_DIRECTIONAL {
        let offset = light.position - surface_point;
        distance_to_light = length(offset);
        // Out of range is not "very dim", it is nothing: the falloff is scaled
        // so it reaches zero at `range`, which is what makes the range a real
        // cull rather than a suggestion.
        if distance_to_light >= light.range || distance_to_light < 1e-4 {
            return vec3<f32>(0.0);
        }
        to_light = offset / distance_to_light;
        let fraction = distance_to_light / light.range;
        let falloff = clamp(1.0 - fraction * fraction, 0.0, 1.0);
        attenuation = falloff * falloff;

        if light.kind == LIGHT_SPOT {
            // Inside the inner cone is full, outside the outer is nothing, and
            // between them it eases rather than stepping.
            let alignment = dot(light.direction, -to_light);
            attenuation *= smoothstep(light.cos_outer, light.cos_inner, alignment);
        }
    }

    let facing = max(dot(normal, to_light), 0.0);
    if facing <= 0.0 || attenuation <= 0.0 {
        return vec3<f32>(0.0);
    }

    var visibility = 1.0;
    if light.shadow != 0u {
        let reach = min(distance_to_light, MAX_MARCH_DISTANCE);
        visibility = shadow_factor(
            surface_point + normal * SHADOW_BIAS,
            to_light,
            reach,
            max(light.softness, 1e-3),
        );
    }
    return light.colour * (light.intensity * facing * attenuation * visibility);
}

/// Every light, added up. Lights out of range cost a length and a compare.
fn shade(surface_point: vec3<f32>, normal: vec3<f32>) -> vec3<f32> {
    var total = vec3<f32>(AMBIENT);
    for (var index = 0u; index < render_params.light_count; index++) {
        total += light_contribution(lights[index], surface_point, normal);
    }
    return total;
}

/// Writing depth is what lets ordinary Bevy 3D entities share this world. The
/// quad sits one unit from the camera, so without this every SDF pixel would
/// claim the quad's depth and anything placed in the scene would sort wrong.
struct FragmentOutput {
    @builtin(frag_depth) depth: f32,
    @location(0) color: vec4<f32>,
};

/// Depth of a world point in whatever convention the projection uses, so
/// reverse-Z needs no special casing here.
fn depth_of(world_point: vec3<f32>) -> f32 {
    let clip = view.clip_from_world * vec4<f32>(world_point, 1.0);
    return clip.z / clip.w;
}

@fragment
fn fragment(quad: QuadVertex) -> FragmentOutput {
    let ray_origin = view.world_position;
    let ray_direction = normalize(quad.world_position.xyz - ray_origin);
    let span = scene_bounds_span(ray_origin, ray_direction);
    var march = vec2<f32>(MAX_MARCH_DISTANCE, 0.0);
    if span.y >= span.x {
        march = ray_march(span.x, MAX_MARCH_STEPS, ray_origin, ray_direction, span.y);
    }
    let travelled = march.x;

    var output: FragmentOutput;

    if render_params.debug_view == 1u {
        let steps_used = march.y;
        output.depth = 0.0;
        output.color = vec4<f32>(heat(steps_used / f32(MAX_MARCH_STEPS)), 1.0);
        return output;
    }
    if travelled >= span.y {
        discard;
    }

    let surface_point = ray_origin + ray_direction * travelled;
    let albedo = scene_albedo(surface_point);
    let light = shade(surface_point, surface_normal(surface_point));

    output.depth = depth_of(surface_point);
    output.color = vec4<f32>(albedo * light, 1.0);
    return output;
}
