struct RenderParams {
    bounds_min: vec3<f32>,
    tan_half_fov: f32,
    bounds_max: vec3<f32>,
    padding_one: f32,
    shape_count: u32,
    debug_view: u32,
    cull: u32,
    omega: f32,
    grid: u32,
    grid_padding: u32,
    grid_padding_two: u32,
    grid_origin: vec3<f32>,
    grid_padding_three: f32,
    grid_cell: vec3<f32>,
    grid_padding_four: f32,
    grid_resolution: vec3<u32>,
    light_count: u32,
    shadow_steps: u32,
    detail: f32,
    hierarchy: u32,
    coarse_scale: f32,
};

struct Shape {
    center: vec3<f32>,
    wall_thickness: f32,
    half_size: vec3<f32>,
    side_radius: f32,
    inverse_rotation: vec4<f32>,
    albedo: vec3<f32>,
    cap_radius: f32,
    cull_extent: vec3<f32>,
    cull_scale: f32,
    taper: f32,
    padding_one: f32,
    padding_two: f32,
    padding_three: f32,
    blend: Blend,
};

struct GpuLight {
    position: vec3<f32>,
    kind: u32,
    direction: vec3<f32>,
    range: f32,
    colour: vec3<f32>,
    intensity: f32,
    cos_inner: f32,
    cos_outer: f32,
    shadow: u32,
    softness: f32,
};

struct Blend {
    mode: u32,
    radius: f32,
    strength: f32,
    chamfer: u32,
};

const MODE_ADD: u32 = 0u;
const MODE_SUBTRACT: u32 = 1u;
const MODE_INTERSECT: u32 = 2u;
const MODE_PAINT: u32 = 3u;
const MODE_PUSH: u32 = 4u;
const MODE_AVOID: u32 = 5u;
const MODE_EMBOSS: u32 = 6u;
const MODE_DEBOSS: u32 = 7u;
const MODE_SHELL: u32 = 8u;

const GRID_CELL_FULL: u32 = 4294967295u;

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> render_params: RenderParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var<storage, read> shapes: array<Shape>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var<storage, read> grid_cells: array<u32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var<storage, read> grid_indices: array<u32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var<storage, read> lights: array<GpuLight>;

const MAX_MARCH_STEPS: i32 = 128;
const MAX_MARCH_DISTANCE: f32 = 100.0;
const SURFACE_THRESHOLD: f32 = 0.001;
const NORMAL_EPSILON: f32 = 0.0005;
const MIN_RADIUS: f32 = 1e-5;
const AMBIENT: f32 = 0.05;

const LIGHT_DIRECTIONAL: u32 = 0u;
const LIGHT_POINT: u32 = 1u;
const LIGHT_SPOT: u32 = 2u;

const SHADOW_BIAS: f32 = 0.02;
