// Signed distance field renderer: cone marching in the vertex stage, ray
// marching in the fragment stage, one draw call over a frustum-fitted quad.
//
// Technique: https://medium.com/@nabilnymansour/cone-marching-in-three-js-6d54eac17ad4
//
// Every function under "the field" is mirrored by one of the same name in
// src/main.rs, which the physics uses. Both read the same packed `Shape`
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
    vertical_cell_count: f32,
    cone_padding: f32, // 0 disables the cone pass entirely
    shape_count: u32, // the buffer is a fixed size; only this many are real
    debug_view: u32, // 0 = shaded, 1 = march-step heatmap
    padding: u32,
};

/// One primitive, packed by `GpuShape::to_gpu` on the Rust side. Byte layout
/// must match `struct GpuShape` in src/main.rs exactly.
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
    blend: Blend,
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

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> render_params: RenderParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var<storage, read> shapes: array<Shape>;

const MAX_MARCH_STEPS: i32 = 128;
const MAX_MARCH_DISTANCE: f32 = 100.0;
const SURFACE_THRESHOLD: f32 = 0.001; // floor for the distance-scaled threshold
const NORMAL_EPSILON: f32 = 0.0005; // mirrored by SURFACE_EPSILON in main.rs
const MIN_RADIUS: f32 = 1e-5; // mirrored by MIN_RADIUS in main.rs
const SUN_DIRECTION: vec3<f32> = vec3<f32>(0.48, 0.8, 0.36);
const AMBIENT: f32 = 0.05;

// ------------------------------------------------------- the field

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

// ponytail: linear scan, O(shapes) per march step. Fine to ~50 shapes.
// Needs a spatial grid or BVH before it scales past that.
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

// ---------------------------------------------------------- blend operations

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

/// Combines one shape with everything already in the field.
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

/// Shapes are applied in order and each one blends against everything before
/// it, so the first shape simply seeds the field - the editor does not apply an
/// operation to it either.
fn scene_distance(world_position: vec3<f32>) -> f32 {
    var field = MAX_MARCH_DISTANCE;
    for (var i = 0u; i < render_params.shape_count; i++) {
        let shape = shapes[i];
        let distance = shape_distance(shape, world_position);
        if i == 0u {
            field = distance;
        } else {
            field = blend_shape(distance, field, shape.blend, shape.chamfer != 0u);
        }
    }
    return field;
}

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
/// instead of six along the axes. Mirrors `scene_normal` in main.rs.
fn surface_normal(surface_point: vec3<f32>) -> vec3<f32> {
    let offset = vec2<f32>(1.0, -1.0) * NORMAL_EPSILON;
    return normalize(
        offset.xyy * scene_distance(surface_point + offset.xyy) +
        offset.yyx * scene_distance(surface_point + offset.yyx) +
        offset.yxy * scene_distance(surface_point + offset.yxy) +
        offset.xxx * scene_distance(surface_point + offset.xxx)
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

// ------------------------------------------------- pass 1: cone per vertex

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
};

struct ConeResult {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    @location(1) travelled_and_steps: vec2<f32>,
};

fn cone_march(ray_origin: vec3<f32>, ray_direction: vec3<f32>, span: vec2<f32>) -> vec2<f32> {
    if render_params.cone_padding <= 0.0 {
        return vec2<f32>(span.x, 0.0);
    }
    let radius_per_unit_distance = 2.0 * render_params.tan_half_fov
        / render_params.vertical_cell_count
        * render_params.cone_padding;

    var travelled = span.x;
    var steps = 0;
    var approached_surface = false;
    loop {
        if steps >= MAX_MARCH_STEPS {
            break;
        }
        let distance = scene_distance(ray_origin + ray_direction * travelled);
        let cone_radius = travelled * radius_per_unit_distance;
        // Stop before stepping: the cone may no longer be empty, and a vertex
        // that overshoots pushes every pixel in its cell past the surface.
        if distance < cone_radius {
            approached_surface = true;
            break;
        }
        if travelled >= span.y {
            break;
        }
        travelled += distance;
        steps++;
    }
    // A vertex that reached the far side of the box without meeting anything
    // has no head start to offer, and the distance it covered is meaningless to
    // its neighbours - interpolating it dents the silhouette of whatever they
    // hit. Only a vertex that came near a surface shares its progress.
    if !approached_surface {
        return vec2<f32>(span.x, f32(steps));
    }
    return vec2<f32>(travelled, f32(steps));
}

@vertex
fn vertex(vertex: Vertex) -> ConeResult {
    var result: ConeResult;
    result.world_position = mesh_position_local_to_world(
        get_world_from_local(vertex.instance_index),
        vec4<f32>(vertex.position, 1.0),
    );
    result.clip_position = view.clip_from_world * result.world_position;

    let ray_origin = view.world_position;
    let ray_direction = normalize(result.world_position.xyz - ray_origin);
    let span = scene_bounds_span(ray_origin, ray_direction);
    if span.y < span.x {
        // Misses the scene box, so there is nothing to skip ahead to. This has
        // to stay 0 rather than something large: the value is interpolated
        // across the cell, and a big number here would push the fragments of
        // every neighbouring pixel - ones that do hit - straight past the
        // surface. The fragment stage runs its own slab test anyway.
        result.travelled_and_steps = vec2<f32>(0.0, 0.0);
    } else {
        result.travelled_and_steps = cone_march(ray_origin, ray_direction, span);
    }
    return result;
}

// ------------------------------------------------- pass 2: ray per pixel

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
    loop {
        if step >= step_budget {
            break;
        }
        let distance = scene_distance(ray_origin + ray_direction * travelled);
        let close_enough = max(SURFACE_THRESHOLD, travelled * precision_per_unit);
        travelled += distance;
        step++;
        if distance < close_enough || travelled >= stop_distance {
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
fn fragment(cone: ConeResult) -> FragmentOutput {
    let ray_origin = view.world_position;
    let ray_direction = normalize(cone.world_position.xyz - ray_origin);
    let span = scene_bounds_span(ray_origin, ray_direction);
    var march = vec2<f32>(MAX_MARCH_DISTANCE, 0.0);
    if span.y >= span.x {
        march = ray_march(
            max(cone.travelled_and_steps.x, span.x),
            MAX_MARCH_STEPS - i32(cone.travelled_and_steps.y),
            ray_origin,
            ray_direction,
            span.y,
        );
    }
    let travelled = march.x;

    var output: FragmentOutput;

    if render_params.debug_view == 1u {
        let steps_used = cone.travelled_and_steps.y + march.y;
        output.depth = 0.0;
        output.color = vec4<f32>(heat(steps_used / f32(MAX_MARCH_STEPS)), 1.0);
        return output;
    }
    if travelled >= span.y {
        discard;
    }

    let surface_point = ray_origin + ray_direction * travelled;
    let sun_strength = max(dot(surface_normal(surface_point), SUN_DIRECTION), 0.0);
    let albedo = scene_albedo(surface_point);

    output.depth = depth_of(surface_point);
    output.color = vec4<f32>(albedo * (AMBIENT + (1.0 - AMBIENT) * sun_strength), 1.0);
    return output;
}
